//! Player XP and level progression.
//!
//! See `docs/progression.md` §4 (XP curve, level-up effects) and §10 (tunables)
//! for the design. This module provides the `Experience` component, the XP
//! curve helpers, and the server system that applies queued XP grants and
//! emits level-up events.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::resources::{GameUiEvent, PendingGameEvents, PendingGameUiEvents};
use crate::player::classes::Class;
use crate::player::components::{BaseStats, Player, PlayerId, PlayerIdentity};
use crate::player::skills::{grant_level_up_skill_points, SkillSheet};

/// Maximum character level. `[tunable]` progression.md §10.
pub const LEVEL_CAP: u32 = 20;

/// Coefficient on the cumulative XP curve. `[tunable]` progression.md §10.
pub const XP_COEFFICIENT: u64 = 1000;

/// Cumulative XP needed to be exactly level `n`. `xp_for_level(1) = 0`.
pub fn xp_for_level(n: u32) -> u64 {
    let n = n as u64;
    XP_COEFFICIENT * n * n.saturating_sub(1) / 2
}

/// Inverts `xp_for_level`. Always returns ≥ 1, ≤ `LEVEL_CAP`.
pub fn level_for_xp(xp: u64) -> u32 {
    let mut n = 1;
    while n < LEVEL_CAP && xp >= xp_for_level(n + 1) {
        n += 1;
    }
    n
}

/// XP awarded for killing a creature of `victim_level`: linear `75·level`.
/// With the `1000·N(N−1)/2` curve, level N→N+1 costs exactly `1000·N`, so
/// same-level kills-to-level is a constant `1000N / 75N ≈ 13.3` all the way
/// up. (The old `level²·50` outran the curve — 20 kills at L1, 1–2 by L15.)
/// `[tunable]` progression.md §4.2.
pub fn xp_grant_for_kill(victim_level: u32) -> u64 {
    victim_level as u64 * 75
}

/// Per-character XP / level state. Lives on both player entities (current_xp
/// drives leveling) and NPC entities (level only, current_xp = 0). The same
/// component is reused so combat code can read victim level uniformly.
#[derive(Component, Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Experience {
    pub current_xp: u64,
    pub level: u32,
}

impl Default for Experience {
    fn default() -> Self {
        Self {
            current_xp: 0,
            level: 1,
        }
    }
}

impl Experience {
    pub const fn at_level(level: u32) -> Self {
        Self {
            current_xp: 0,
            level,
        }
    }

    /// XP into the current level (i.e. progress toward next level).
    pub fn xp_into_level(&self) -> u64 {
        self.current_xp.saturating_sub(xp_for_level(self.level))
    }

    /// XP required for the next level, or `None` if at level cap.
    pub fn xp_for_next(&self) -> Option<u64> {
        if self.level >= LEVEL_CAP {
            None
        } else {
            Some(xp_for_level(self.level + 1) - xp_for_level(self.level))
        }
    }
}

/// Snapshot replicated to the client for the HUD XP bar.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ExperienceView {
    pub current_xp: u64,
    pub level: u32,
    pub xp_into_level: u64,
    pub xp_for_next: Option<u64>,
}

impl From<&Experience> for ExperienceView {
    fn from(e: &Experience) -> Self {
        Self {
            current_xp: e.current_xp,
            level: e.level,
            xp_into_level: e.xp_into_level(),
            xp_for_next: e.xp_for_next(),
        }
    }
}

/// Queued XP grant for a player, produced by combat on a kill, drained by
/// `apply_xp_grants` after combat resolution. Decoupled from the combat loop
/// so we don't borrow the `Experience` query inside the `ParamSet`.
#[derive(Clone, Copy, Debug)]
pub struct PendingXpGrant {
    pub player_id: PlayerId,
    pub amount: u64,
}

#[derive(Resource, Default)]
pub struct PendingXpGrants {
    pub grants: Vec<PendingXpGrant>,
}

/// Apply queued XP grants. Mutates `Experience`, emits `ExperienceGained` /
/// `LevelUp` GameEvents, and a `LevelUpToast` GameUiEvent for each level
/// crossed.
pub fn apply_xp_grants(
    mut grants: ResMut<PendingXpGrants>,
    mut player_query: Query<
        (
            &PlayerIdentity,
            &mut Experience,
            &mut SkillSheet,
            &Class,
            &BaseStats,
        ),
        With<Player>,
    >,
    mut events: ResMut<PendingGameEvents>,
    mut ui_events: ResMut<PendingGameUiEvents>,
) {
    if grants.grants.is_empty() {
        return;
    }

    let drained = std::mem::take(&mut grants.grants);
    for grant in drained {
        let Some((identity, mut experience, mut skill_sheet, class, base_stats)) = player_query
            .iter_mut()
            .find(|(identity, _, _, _, _)| identity.id == grant.player_id)
        else {
            continue;
        };

        experience.current_xp = experience.current_xp.saturating_add(grant.amount);
        events
            .events
            .push(crate::game::resources::GameEvent::ExperienceGained {
                amount: grant.amount,
            });

        while experience.level < LEVEL_CAP
            && experience.current_xp >= xp_for_level(experience.level + 1)
        {
            experience.level += 1;
            events
                .events
                .push(crate::game::resources::GameEvent::LevelUp {
                    new_level: experience.level,
                });
            ui_events.push(
                identity.id,
                GameUiEvent::LevelUpToast {
                    new_level: experience.level,
                },
            );
            grant_level_up_skill_points(
                &mut skill_sheet,
                *class,
                base_stats,
                identity,
                &mut events,
                &mut ui_events,
            );
            // Ability bump every 4 levels (4/8/12/16/20) — progression.md §4.3
            // step 5. Banks a +1-attribute pick the player spends via the UI.
            if experience.level % 4 == 0 {
                crate::player::skills::grant_level_up_ability_bump(
                    &mut skill_sheet,
                    identity,
                    &mut ui_events,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xp_curve_anchors() {
        assert_eq!(xp_for_level(1), 0);
        assert_eq!(xp_for_level(2), 1_000);
        assert_eq!(xp_for_level(3), 3_000);
        assert_eq!(xp_for_level(4), 6_000);
        assert_eq!(xp_for_level(5), 10_000);
        assert_eq!(xp_for_level(10), 45_000);
        assert_eq!(xp_for_level(20), 190_000);
    }

    #[test]
    fn level_for_xp_round_trips() {
        for n in 1..=LEVEL_CAP {
            assert_eq!(level_for_xp(xp_for_level(n)), n);
        }
        assert_eq!(level_for_xp(0), 1);
        assert_eq!(level_for_xp(999), 1);
        assert_eq!(level_for_xp(1_000), 2);
        assert_eq!(level_for_xp(190_000), 20);
        assert_eq!(level_for_xp(u64::MAX), LEVEL_CAP);
    }

    #[test]
    fn xp_grant_anchors() {
        assert_eq!(xp_grant_for_kill(1), 75);
        assert_eq!(xp_grant_for_kill(2), 150);
        assert_eq!(xp_grant_for_kill(3), 225);
        assert_eq!(xp_grant_for_kill(8), 600);
        assert_eq!(xp_grant_for_kill(20), 1_500);
    }

    #[test]
    fn same_level_kills_per_level_stays_in_band() {
        // Level N→N+1 costs 1000·N; a same-level kill grants 75·N. The ratio
        // must stay ~13.3 (kills/level) across the whole 1..20 band — the old
        // quadratic grant collapsed to 1-2 kills by L15.
        for n in 1..LEVEL_CAP {
            let cost = xp_for_level(n + 1) - xp_for_level(n);
            let kills = cost as f64 / xp_grant_for_kill(n) as f64;
            assert!(
                (8.0..=15.0).contains(&kills),
                "level {n}: {kills:.1} kills/level out of band"
            );
        }
    }

    #[test]
    fn experience_progress_helpers() {
        let e = Experience {
            current_xp: 1_500,
            level: 2,
        };
        assert_eq!(e.xp_into_level(), 500);
        assert_eq!(e.xp_for_next(), Some(2_000));

        let cap = Experience {
            current_xp: 1_000_000,
            level: LEVEL_CAP,
        };
        assert_eq!(cap.xp_for_next(), None);
    }
}
