//! Hidden trait for world objects.
//!
//! An object with `Hidden` is filtered out of every peer's projection unless
//! that peer's `PlayerId` is in `detected_by`. Detection is per-player; one
//! player spotting the trap never affects what another sees. Reveals happen
//! two ways:
//!
//! * **Passive perception** — `passive_perception_tick` rolls a Perception
//!   skill check the first frame a player enters a hidden object's detection
//!   range (`inspect_range - 1` tiles), then once every `PERCEPTION_COOLDOWN`
//!   seconds for that (player, object) pair.
//! * **Auto-reveal on step** — `process_step_triggers` (see `step_triggers.rs`)
//!   reveals the triggered object to a player stepper.
//!
//! `detected_by` is persisted in the world snapshot so spotted traps stay
//! visible to the player across restarts. `next_check_at` is in-memory only —
//! pacing state, fine to reset.
//!
//! Server-authoritative. Clients learn of newly-detected objects via the
//! existing `WorldObjectUpserted` projection event.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::player::components::{BaseStats, ChatLog, Player, PlayerIdentity};
use crate::player::skills::{skill_check, Skill, SkillSheet};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Seconds between perception rolls for a given (player, hidden_object) pair
/// after the first check has fired. The first check fires immediately on
/// entering range; subsequent checks wait for this cooldown.
pub const PERCEPTION_COOLDOWN: f64 = 5.0;

/// Fallback Chebyshev range when the object's definition omits
/// `inspect_range`. Mirrors `DEFAULT_INSPECT_RANGE` in `game/systems.rs`.
pub const DEFAULT_INSPECT_RANGE: i32 = 3;

/// Server-only marker on a world-object entity. While present, every peer's
/// projection skips this object unless their `PlayerId` is in `detected_by`.
#[derive(Component, Clone, Debug, Default)]
pub struct Hidden {
    pub dc: u32,
    pub detected_by: HashSet<crate::player::components::PlayerId>,
    /// Absolute `Time::elapsed_secs_f64()` at which the (player, this object)
    /// pair becomes eligible for the next Perception roll. Missing entry =
    /// eligible immediately ("fires the moment a player gets close").
    pub next_check_at: HashMap<crate::player::components::PlayerId, f64>,
}

impl Hidden {
    pub fn new(dc: u32) -> Self {
        Self {
            dc,
            detected_by: HashSet::new(),
            next_check_at: HashMap::new(),
        }
    }

    pub fn is_detected_by(&self, player_id: crate::player::components::PlayerId) -> bool {
        self.detected_by.contains(&player_id)
    }

    /// Inserts `player_id` into `detected_by`. Returns `true` iff this call
    /// changed the set — callers fire the narrator line only on `true`.
    pub fn reveal_to(&mut self, player_id: crate::player::components::PlayerId) -> bool {
        self.detected_by.insert(player_id)
    }

    /// True when `now` ≥ the player's scheduled next check (or no check has
    /// ever fired for this player). Used by `passive_perception_tick`.
    pub fn is_eligible_for_check(
        &self,
        player_id: crate::player::components::PlayerId,
        now: f64,
    ) -> bool {
        match self.next_check_at.get(&player_id) {
            Some(at) => now >= *at,
            None => true,
        }
    }

    pub fn schedule_next_check(
        &mut self,
        player_id: crate::player::components::PlayerId,
        now: f64,
        cooldown: f64,
    ) {
        self.next_check_at.insert(player_id, now + cooldown);
    }
}

/// Per-frame perception rolling. For each (player, hidden-object) pair in the
/// same space where the player is within `inspect_range - 1` Chebyshev tiles
/// and is not already in `detected_by`, the first eligible tick rolls a
/// Perception check against the object's DC. On success, the player is added
/// to `detected_by` and a narrator line is pushed to their chat log; the
/// projection naturally emits `WorldObjectUpserted` on the next tick.
///
/// Every roll (pass or fail) sets the (player, object) cooldown to `now +
/// PERCEPTION_COOLDOWN` so the player isn't re-rolling every frame.
pub fn passive_perception_tick(
    time: Res<Time>,
    mut player_query: Query<
        (
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &SkillSheet,
            &BaseStats,
            &mut ChatLog,
        ),
        With<Player>,
    >,
    mut hidden_query: Query<
        (&SpaceResident, &TilePosition, &OverworldObject, &mut Hidden),
        Without<Player>,
    >,
    definitions: Res<OverworldObjectDefinitions>,
) {
    let now = time.elapsed_secs_f64();

    for (identity, p_resident, p_tile, sheet, base, mut chat_log) in player_query.iter_mut() {
        for (h_resident, h_tile, object, mut hidden) in hidden_query.iter_mut() {
            if h_resident.space_id != p_resident.space_id {
                continue;
            }
            if hidden.is_detected_by(identity.id) {
                continue;
            }
            let definition = definitions.get(&object.definition_id);
            let base_inspect = definition
                .and_then(|def| def.inspect_range)
                .unwrap_or(DEFAULT_INSPECT_RANGE);
            let range = (base_inspect - 1).max(1);
            if chebyshev_distance(*p_tile, *h_tile) > range {
                continue;
            }
            if !hidden.is_eligible_for_check(identity.id, now) {
                continue;
            }
            let result = skill_check(
                sheet,
                &base.attributes,
                Skill::Perception,
                hidden.dc as i32,
                0,
            );
            hidden.schedule_next_check(identity.id, now, PERCEPTION_COOLDOWN);
            if result.success && hidden.reveal_to(identity.id) {
                let name = definition
                    .map(|def| def.name.to_lowercase())
                    .unwrap_or_else(|| object.definition_id.clone());
                chat_log.push_narrator(format!("You spot a {name}!"));
            }
        }
    }
}

fn chebyshev_distance(a: TilePosition, b: TilePosition) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::PlayerId;

    #[test]
    fn reveal_to_returns_true_only_first_time() {
        let mut hidden = Hidden::new(15);
        let p1 = PlayerId(1);
        assert!(hidden.reveal_to(p1));
        assert!(!hidden.reveal_to(p1));
        assert!(hidden.is_detected_by(p1));
    }

    #[test]
    fn per_player_detection_is_isolated() {
        let mut hidden = Hidden::new(15);
        let p1 = PlayerId(1);
        let p2 = PlayerId(2);
        hidden.reveal_to(p1);
        assert!(hidden.is_detected_by(p1));
        assert!(!hidden.is_detected_by(p2));
    }

    #[test]
    fn check_eligibility_respects_cooldown() {
        let mut hidden = Hidden::new(15);
        let p1 = PlayerId(1);
        // No prior check: eligible immediately at any positive time.
        assert!(hidden.is_eligible_for_check(p1, 0.0));
        assert!(hidden.is_eligible_for_check(p1, 100.0));
        hidden.schedule_next_check(p1, 10.0, PERCEPTION_COOLDOWN);
        // Cooldown active until 15.0.
        assert!(!hidden.is_eligible_for_check(p1, 11.0));
        assert!(!hidden.is_eligible_for_check(p1, 14.9));
        assert!(hidden.is_eligible_for_check(p1, 15.0));
        assert!(hidden.is_eligible_for_check(p1, 20.0));
    }
}
