//! Stealth detection math — the opposed roll that decides whether an NPC
//! notices a *sneaking* player. Kept pure (roll values injected) so it unit-tests
//! without a Bevy world, mirroring `crate::player::skills::resolve_skill_check`.
//!
//! A non-sneaking player in sensing range with line-of-sight is always detected
//! (handled by the caller in `nearest_visible_player`). This function only
//! resolves the contest for a sneaking player: their Stealth + a sneak bonus vs
//! the NPC's perception + a light bonus (it's easier to spot someone in the
//! light). See `docs/utility_systems.md` §3.

/// Flat Stealth bonus a player gets for actively sneaking. `[tunable]` — §7.
pub const SNEAK_STEALTH_BONUS: i32 = 5;

/// Maximum perception bonus an NPC gets from full daylight (light level 1.0).
/// Scales linearly down to 0 in pitch darkness. `[tunable]` — §7.
pub const LIGHT_MAX_BONUS: i32 = 6;

/// Convert a `0.0..1.0` light level into the NPC's perception bonus.
pub fn light_to_perception_bonus(light_level: f32) -> i32 {
    (light_level.clamp(0.0, 1.0) * LIGHT_MAX_BONUS as f32).round() as i32
}

/// Resolve the opposed detection roll for a sneaking player. `npc_roll` and
/// `player_roll` are pre-rolled d20s (1..=20). Returns `true` if the NPC spots
/// the player this tick. NPC wins ties — a dead-even contest favors the watcher.
pub fn detection_outcome(
    npc_perception: i32,
    player_stealth: i32,
    light_level: f32,
    npc_roll: i32,
    player_roll: i32,
) -> bool {
    let npc_total = npc_roll + npc_perception + light_to_perception_bonus(light_level);
    let player_total = player_roll + player_stealth + SNEAK_STEALTH_BONUS;
    npc_total >= player_total
}

/// Is this NPC *aware* of `attacker` — i.e. actively fighting, chasing, or
/// fleeing from them? Backstab (`docs/progression.md` §3.4) keys off the
/// inverse: an attack from a sneaking player this NPC is NOT aware of. Pure so
/// combat can call it without reaching into npc system internals.
///
/// `Wander` and `Alert` count as unaware — an alerted NPC is *searching* (it
/// heard something, it hasn't acquired the attacker), matching the
/// `NpcAwareness::Searching` presentation mapping in `game/projection.rs`.
pub fn npc_aware_of(
    state: Option<&crate::npc::components::AiState>,
    combat_target: Option<bevy::prelude::Entity>,
    attacker: bevy::prelude::Entity,
) -> bool {
    use crate::npc::components::AiState;
    if combat_target == Some(attacker) {
        return true;
    }
    match state {
        Some(AiState::Pursue { target }) | Some(AiState::Engage { target }) => *target == attacker,
        // A fleeing NPC lost its CombatTarget (`tick_flee`) but absolutely
        // knows who it's running from.
        Some(AiState::Flee { from, .. }) => *from == attacker,
        Some(AiState::Wander) | Some(AiState::Alert { .. }) | None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npc_aware_of_truth_table() {
        use crate::npc::components::AiState;
        use bevy::prelude::Entity;
        let me = Entity::from_raw_u32(1).unwrap();
        let other = Entity::from_raw_u32(2).unwrap();

        // CombatTarget on me -> aware, regardless of AI state.
        assert!(npc_aware_of(Some(&AiState::Wander), Some(me), me));
        // Pursue/Engage on me -> aware; on someone else -> not of me.
        assert!(npc_aware_of(
            Some(&AiState::Pursue { target: me }),
            None,
            me
        ));
        assert!(npc_aware_of(
            Some(&AiState::Engage { target: me }),
            None,
            me
        ));
        assert!(!npc_aware_of(
            Some(&AiState::Engage { target: other }),
            None,
            me
        ));
        // Fleeing from me -> aware (it knows exactly who hurt it).
        assert!(npc_aware_of(
            Some(&AiState::Flee {
                from: me,
                expires_at_seconds: 0.0,
                reason: crate::npc::components::FleeReason::UnreachableAttacker,
            }),
            None,
            me
        ));
        // Wander / Alert (searching, hasn't acquired) / no state -> unaware.
        assert!(!npc_aware_of(Some(&AiState::Wander), None, me));
        assert!(!npc_aware_of(
            Some(&AiState::Alert {
                last_seen: crate::world::components::TilePosition { x: 0, y: 0, z: 0 },
                expires_at_seconds: 0.0
            }),
            None,
            me
        ));
        assert!(!npc_aware_of(None, None, me));
    }

    #[test]
    fn darkness_and_sneaking_beats_a_dull_guard() {
        // Pitch dark (light 0 → no NPC bonus), average rolls, no ranks.
        // player_total = 10 + 0 + 5 = 15; npc_total = 10 + 0 + 0 = 10.
        assert!(!detection_outcome(0, 0, 0.0, 10, 10));
    }

    #[test]
    fn bright_light_defeats_stealth() {
        // Full daylight gives the NPC +6. Same rolls/ranks as above:
        // npc_total = 10 + 0 + 6 = 16 >= player_total 15.
        assert!(detection_outcome(0, 0, 1.0, 10, 10));
    }

    #[test]
    fn sharp_eyed_guard_perception_matters() {
        // In the dark, a high-perception guard still spots a low-skill sneaker.
        // npc_total = 10 + 8 + 0 = 18 >= player_total = 10 + 0 + 5 = 15.
        assert!(detection_outcome(8, 0, 0.0, 10, 10));
    }

    #[test]
    fn high_stealth_evades_in_the_dark() {
        // A skilled sneaker (ranks+agi = 8) in darkness beats an average guard.
        // player_total = 10 + 8 + 5 = 23; npc_total = 10 + 0 + 0 = 10.
        assert!(!detection_outcome(0, 8, 0.0, 10, 10));
    }

    #[test]
    fn light_bonus_is_linear_and_clamped() {
        assert_eq!(light_to_perception_bonus(0.0), 0);
        assert_eq!(light_to_perception_bonus(1.0), LIGHT_MAX_BONUS);
        assert_eq!(
            light_to_perception_bonus(0.5),
            (LIGHT_MAX_BONUS as f32 * 0.5).round() as i32
        );
        // Out-of-range inputs clamp rather than overshoot.
        assert_eq!(light_to_perception_bonus(2.0), LIGHT_MAX_BONUS);
        assert_eq!(light_to_perception_bonus(-1.0), 0);
    }
}
