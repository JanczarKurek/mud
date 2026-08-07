//! The Exertion fatigue meter — the Medium-sim currency from
//! `docs/utility_systems.md` §6.1.
//!
//! Internally the component accumulates *exertion* (0 = rested, `max` =
//! exhausted); the HUD presents the inverse as a depleting **Stamina** bar
//! (`max − current`). Exertion is raised by physical effort (climbing, jumping,
//! sustained sneaking, combat — the action hooks live at their respective
//! sites) and lowered by idle rest and food/drink. High exertion raises the DC
//! of *physical* checks ([`exertion_dc_modifier`], surfaced via
//! [`crate::player::check::Dc`]) and slows HP/mana regen
//! ([`exertion_regen_multiplier`], folded into `tick_vital_regen`).
//!
//! The pool is governed by **attributes**, not a skill: **Constitution** sets
//! the ceiling and **Willpower** sets the recovery rate. (The Endurance *skill*
//! separately multiplies HP/mana regen — see `src/player/regen.rs`.)
//!
//! This module owns the tunable constants, the pure helpers (unit-tested
//! below), and the server-side [`tick_exertion`] decay system. It mutates the
//! authoritative [`Exertion`] component only; the value replicates to the
//! client via the `PlayerExertionChanged` diff in `compute_events_for_peer`.

use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::player::classes::ability_mod;
use crate::player::components::{
    DerivedStats, Exertion, MovementCooldown, Player, VitalStats, EXERTION_BASE_MAX,
};

// ── Tunable knobs (`docs/utility_systems.md` §7) ──────────────────────────────

/// Each point of Constitution modifier widens the stamina ceiling by this much.
pub const EXERTION_MAX_PER_CON_MOD: f32 = 15.0;
/// Floor on the ceiling so even a very frail character keeps a usable pool.
pub const EXERTION_MIN_MAX: f32 = 40.0;
/// Passive recovery while idle (points/sec at Willpower 10 / modifier 0).
pub const EXERTION_IDLE_RECOVERY_PER_SEC: f32 = 6.0;
/// Recovery while active — recently moved or in combat — a much slower trickle.
pub const EXERTION_ACTIVE_RECOVERY_PER_SEC: f32 = 1.5;
/// Each point of Willpower modifier scales the recovery rate by this fraction.
pub const EXERTION_RECOVERY_PER_WILL_MOD: f32 = 0.10;

/// Exertion cost of a successful ledge climb.
pub const EXERTION_COST_CLIMB: f32 = 8.0;
/// Exertion cost of a resolved jump.
pub const EXERTION_COST_JUMP: f32 = 6.0;
/// Exertion cost of a committed attack (hit or miss).
pub const EXERTION_COST_ATTACK: f32 = 4.0;
/// Exertion cost of a successful heavy-object shove (push/pull). Charged once
/// per resolved push regardless of distance, like climb/jump.
pub const EXERTION_COST_PUSH: f32 = 6.0;
/// Sustained-sneaking cost, applied once per `SENSE_INTERVAL` (~1s) while sneaking.
pub const EXERTION_COST_SNEAK_PER_SEC: f32 = 2.0;
/// Fatigue relief from eating/drinking a regen-buff consumable.
pub const EXERTION_FOOD_RELIEF: f32 = 25.0;

/// Fraction of the pool spent at which the fatigue DC penalty begins.
pub const EXERTION_DC_RAMP_START: f32 = 0.5;
/// Each additional fraction spent past the start adds +1 to the DC.
pub const EXERTION_DC_RAMP_STEP: f32 = 0.1;
/// Cap on the fatigue DC penalty (reached at a fully-spent pool).
pub const EXERTION_DC_MAX: i32 = 6;
/// Worst-case regen multiplier (at a full meter) from high exertion.
pub const EXERTION_REGEN_FLOOR: f32 = 0.5;

// ── Pure helpers ──────────────────────────────────────────────────────────────

/// The stamina ceiling for a given Constitution score (§6.1): base widened per
/// Constitution modifier point, floored at [`EXERTION_MIN_MAX`].
pub fn exertion_max(constitution: i32) -> f32 {
    (EXERTION_BASE_MAX + ability_mod(constitution) as f32 * EXERTION_MAX_PER_CON_MOD)
        .max(EXERTION_MIN_MAX)
}

/// Recovery rate (points/sec) for a player given their Willpower score and
/// whether they are idle (standing still, out of combat).
pub fn exertion_recovery_rate(willpower: i32, idle: bool) -> f32 {
    let base = if idle {
        EXERTION_IDLE_RECOVERY_PER_SEC
    } else {
        EXERTION_ACTIVE_RECOVERY_PER_SEC
    };
    (base * (1.0 + ability_mod(willpower) as f32 * EXERTION_RECOVERY_PER_WILL_MOD)).max(0.1)
}

/// Fatigue penalty to the *DC* of a physical check (a positive number makes the
/// check harder). Zero below [`EXERTION_DC_RAMP_START`] (50% spent), then +1 at
/// the start and +1 for every further [`EXERTION_DC_RAMP_STEP`] (10%) spent,
/// capped at [`EXERTION_DC_MAX`]. `None` (a player without the meter — e.g. test
/// fixtures) yields 0.
pub fn exertion_dc_modifier(exertion: Option<&Exertion>) -> i32 {
    let Some(e) = exertion else {
        return 0;
    };
    let ratio = e.ratio();
    if ratio < EXERTION_DC_RAMP_START {
        return 0;
    }
    // Nudge before flooring so f32 rounding (e.g. (0.9−0.5)/0.1 = 3.9999…)
    // doesn't drop a whole step at a clean 10% boundary.
    let steps = ((ratio - EXERTION_DC_RAMP_START) / EXERTION_DC_RAMP_STEP + 1e-4).floor() as i32;
    (1 + steps).min(EXERTION_DC_MAX)
}

/// Multiplier applied to HP/mana regen from exertion: 1.0 below the high
/// threshold, ramping linearly down to [`EXERTION_REGEN_FLOOR`] at a full meter.
pub fn exertion_regen_multiplier(exertion: Option<&Exertion>) -> f32 {
    use crate::player::components::EXERTION_HIGH_THRESHOLD;
    match exertion {
        Some(e) if e.is_high() => {
            let span = (1.0 - EXERTION_HIGH_THRESHOLD).max(f32::EPSILON);
            let t = ((e.ratio() - EXERTION_HIGH_THRESHOLD) / span).clamp(0.0, 1.0);
            1.0 - t * (1.0 - EXERTION_REGEN_FLOOR)
        }
        _ => 1.0,
    }
}

// ── Server system ─────────────────────────────────────────────────────────────

/// Decay exertion each frame. Recovery is fast while idle (standing still and
/// out of combat) and a slow trickle otherwise, scaled by Willpower. Costs are
/// added at the action sites; this system only recovers. Dead players
/// (`health <= 0`) are skipped, mirroring `tick_vital_regen`.
pub fn tick_exertion(
    time: Res<Time>,
    mut query: Query<
        (
            &mut Exertion,
            &MovementCooldown,
            Option<&CombatTarget>,
            &DerivedStats,
            &VitalStats,
        ),
        With<Player>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut exertion, cooldown, combat_target, derived, vitals) in &mut query {
        if vitals.health <= 0.0 {
            continue;
        }
        if exertion.current <= 0.0 {
            continue;
        }
        // Idle proxy: not mid-step and not engaged in combat. No dedicated
        // Sitting/Resting component exists, so movement-cooldown + combat-target
        // stand in for "actively exerting".
        let idle = cooldown.remaining_seconds <= 0.0 && combat_target.is_none();
        let rate = exertion_recovery_rate(derived.attributes.willpower, idle);
        exertion.add(-rate * dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::{EXERTION_BASE_MAX, EXERTION_HIGH_THRESHOLD};

    fn at_ratio(ratio: f32) -> Exertion {
        Exertion {
            current: EXERTION_BASE_MAX * ratio,
            max: EXERTION_BASE_MAX,
        }
    }

    #[test]
    fn dc_modifier_ramps_from_fifty_percent() {
        assert_eq!(exertion_dc_modifier(None), 0);
        assert_eq!(exertion_dc_modifier(Some(&at_ratio(0.49))), 0);
        // +1 at the 50% start, then +1 per 10% spent.
        assert_eq!(exertion_dc_modifier(Some(&at_ratio(0.50))), 1);
        assert_eq!(exertion_dc_modifier(Some(&at_ratio(0.60))), 2);
        assert_eq!(exertion_dc_modifier(Some(&at_ratio(0.75))), 3);
        assert_eq!(exertion_dc_modifier(Some(&at_ratio(0.90))), 5);
        // Capped at EXERTION_DC_MAX even at a fully-spent pool.
        assert_eq!(exertion_dc_modifier(Some(&at_ratio(1.0))), EXERTION_DC_MAX);
    }

    #[test]
    fn regen_multiplier_ramps_from_one_to_floor() {
        assert!((exertion_regen_multiplier(None) - 1.0).abs() < f32::EPSILON);
        assert!((exertion_regen_multiplier(Some(&at_ratio(0.5))) - 1.0).abs() < f32::EPSILON);
        // At the threshold the ramp is still 1.0.
        assert!(
            (exertion_regen_multiplier(Some(&at_ratio(EXERTION_HIGH_THRESHOLD))) - 1.0).abs()
                < 1e-4
        );
        // Full meter → floor.
        assert!(
            (exertion_regen_multiplier(Some(&at_ratio(1.0))) - EXERTION_REGEN_FLOOR).abs() < 1e-4
        );
        // Midpoint between threshold and full is between floor and 1.0.
        let mid = exertion_regen_multiplier(Some(&at_ratio((EXERTION_HIGH_THRESHOLD + 1.0) / 2.0)));
        assert!(mid > EXERTION_REGEN_FLOOR && mid < 1.0);
    }

    #[test]
    fn add_clamps_to_bounds() {
        let mut e = Exertion {
            current: 0.0,
            max: 100.0,
        };
        e.add(150.0);
        assert_eq!(e.current, 100.0);
        assert!(e.is_high());
        e.add(-300.0);
        assert_eq!(e.current, 0.0);
        assert!(!e.is_high());
        assert_eq!(e.ratio(), 0.0);
    }

    #[test]
    fn ceiling_scales_with_constitution() {
        // Constitution 10 (modifier 0) → base; higher CON widens, lower narrows.
        assert_eq!(exertion_max(10), EXERTION_BASE_MAX);
        assert!(exertion_max(18) > exertion_max(10));
        assert!(exertion_max(6) < exertion_max(10));
        // Floor protects a frail character.
        assert!(exertion_max(1) >= EXERTION_MIN_MAX);
    }

    #[test]
    fn idle_recovers_faster_and_willpower_helps() {
        let idle = exertion_recovery_rate(10, true);
        let active = exertion_recovery_rate(10, false);
        assert!(idle > active, "idle {idle} should exceed active {active}");
        // Willpower scales recovery up.
        assert!(exertion_recovery_rate(18, true) > exertion_recovery_rate(10, true));
        // Sustained sneaking (cost 2/sec) outpaces active recovery (1.5/sec) at
        // an average Willpower, so the meter climbs while sneaking — intended.
        assert!(EXERTION_COST_SNEAK_PER_SEC > exertion_recovery_rate(10, false));
    }
}
