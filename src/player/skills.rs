//! Player skill system — `Skill` enum, `SkillSheet` component, and the
//! `skill_check` helper that drives lock-picking, persuasion, dialog branches,
//! and (eventually) Stealth/Perception/Survival/etc.
//!
//! See `docs/progression.md` §5 for the design (10 skills, d20-based check,
//! class/cross-class cost & cap split).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::game::resources::{
    GameEvent, GameUiEvent, PendingGameCommands, PendingGameEvents, PendingGameUiEvents,
};
use crate::player::classes::{ability_mod, class_data, Class};
use crate::player::components::{AttributeSet, BaseStats, ChatLog, Player, PlayerIdentity};
use crate::player::progression::Experience;

/// All ten skills, in their canonical ordering. Indexes into `SkillSheet.ranks`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Skill {
    Athletics,
    Stealth,
    Perception,
    Lore,
    Spellcraft,
    Persuasion,
    Survival,
    Heal,
    Thievery,
    Concentration,
}

impl Skill {
    pub const ALL: [Skill; 10] = [
        Skill::Athletics,
        Skill::Stealth,
        Skill::Perception,
        Skill::Lore,
        Skill::Spellcraft,
        Skill::Persuasion,
        Skill::Survival,
        Skill::Heal,
        Skill::Thievery,
        Skill::Concentration,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            Skill::Athletics => "Athletics",
            Skill::Stealth => "Stealth",
            Skill::Perception => "Perception",
            Skill::Lore => "Lore",
            Skill::Spellcraft => "Spellcraft",
            Skill::Persuasion => "Persuasion",
            Skill::Survival => "Survival",
            Skill::Heal => "Heal",
            Skill::Thievery => "Thievery",
            Skill::Concentration => "Concentration",
        }
    }

    /// Parse a `Skill` from its `label()` form, case-insensitive. Used by Yarn
    /// `<<skill_check Persuasion 15>>` and admin REPL commands.
    pub fn from_label(s: &str) -> Option<Skill> {
        Skill::ALL
            .iter()
            .copied()
            .find(|skill| skill.label().eq_ignore_ascii_case(s))
    }

    /// Which `AttributeSet` field this skill keys off, per `progression.md §5`.
    pub fn ability_score(self, attributes: &AttributeSet) -> i32 {
        match self {
            Skill::Athletics => attributes.strength,
            Skill::Stealth => attributes.agility,
            Skill::Perception => attributes.willpower,
            Skill::Lore => attributes.focus,
            Skill::Spellcraft => attributes.focus,
            Skill::Persuasion => attributes.charisma,
            Skill::Survival => attributes.willpower,
            Skill::Heal => attributes.willpower,
            Skill::Thievery => attributes.agility,
            Skill::Concentration => attributes.constitution,
        }
    }
}

/// Class skill table per `docs/progression.md §5.2`. Class skills cost 1
/// point/rank and cap at `level + 3`; cross-class costs 2/rank and caps at
/// `(level + 3) / 2`.
pub const fn class_skills(class: Class) -> &'static [Skill] {
    match class {
        Class::Fighter => &[
            Skill::Athletics,
            Skill::Perception,
            Skill::Concentration,
            Skill::Survival,
        ],
        Class::Wizard => &[
            Skill::Spellcraft,
            Skill::Lore,
            Skill::Concentration,
            Skill::Heal,
        ],
        Class::Cleric => &[
            Skill::Heal,
            Skill::Lore,
            Skill::Persuasion,
            Skill::Concentration,
            Skill::Spellcraft,
            Skill::Perception,
        ],
        Class::Vagabond => &[
            Skill::Stealth,
            Skill::Thievery,
            Skill::Perception,
            Skill::Persuasion,
            Skill::Athletics,
            Skill::Survival,
            Skill::Lore,
        ],
    }
}

pub fn is_class_skill(class: Class, skill: Skill) -> bool {
    class_skills(class).contains(&skill)
}

/// Skill points needed to buy one rank of `skill` for a `class`. 1 for class
/// skills, 2 for cross-class (per `progression.md §5.1`).
pub fn rank_cost(class: Class, skill: Skill) -> u32 {
    if is_class_skill(class, skill) {
        1
    } else {
        2
    }
}

/// Maximum rank a character of `class` at `level` may take in `skill`:
/// `level + 3` for class skills, `floor((level + 3) / 2)` for cross-class.
pub fn max_rank(class: Class, skill: Skill, level: u32) -> u8 {
    let level_plus_3 = level.saturating_add(3);
    let cap = if is_class_skill(class, skill) {
        level_plus_3
    } else {
        level_plus_3 / 2
    };
    cap.min(u8::MAX as u32) as u8
}

/// Skill points awarded on level-up. Uses `class_data().skill_points_per_level`
/// (Fighter/Wizard/Cleric = 2, Vagabond = 8) plus the character's Focus
/// modifier. Floor of 1 — even a low-Focus Fighter still gains something.
pub fn skill_points_for_level_up(class: Class, attributes: &AttributeSet) -> u32 {
    let base = class_data(class).skill_points_per_level as i32;
    let focus_mod = ability_mod(attributes.focus);
    (base + focus_mod).max(1) as u32
}

/// Per-character skill state. Lives as a `Component` on the player entity and
/// is persisted in `PlayerStateDump`. Ranks are indexed by `Skill::index()`.
#[derive(Component, Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct SkillSheet {
    pub ranks: [u8; 10],
    pub available_points: u32,
}

impl SkillSheet {
    pub fn rank(&self, skill: Skill) -> u8 {
        self.ranks[skill.index()]
    }

    pub fn set_rank(&mut self, skill: Skill, rank: u8) {
        self.ranks[skill.index()] = rank;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillCheckResult {
    pub roll: i32,
    pub total: i32,
    pub success: bool,
}

/// Pure skill-check math: `d20 + ranks + ability_mod + situational vs dc`. The
/// caller supplies the d20 roll so tests are deterministic and admin tooling
/// can short-circuit randomness.
pub fn resolve_skill_check(
    sheet: &SkillSheet,
    attributes: &AttributeSet,
    skill: Skill,
    dc: i32,
    situational: i32,
    roll: i32,
) -> SkillCheckResult {
    let ranks = sheet.rank(skill) as i32;
    let ability = ability_mod(skill.ability_score(attributes));
    let total = roll + ranks + ability + situational;
    SkillCheckResult {
        roll,
        total,
        success: total >= dc,
    }
}

/// Game-side wrapper: rolls a d20 and invokes `resolve_skill_check`. The
/// random source is the same time-mix-based scheme `world/loot.rs` uses, so
/// the project keeps a single conventional source of randomness rather than
/// pulling in a `rand` crate dependency.
pub fn skill_check(
    sheet: &SkillSheet,
    attributes: &AttributeSet,
    skill: Skill,
    dc: i32,
    situational: i32,
) -> SkillCheckResult {
    let roll = roll_d20();
    resolve_skill_check(sheet, attributes, skill, dc, situational, roll)
}

/// Roll 1..=20 using a time-based mix consistent with the rest of the
/// codebase (see `world::loot::roll_loot`).
pub fn roll_d20() -> i32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    // Splat over a u64 with a simple hash before clipping to 1..=20 so
    // consecutive nanoseconds don't bias the result.
    let mut z = nanos.wrapping_mul(0x9E3779B97F4A7C15);
    z ^= z >> 30;
    z = z.wrapping_mul(0xBF58476D1CE4E5B9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94D049BB133111EB);
    z ^= z >> 31;
    ((z % 20) + 1) as i32
}

/// Outcome of attempting to spend points on a skill rank — used both by the
/// command handler and by in-module tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AllocationOutcome {
    Applied { new_rank: u8, remaining_points: u32 },
    AtMaxRank,
    InsufficientPoints,
}

/// Pure allocation step: validates that `ranks_to_buy` more ranks fit under
/// the class/level cap and within available points, then mutates the sheet.
/// Buys ranks one at a time so partial successes are well-defined.
pub fn allocate_skill_ranks(
    sheet: &mut SkillSheet,
    class: Class,
    level: u32,
    skill: Skill,
    ranks_to_buy: u8,
) -> AllocationOutcome {
    if ranks_to_buy == 0 {
        return AllocationOutcome::Applied {
            new_rank: sheet.rank(skill),
            remaining_points: sheet.available_points,
        };
    }
    let cap = max_rank(class, skill, level);
    let cost_per_rank = rank_cost(class, skill);

    let mut bought = 0u8;
    for _ in 0..ranks_to_buy {
        if sheet.rank(skill) >= cap {
            break;
        }
        if sheet.available_points < cost_per_rank {
            break;
        }
        sheet.available_points -= cost_per_rank;
        sheet.set_rank(skill, sheet.rank(skill) + 1);
        bought += 1;
    }

    if bought == 0 {
        if sheet.rank(skill) >= cap {
            AllocationOutcome::AtMaxRank
        } else {
            AllocationOutcome::InsufficientPoints
        }
    } else {
        AllocationOutcome::Applied {
            new_rank: sheet.rank(skill),
            remaining_points: sheet.available_points,
        }
    }
}

/// Server system: drains `GameCommand::AllocateSkillPoint` from
/// `PendingGameCommands` in the `CommandIntercept` set (before the main
/// `process_game_commands`). Emits `SkillRanksChanged` on success, or a chat
/// line on rejection so admin REPL/scripted callers see feedback.
pub fn process_allocate_skill_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut player_query: Query<
        (
            &PlayerIdentity,
            &mut SkillSheet,
            &Class,
            &Experience,
            &mut ChatLog,
        ),
        With<Player>,
    >,
    mut events: ResMut<PendingGameEvents>,
) {
    let queued = std::mem::take(&mut pending_commands.commands);
    let mut remaining = Vec::with_capacity(queued.len());

    for cmd in queued {
        match cmd.command {
            GameCommand::AllocateSkillPoint { skill, ranks } => {
                let mut applied_for_local = false;
                for (identity, mut sheet, class, experience, mut chat_log) in
                    player_query.iter_mut()
                {
                    let matches = match cmd.player_id {
                        Some(id) => identity.id == id,
                        None => true,
                    };
                    if !matches {
                        continue;
                    }
                    let outcome = allocate_skill_ranks(
                        &mut sheet,
                        *class,
                        experience.level,
                        skill,
                        ranks.max(1),
                    );
                    match outcome {
                        AllocationOutcome::Applied {
                            new_rank,
                            remaining_points,
                        } => {
                            events.events.push(GameEvent::SkillRanksChanged {
                                skill,
                                new_rank,
                                remaining_points,
                            });
                            applied_for_local = true;
                        }
                        AllocationOutcome::AtMaxRank => {
                            chat_log.push_narrator(format!(
                                "{} is already at the maximum rank for your level.",
                                skill.label()
                            ));
                        }
                        AllocationOutcome::InsufficientPoints => {
                            chat_log.push_narrator(format!(
                                "Not enough skill points to raise {}.",
                                skill.label()
                            ));
                        }
                    }
                    break;
                }
                if !applied_for_local {
                    bevy::log::debug!(
                        "AllocateSkillPoint command for player {:?} produced no event",
                        cmd.player_id
                    );
                }
            }
            other => remaining.push(crate::game::resources::QueuedGameCommand {
                player_id: cmd.player_id,
                command: other,
            }),
        }
    }

    pending_commands.commands = remaining;
}

/// Hook into the level-up loop: award `skill_points_for_level_up` points,
/// surface a HUD toast, and emit `SkillPointsGranted` for replication. Called
/// from `apply_xp_grants` for each level crossed.
pub fn grant_level_up_skill_points(
    sheet: &mut SkillSheet,
    class: Class,
    base_stats: &BaseStats,
    identity: &PlayerIdentity,
    events: &mut PendingGameEvents,
    ui_events: &mut PendingGameUiEvents,
) {
    let amount = skill_points_for_level_up(class, &base_stats.attributes);
    sheet.available_points = sheet.available_points.saturating_add(amount);
    events.events.push(GameEvent::SkillPointsGranted { amount });
    ui_events.push(identity.id, GameUiEvent::SkillPointsToast { amount });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::AttributeSet;

    #[allow(dead_code)]
    fn default_attrs() -> AttributeSet {
        AttributeSet::new(10, 10, 10, 10, 10, 10)
    }

    #[test]
    fn class_skill_recap_matches_progression_doc() {
        assert!(is_class_skill(Class::Fighter, Skill::Athletics));
        assert!(!is_class_skill(Class::Fighter, Skill::Thievery));
        assert!(is_class_skill(Class::Vagabond, Skill::Thievery));
        assert!(is_class_skill(Class::Vagabond, Skill::Persuasion));
        assert!(is_class_skill(Class::Cleric, Skill::Persuasion));
        assert!(!is_class_skill(Class::Wizard, Skill::Persuasion));
        assert!(is_class_skill(Class::Wizard, Skill::Spellcraft));
    }

    #[test]
    fn rank_cost_and_max_rank() {
        assert_eq!(rank_cost(Class::Vagabond, Skill::Thievery), 1);
        assert_eq!(rank_cost(Class::Fighter, Skill::Thievery), 2);

        assert_eq!(max_rank(Class::Vagabond, Skill::Thievery, 1), 4);
        assert_eq!(max_rank(Class::Fighter, Skill::Thievery, 1), 2);
        assert_eq!(max_rank(Class::Vagabond, Skill::Thievery, 10), 13);
        assert_eq!(max_rank(Class::Fighter, Skill::Thievery, 10), 6);
    }

    #[test]
    fn skill_points_floor_at_one() {
        // Focus 1 → mod = -5, base Fighter = 2, total = -3, floor = 1.
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 1);
        assert_eq!(skill_points_for_level_up(Class::Fighter, &attrs), 1);
        // Focus 18 → mod = 4, base = 2, total = 6.
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 18);
        assert_eq!(skill_points_for_level_up(Class::Fighter, &attrs), 6);
        // Vagabond base = 8.
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 12);
        assert_eq!(skill_points_for_level_up(Class::Vagabond, &attrs), 9);
    }

    #[test]
    fn resolve_skill_check_math() {
        let mut sheet = SkillSheet::default();
        sheet.set_rank(Skill::Thievery, 5);
        let attrs = AttributeSet::new(10, 14, 10, 10, 10, 10);

        // Thievery keys off agility = 14 → mod +2. Ranks +5. Roll 8 + 2 + 5 = 15 ≥ DC 15.
        let res = resolve_skill_check(&sheet, &attrs, Skill::Thievery, 15, 0, 8);
        assert!(res.success);
        assert_eq!(res.total, 15);

        // Same setup, DC 16 — total still 15, fails.
        let res = resolve_skill_check(&sheet, &attrs, Skill::Thievery, 16, 0, 8);
        assert!(!res.success);

        // Situational bonus +2 — total 17.
        let res = resolve_skill_check(&sheet, &attrs, Skill::Thievery, 16, 2, 8);
        assert!(res.success);
        assert_eq!(res.total, 17);
    }

    #[test]
    fn allocate_skill_ranks_decrements_points() {
        let mut sheet = SkillSheet {
            ranks: [0; 10],
            available_points: 4,
        };
        // Vagabond + Thievery is a class skill → cost 1.
        let outcome = allocate_skill_ranks(&mut sheet, Class::Vagabond, 1, Skill::Thievery, 3);
        assert_eq!(
            outcome,
            AllocationOutcome::Applied {
                new_rank: 3,
                remaining_points: 1,
            }
        );
        assert_eq!(sheet.rank(Skill::Thievery), 3);
        assert_eq!(sheet.available_points, 1);
    }

    #[test]
    fn allocate_refuses_at_max_rank() {
        let mut sheet = SkillSheet {
            ranks: [0; 10],
            available_points: 100,
        };
        // Fighter + Thievery is cross-class → cap at level 1 = (1+3)/2 = 2.
        let _ = allocate_skill_ranks(&mut sheet, Class::Fighter, 1, Skill::Thievery, 2);
        assert_eq!(sheet.rank(Skill::Thievery), 2);
        let outcome = allocate_skill_ranks(&mut sheet, Class::Fighter, 1, Skill::Thievery, 1);
        assert_eq!(outcome, AllocationOutcome::AtMaxRank);
    }

    #[test]
    fn allocate_refuses_insufficient_points() {
        let mut sheet = SkillSheet {
            ranks: [0; 10],
            available_points: 1,
        };
        // Cross-class costs 2/rank; 1 point is not enough for even one.
        let outcome = allocate_skill_ranks(&mut sheet, Class::Fighter, 5, Skill::Thievery, 1);
        assert_eq!(outcome, AllocationOutcome::InsufficientPoints);
        assert_eq!(sheet.rank(Skill::Thievery), 0);
        assert_eq!(sheet.available_points, 1);
    }

    #[test]
    fn allocate_partial_buy_when_points_run_out() {
        let mut sheet = SkillSheet {
            ranks: [0; 10],
            available_points: 3,
        };
        // Cross-class cost 2 → can buy 1, not 2.
        let outcome = allocate_skill_ranks(&mut sheet, Class::Fighter, 10, Skill::Thievery, 2);
        assert_eq!(
            outcome,
            AllocationOutcome::Applied {
                new_rank: 1,
                remaining_points: 1,
            }
        );
    }
}
