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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(light_to_perception_bonus(0.5), (LIGHT_MAX_BONUS as f32 * 0.5).round() as i32);
        // Out-of-range inputs clamp rather than overshoot.
        assert_eq!(light_to_perception_bonus(2.0), LIGHT_MAX_BONUS);
        assert_eq!(light_to_perception_bonus(-1.0), 0);
    }
}
