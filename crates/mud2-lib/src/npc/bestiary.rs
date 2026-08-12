//! The Bestiary: what watching a creature teaches you about it.
//!
//! Where the People dossier is an *active* verb (right-click → Details, one
//! Persuasion roll), bestiary knowledge accrues passively. While a creature is
//! in sight, [`tick_bestiary_observation`] periodically rolls Perception
//! against a DC derived from its level, advancing the player's entry one rung
//! at a time. The pattern — a per-(player, subject) reroll schedule that
//! reschedules on failure as well as success — is lifted from
//! `world::hidden::passive_perception_tick` and `player::sense::tick_player_sense`.
//!
//! | Tier | Name | Reveals |
//! |---|---|---|
//! | 1 | Sighted | what it is, and its description |
//! | 2 | Studied | level, a coarse vitality band, armour and block |
//! | 3 | Analyzed | its attack, what it hunts, what scares it, its senses |
//! | 4 | Mastered | loot and lore — **also needs [`BESTIARY_MASTERY_KILLS`] kills** |
//!
//! The kill gate on the last rung is the one place the ladder isn't purely
//! observational: you can watch a wolf all day, but you don't know what a wolf
//! is worth until you've skinned a few.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::codex::{CodexState, CodexUpdate, PendingCodexUpdates};
use crate::crafting::CharacterStash;
use crate::npc::components::Npc;
use crate::player::components::{Aware, BaseStats, Player, PlayerIdentity, AWARE_PERCEPTION_BONUS};
use crate::player::skills::{resolve_skill_check, roll_d20, Skill, SkillSheet};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::hidden::Hidden;
use crate::world::object_definitions::{CodexClass, OverworldObjectDefinitions};

/// Seconds between observation rolls for one (player, creature type) pair.
/// Per *type*, not per creature: a pack of six wolves is still one wolf entry,
/// and shouldn't advance six times faster. `[tunable]`
pub const BESTIARY_CHECK_INTERVAL: f64 = 3.0;

/// Chebyshev tiles within which a creature can be studied. `[tunable]`
pub const BESTIARY_RANGE: i32 = 12;

/// Base Perception DC before level and tier scaling. `[tunable]`
pub const BESTIARY_BASE_DC: i32 = 8;

/// Extra DC per rung climbed. Deliberately shallow: the top rung is gated by
/// [`BESTIARY_MASTERY_KILLS`] as well, so making it also need a natural 20
/// would put mastery out of reach of anyone but a Perception specialist.
/// `[tunable]`
pub const BESTIARY_DC_PER_TIER: i32 = 2;

/// Kills of a type required before its final tier can be reached. `[tunable]`
pub const BESTIARY_MASTERY_KILLS: u32 = 10;

/// Highest bestiary tier.
pub const BESTIARY_MAX_TIER: u8 = 4;

/// Per-player reroll schedule, keyed by creature `definition_id`. Session-only
/// state attached lazily on first tick, like `player::sense::SenseReveals`; a
/// reload just means the next roll comes immediately.
#[derive(Component, Default)]
pub struct BestiaryObservation {
    next_check_at: HashMap<String, f64>,
}

impl BestiaryObservation {
    fn eligible(&self, definition_id: &str, now: f64) -> bool {
        self.next_check_at
            .get(definition_id)
            .is_none_or(|next| now >= *next)
    }

    fn schedule_next(&mut self, definition_id: &str, now: f64) {
        self.next_check_at
            .insert(definition_id.to_owned(), now + BESTIARY_CHECK_INTERVAL);
    }
}

/// Perception DC to advance *into* `tier` for a creature of `level`.
/// A level-2 goblin runs 10 / 12 / 14 / 16.
pub fn bestiary_dc(level: u32, tier: u8) -> i32 {
    BESTIARY_BASE_DC + level as i32 + BESTIARY_DC_PER_TIER * (tier as i32 - 1)
}

/// Resolves one observation. Pure, with the d20 injected, so the ladder rules
/// are testable without a World.
///
/// Returns the tier the observer reaches, or `None` when nothing advances.
/// Rules: always target exactly `current_tier + 1` (knowledge never skips a
/// rung), never exceed [`BESTIARY_MAX_TIER`], and refuse the final rung until
/// the observer has [`BESTIARY_MASTERY_KILLS`] kills of the type.
#[allow(clippy::too_many_arguments)]
pub fn resolve_observation(
    current_tier: u8,
    level: u32,
    kills: u32,
    sheet: &SkillSheet,
    attributes: &crate::player::components::AttributeSet,
    situational: i32,
    roll: i32,
) -> Option<u8> {
    if current_tier >= BESTIARY_MAX_TIER {
        return None;
    }
    let target = current_tier + 1;
    // The mastery rung is bought with kills as well as attention.
    if target == BESTIARY_MAX_TIER && kills < BESTIARY_MASTERY_KILLS {
        return None;
    }
    let dc = bestiary_dc(level, target);
    let result = resolve_skill_check(sheet, attributes, Skill::Perception, dc, situational, roll);
    result.success.then_some(target)
}

type BestiaryPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PlayerIdentity,
        &'static SpaceResident,
        &'static TilePosition,
        &'static BaseStats,
        &'static SkillSheet,
        &'static CharacterStash,
        Has<Aware>,
        Option<&'static mut BestiaryObservation>,
    ),
    With<Player>,
>;

/// Rolls Perception for every (player, nearby creature type) pair that is due,
/// queueing any tier-up into [`PendingCodexUpdates`].
///
/// Read-only on `CharacterStash`: the write goes through the codex queue, so
/// this system and `npc::social_read` can both run without fighting over the
/// stash borrow.
pub fn tick_bestiary_observation(
    time: Res<Time>,
    mut commands: Commands,
    mut players: BestiaryPlayerQuery,
    creatures: Query<
        (
            &OverworldObject,
            &SpaceResident,
            &TilePosition,
            Option<&Hidden>,
        ),
        With<Npc>,
    >,
    definitions: Res<OverworldObjectDefinitions>,
    mut updates: ResMut<PendingCodexUpdates>,
) {
    let now = time.elapsed_secs_f64();

    for (
        entity,
        identity,
        player_space,
        player_tile,
        base_stats,
        sheet,
        stash,
        is_aware,
        observation,
    ) in &mut players
    {
        let Some(mut observation) = observation else {
            // First sighting of this player — attach the schedule; it starts
            // observing next frame.
            commands
                .entity(entity)
                .insert(BestiaryObservation::default());
            continue;
        };

        let codex = CodexState::from_stash(stash);
        let situational = if is_aware { AWARE_PERCEPTION_BONUS } else { 0 };

        for (object, creature_space, creature_tile, hidden) in &creatures {
            if creature_space.space_id != player_space.space_id {
                continue;
            }
            if chebyshev(*player_tile, *creature_tile) > BESTIARY_RANGE {
                continue;
            }
            // You can't study what you haven't spotted.
            if hidden.is_some_and(|h| !h.is_detected_by(identity.id)) {
                continue;
            }
            let definition_id = object.definition_id.as_str();
            let Some(def) = definitions.get(definition_id) else {
                continue;
            };
            if def.codex_class() != CodexClass::Bestiary {
                continue;
            }
            if !observation.eligible(definition_id, now) {
                continue;
            }
            // Reschedule whether the roll lands or not, so a hard creature
            // can't be brute-forced by standing still.
            observation.schedule_next(definition_id, now);

            let Some(tier) = resolve_observation(
                codex.mob_tier(definition_id),
                def.level.unwrap_or(1),
                codex.kills_of(definition_id),
                sheet,
                &base_stats.attributes,
                situational,
                roll_d20(),
            ) else {
                continue;
            };
            updates.push(
                identity.id,
                CodexUpdate::MobTier {
                    definition_id: definition_id.to_owned(),
                    tier,
                },
            );
        }
    }
}

fn chebyshev(a: TilePosition, b: TilePosition) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::AttributeSet;

    /// A baseline character: every attribute at the point-buy baseline, so
    /// ability modifiers are 0 and the assertions below are pure DC math.
    fn sheet_and_attrs() -> (SkillSheet, AttributeSet) {
        let baseline = crate::player::components::ATTR_BASELINE;
        (
            SkillSheet::default(),
            AttributeSet {
                strength: baseline,
                agility: baseline,
                constitution: baseline,
                willpower: baseline,
                charisma: baseline,
                focus: baseline,
            },
        )
    }

    #[test]
    fn dc_ladder_scales_with_level_and_tier() {
        assert_eq!(
            (1..=4).map(|t| bestiary_dc(2, t)).collect::<Vec<_>>(),
            vec![10, 12, 14, 16]
        );
        // Tougher creatures are harder to read at every rung.
        assert!(bestiary_dc(9, 1) > bestiary_dc(2, 1));
    }

    #[test]
    fn tier_never_skips_and_never_regresses() {
        let (sheet, attrs) = sheet_and_attrs();
        // A natural 20 from tier 1 still only reaches tier 2.
        assert_eq!(
            resolve_observation(1, 1, 0, &sheet, &attrs, 0, 20),
            Some(2),
            "one rung per roll"
        );
        // Already at the top: nothing left to learn.
        assert_eq!(
            resolve_observation(BESTIARY_MAX_TIER, 1, 999, &sheet, &attrs, 0, 20),
            None
        );
    }

    #[test]
    fn mastery_requires_the_kill_count() {
        let (sheet, attrs) = sheet_and_attrs();
        assert_eq!(
            resolve_observation(3, 1, BESTIARY_MASTERY_KILLS - 1, &sheet, &attrs, 0, 20),
            None,
            "watching alone never masters a creature"
        );
        assert_eq!(
            resolve_observation(3, 1, BESTIARY_MASTERY_KILLS, &sheet, &attrs, 0, 20),
            Some(BESTIARY_MAX_TIER)
        );
    }

    #[test]
    fn a_failed_roll_reveals_nothing() {
        let (sheet, attrs) = sheet_and_attrs();
        // Natural 1 against a level-9 creature: DC 17, unreachable.
        assert_eq!(bestiary_dc(9, 1), 17);
        assert_eq!(resolve_observation(0, 9, 0, &sheet, &attrs, 0, 1), None);
    }

    #[test]
    fn the_aware_bonus_can_carry_a_marginal_roll() {
        let (sheet, attrs) = sheet_and_attrs();
        // DC 11 for tier 1 against a level-3 wolf; a 10 misses by one.
        assert_eq!(resolve_observation(0, 3, 0, &sheet, &attrs, 0, 10), None);
        assert_eq!(
            resolve_observation(0, 3, 0, &sheet, &attrs, AWARE_PERCEPTION_BONUS, 10),
            Some(1)
        );
    }

    #[test]
    fn schedule_gates_rerolls_per_definition() {
        let mut observation = BestiaryObservation::default();
        assert!(observation.eligible("wolf", 0.0), "first look is immediate");
        observation.schedule_next("wolf", 0.0);
        assert!(!observation.eligible("wolf", BESTIARY_CHECK_INTERVAL - 0.1));
        assert!(observation.eligible("wolf", BESTIARY_CHECK_INTERVAL));
        assert!(
            observation.eligible("goblin", 0.0),
            "schedules are per creature type"
        );
    }

    #[test]
    fn classification_splits_people_from_creatures() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        assert_eq!(
            definitions.get("wolf").unwrap().codex_class(),
            CodexClass::Bestiary
        );
        assert_eq!(
            definitions.get("villager").unwrap().codex_class(),
            CodexClass::People
        );
        // Guards have no dialog node, so they rely on the explicit override.
        assert_eq!(
            definitions.get("town_guard").unwrap().codex_class(),
            CodexClass::People,
            "town_guard needs `codex: people` in its metadata"
        );
    }
}

/// Whole-chain tests: observation tick → `PendingCodexUpdates` →
/// `apply_codex_updates` → `process_log_commands` → a readable Bestiary entry.
/// These are what prove the cross-plugin wiring, which the pure-logic tests
/// above deliberately don't touch.
#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use crate::codex::updates::{apply_codex_updates, drain_codex_kills};
    use crate::codex::PendingCodexKills;
    use crate::log::{LogOwner, LogState, BESTIARY_SECTION};
    use crate::player::components::{AttributeSet, PlayerId};
    use crate::world::components::SpaceId;

    const TEST_SPACE: SpaceId = SpaceId(0);

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::game::resources::PendingGameCommands>()
            .init_resource::<PendingCodexUpdates>()
            .init_resource::<PendingCodexKills>()
            .insert_resource(OverworldObjectDefinitions::load_from_disk());
        // Lift the virtual clock's 250ms max_delta clamp so a multi-second
        // manual step arrives as one delta.
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_max_delta(std::time::Duration::MAX);
        app.add_systems(
            Update,
            (
                tick_bestiary_observation,
                drain_codex_kills,
                apply_codex_updates,
                crate::log::commands::process_log_commands,
            )
                .chain(),
        );
        app
    }

    /// A watcher with maxed-out Perception so the rolls aren't flaky.
    fn spawn_watcher(app: &mut App, id: PlayerId, tile: (i32, i32)) -> Entity {
        let mut sheet = SkillSheet::default();
        sheet.ranks[Skill::Perception as usize] = 20;
        let baseline = crate::player::components::ATTR_BASELINE;
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(id),
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(tile.0, tile.1),
                BaseStats {
                    attributes: AttributeSet {
                        strength: baseline,
                        agility: baseline,
                        constitution: baseline,
                        willpower: baseline,
                        charisma: baseline,
                        focus: baseline,
                    },
                    ..Default::default()
                },
                sheet,
                CharacterStash::default(),
            ))
            .id()
    }

    fn spawn_creature(app: &mut App, object_id: u64, tile: (i32, i32), definition_id: &str) {
        app.world_mut().spawn((
            Npc,
            OverworldObject {
                object_id,
                definition_id: definition_id.to_owned(),
                placement_seq: 0,
            },
            SpaceResident {
                space_id: TEST_SPACE,
            },
            TilePosition::ground(tile.0, tile.1),
        ));
    }

    fn bestiary_entry(
        app: &App,
        watcher: Entity,
        definition_id: &str,
    ) -> Option<crate::log::LogEntry> {
        let stash = app.world().get::<CharacterStash>(watcher).unwrap();
        LogState::from_stash(stash)
            .entry(BESTIARY_SECTION, definition_id)
            .cloned()
    }

    /// Runs one update with the clock pushed past the reroll cooldown, so the
    /// tick is eligible to roll again. Same manual-stepping trick as
    /// `npc::witness`'s tests: `Time<Virtual>::advance_by` alone doesn't move
    /// the delta the next update computes.
    fn tick_past_cooldown(app: &mut App) {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(BESTIARY_CHECK_INTERVAL + 0.5),
        ));
        app.update();
    }

    #[test]
    fn watching_a_wolf_writes_a_bestiary_log_entry() {
        let mut app = test_app();
        let watcher = spawn_watcher(&mut app, PlayerId(1), (5, 5));
        spawn_creature(&mut app, 77, (5, 8), "wolf");

        // First update only attaches `BestiaryObservation`.
        app.update();
        assert!(bestiary_entry(&app, watcher, "wolf").is_none());

        // Second update rolls and should land tier 1.
        app.update();
        let entry = bestiary_entry(&app, watcher, "wolf").expect("a tier-1 wolf entry");
        assert_eq!(entry.title, "Wolf");
        assert_eq!(entry.owner, LogOwner::Engine);
        assert!(entry.body.contains("Beast"), "{}", entry.body);
        // Tier 1 says nothing about how it fights.
        assert!(!entry.body.contains("Level"), "{}", entry.body);

        // Keep watching: the entry climbs.
        tick_past_cooldown(&mut app);
        app.update();
        let entry = bestiary_entry(&app, watcher, "wolf").expect("wolf entry");
        assert!(entry.body.contains("Level 3"), "{}", entry.body);
    }

    #[test]
    fn people_are_not_filed_in_the_bestiary() {
        let mut app = test_app();
        let watcher = spawn_watcher(&mut app, PlayerId(1), (5, 5));
        spawn_creature(&mut app, 78, (5, 6), "town_guard");

        app.update();
        tick_past_cooldown(&mut app);
        app.update();

        assert!(
            bestiary_entry(&app, watcher, "town_guard").is_none(),
            "guards belong to the People codex"
        );
    }

    #[test]
    fn a_creature_out_of_range_teaches_nothing() {
        let mut app = test_app();
        let watcher = spawn_watcher(&mut app, PlayerId(1), (5, 5));
        spawn_creature(&mut app, 77, (5, 5 + BESTIARY_RANGE + 1), "wolf");

        app.update();
        tick_past_cooldown(&mut app);
        app.update();

        assert!(bestiary_entry(&app, watcher, "wolf").is_none());
    }

    /// The mastery rung is bought with kills as well as attention: watching
    /// alone stalls the entry at tier 3.
    #[test]
    fn mastery_waits_for_the_kill_count() {
        let mut app = test_app();
        let watcher = spawn_watcher(&mut app, PlayerId(1), (5, 5));
        spawn_creature(&mut app, 77, (5, 6), "wolf");

        // Plenty of rolls to reach the ceiling of what watching can give.
        for _ in 0..8 {
            app.update();
            tick_past_cooldown(&mut app);
        }
        let watched = bestiary_entry(&app, watcher, "wolf").expect("wolf entry");
        assert!(watched.body.contains("1d6+2"), "tier 3: {}", watched.body);
        assert!(
            !watched.body.contains("Yields:"),
            "loot is mastery-only: {}",
            watched.body
        );

        // Now credit the kills and keep watching.
        for _ in 0..BESTIARY_MASTERY_KILLS {
            app.world_mut()
                .resource_mut::<PendingCodexKills>()
                .kills
                .push((PlayerId(1), "wolf".to_owned()));
            app.update();
            tick_past_cooldown(&mut app);
        }
        let mastered = bestiary_entry(&app, watcher, "wolf").expect("wolf entry");
        assert!(
            mastered.body.contains("Yields:"),
            "mastery should reveal loot: {}",
            mastered.body
        );
        assert!(mastered.body.starts_with(&watched.body), "tiers append");
    }
}
