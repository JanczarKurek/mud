//! Player XP and level progression.
//!
//! See `docs/progression.md` §4 (XP curve, level-up effects) and §10 (tunables)
//! for the design. This module provides the `Experience` component, the XP
//! curve helpers, and the server system that applies queued XP grants and
//! emits level-up events.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::resources::{GameUiEvent, PendingGameEvents, PendingGameUiEvents};
use crate::player::components::{Player, PlayerId, PlayerIdentity};

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

/// XP awarded for killing a creature of `victim_level`.
/// `[tunable]` progression.md §4.2.
pub fn xp_grant_for_kill(victim_level: u32) -> u64 {
    (victim_level as u64).pow(2) * 50
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
    mut player_query: Query<(&PlayerIdentity, &mut Experience), With<Player>>,
    mut events: ResMut<PendingGameEvents>,
    mut ui_events: ResMut<PendingGameUiEvents>,
) {
    if grants.grants.is_empty() {
        return;
    }

    let drained = std::mem::take(&mut grants.grants);
    for grant in drained {
        let Some((identity, mut experience)) = player_query
            .iter_mut()
            .find(|(identity, _)| identity.id == grant.player_id)
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
        assert_eq!(xp_grant_for_kill(1), 50);
        assert_eq!(xp_grant_for_kill(2), 200);
        assert_eq!(xp_grant_for_kill(3), 450);
        assert_eq!(xp_grant_for_kill(8), 3_200);
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
