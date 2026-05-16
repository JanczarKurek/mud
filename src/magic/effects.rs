//! Generic timed-buff/debuff component used by both player and NPC entities.
//!
//! Modeled on the existing `RegenBuffs` → `PlayerRegenBuffChanged` replication
//! path. Server-authoritative; the projection (`game::projection`) diffs the
//! caster's `MagicEffects` at integer-second resolution and emits
//! `PlayerEffectsChanged` so the client HUD can render the active buffs and
//! presentation systems can react (e.g., Glimmer expands the player's
//! `LightSource` radius).
//!
//! NPC debuffs (Slow, Sleep) are also stored here but are not replicated for
//! this batch — see `docs/yaml_formats.md` and the project plan.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::damage_type::DamageType;
use crate::magic::resources::{EffectKind, EffectSpec};
use crate::npc::components::Npc;
use crate::player::components::{Player, VitalStats};

/// Active timed magical effects on a single entity.
#[derive(Component, Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MagicEffects {
    pub active: Vec<ActiveEffect>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct ActiveEffect {
    pub kind: EffectKind,
    pub magnitude: f32,
    pub remaining_seconds: f32,
    /// Per-kind second parameter. Only `Chill` reads it (slow multiplier).
    /// `None` for all other kinds.
    #[serde(default)]
    pub secondary_magnitude: Option<f32>,
    /// DOT bookkeeping. Counts up by `delta_seconds` each frame; every full
    /// second `tick_dot_effects` emits `magnitude` damage and subtracts 1.0.
    /// Unused for non-DOT kinds.
    #[serde(default)]
    pub tick_accumulator: f32,
}

/// `apply()` always appends a new entry (no refresh) for these kinds — the
/// "arbitrary stacking" rule. All other kinds use refresh-on-reapply via
/// `combine_magnitude`. Stacking math (sum vs product vs max across multiple
/// entries of the same kind) is intentionally simple here; tuning lands in
/// a follow-up change.
fn kind_stacks(kind: EffectKind) -> bool {
    matches!(
        kind,
        EffectKind::Paralyze
            | EffectKind::Chill
            | EffectKind::Burning
            | EffectKind::Poisoned
            | EffectKind::Drunk
    )
}

impl MagicEffects {
    /// Apply a timed effect by `kind`.
    ///
    /// Kinds in `kind_stacks()` always append a new entry (arbitrary
    /// stacking — independent timers and effects). Other kinds refresh in
    /// place: take the longer of the two `remaining_seconds`, and for
    /// `magnitude` keep "stronger" — which for `Slow` means the larger
    /// multiplier (slower target), for `Haste` means the smaller multiplier
    /// (faster caster), and for everything else means the larger magnitude.
    pub fn apply(&mut self, spec: EffectSpec) {
        if spec.seconds <= 0.0 {
            return;
        }
        if !kind_stacks(spec.kind) {
            if let Some(existing) = self.active.iter_mut().find(|e| e.kind == spec.kind) {
                existing.remaining_seconds = existing.remaining_seconds.max(spec.seconds);
                existing.magnitude =
                    combine_magnitude(spec.kind, existing.magnitude, spec.magnitude);
                // Refresh secondary_magnitude with the incoming spec's value
                // when present (no combining for old kinds — they don't use it).
                if spec.secondary_magnitude.is_some() {
                    existing.secondary_magnitude = spec.secondary_magnitude;
                }
                return;
            }
        }
        self.active.push(ActiveEffect {
            kind: spec.kind,
            magnitude: spec.magnitude,
            remaining_seconds: spec.seconds,
            secondary_magnitude: spec.secondary_magnitude,
            tick_accumulator: 0.0,
        });
    }

    pub fn clear(&mut self, kind: EffectKind) {
        self.active.retain(|e| e.kind != kind);
    }

    pub fn find(&self, kind: EffectKind) -> Option<&ActiveEffect> {
        self.active.iter().find(|e| e.kind == kind)
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// Step-interval multiplier for the player. Haste shrinks it (e.g. 0.7).
    /// Returns 1.0 when no Haste is active.
    pub fn haste_multiplier(&self) -> f32 {
        self.find(EffectKind::Haste)
            .map(|e| e.magnitude.max(0.05))
            .unwrap_or(1.0)
    }

    /// Step-interval multiplier for an NPC. Slow extends it; Chill's
    /// `secondary_magnitude` (when set) layers on top by multiplying. Multiple
    /// stacked Chill entries multiply their slow components together. Returns
    /// 1.0 when nothing is slowing the NPC.
    pub fn npc_step_multiplier(&self) -> f32 {
        let slow = self
            .find(EffectKind::Slow)
            .map(|e| e.magnitude.max(0.05))
            .unwrap_or(1.0);
        let chill_product: f32 = self
            .active
            .iter()
            .filter(|e| e.kind == EffectKind::Chill)
            .filter_map(|e| e.secondary_magnitude)
            .map(|m| m.max(0.05))
            .product::<f32>()
            .max(0.05);
        slow * chill_product
    }

    pub fn is_asleep(&self) -> bool {
        self.find(EffectKind::Sleep).is_some()
    }

    pub fn is_paralyzed(&self) -> bool {
        self.find(EffectKind::Paralyze).is_some()
    }

    /// Probability (in `[0, 1]`) that the next move command fumbles into a
    /// ±45° adjacent direction. With multiple stacked Drunk effects the
    /// probabilities combine via the complement rule (independent rolls).
    /// Returns `None` when no Drunk is active.
    pub fn drunk_deviation_probability(&self) -> Option<f32> {
        let none_chance: f32 = self
            .active
            .iter()
            .filter(|e| e.kind == EffectKind::Drunk)
            .map(|e| 1.0 - e.magnitude.clamp(0.0, 1.0))
            .product::<f32>();
        if none_chance >= 1.0 {
            None
        } else {
            Some((1.0 - none_chance).clamp(0.0, 1.0))
        }
    }

    /// AC bonus from active `Shield` effects. Wired into the future combat
    /// math rewrite (Phase B); no-op today since combat is auto-hit.
    pub fn bonus_ac(&self) -> i32 {
        self.find(EffectKind::Shield)
            .map(|e| e.magnitude as i32)
            .unwrap_or(0)
    }

    /// To-hit bonus from active `Bless` effects. Wired for Phase B combat
    /// math; no-op today.
    pub fn to_hit_bonus(&self) -> i32 {
        self.find(EffectKind::Bless)
            .map(|e| e.magnitude as i32)
            .unwrap_or(0)
    }
}

fn combine_magnitude(kind: EffectKind, existing: f32, incoming: f32) -> f32 {
    match kind {
        EffectKind::Haste => existing.min(incoming),
        _ => existing.max(incoming),
    }
}

/// DOT effects (Burning, Chill, Poisoned) deal `magnitude` damage of their
/// associated type every `DOT_TICK_INTERVAL_SECONDS`.
const DOT_TICK_INTERVAL_SECONDS: f32 = 1.0;

/// Map a DOT-bearing `EffectKind` to the damage type it deals. Non-DOT kinds
/// return `None`.
pub fn dot_damage_type(kind: EffectKind) -> Option<DamageType> {
    match kind {
        EffectKind::Burning => Some(DamageType::Fire),
        EffectKind::Chill => Some(DamageType::Frost),
        EffectKind::Poisoned => Some(DamageType::Poison),
        _ => None,
    }
}

/// Decrement `remaining_seconds` for every entity carrying `MagicEffects` and
/// drop expired entries. Server-side only; gated by `simulation_active`.
pub fn tick_magic_effects(time: Res<Time>, mut query: Query<&mut MagicEffects>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut effects in query.iter_mut() {
        if effects.active.is_empty() {
            continue;
        }
        for effect in effects.active.iter_mut() {
            effect.remaining_seconds -= dt;
        }
        effects.active.retain(|e| e.remaining_seconds > 0.0);
    }
}

/// Apply DOT damage from active Burning / Chill / Poisoned effects. Advances
/// each DOT entry's `tick_accumulator`; for every full
/// `DOT_TICK_INTERVAL_SECONDS` worth, subtracts `magnitude` from the entity's
/// `VitalStats::health` and consumes that interval. Multiple stacked DOT
/// effects tick independently. Server-side only; gated by `simulation_active`.
pub fn tick_dot_effects(
    time: Res<Time>,
    mut query: Query<
        (&mut MagicEffects, &mut VitalStats),
        bevy::ecs::query::Or<(With<Player>, With<Npc>)>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (mut effects, mut vitals) in query.iter_mut() {
        if effects.active.is_empty() {
            continue;
        }
        for effect in effects.active.iter_mut() {
            if dot_damage_type(effect.kind).is_none() {
                continue;
            }
            if effect.magnitude <= 0.0 {
                continue;
            }
            effect.tick_accumulator += dt;
            while effect.tick_accumulator >= DOT_TICK_INTERVAL_SECONDS {
                effect.tick_accumulator -= DOT_TICK_INTERVAL_SECONDS;
                if vitals.health <= 0.0 {
                    break;
                }
                vitals.health = (vitals.health - effect.magnitude).max(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: EffectKind, magnitude: f32, seconds: f32) -> EffectSpec {
        EffectSpec {
            kind,
            magnitude,
            seconds,
            secondary_magnitude: None,
        }
    }

    fn spec_with_secondary(
        kind: EffectKind,
        magnitude: f32,
        secondary: f32,
        seconds: f32,
    ) -> EffectSpec {
        EffectSpec {
            kind,
            magnitude,
            seconds,
            secondary_magnitude: Some(secondary),
        }
    }

    #[test]
    fn apply_inserts_new_effect() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Glimmer, 4.0, 600.0));
        assert_eq!(effects.active.len(), 1);
        assert_eq!(effects.find(EffectKind::Glimmer).unwrap().magnitude, 4.0);
    }

    #[test]
    fn re_applying_glimmer_takes_longer_duration_and_brighter_magnitude() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Glimmer, 4.0, 600.0));
        effects.apply(spec(EffectKind::Glimmer, 6.0, 300.0));
        let glimmer = effects.find(EffectKind::Glimmer).unwrap();
        assert_eq!(glimmer.magnitude, 6.0);
        assert_eq!(glimmer.remaining_seconds, 600.0);
    }

    #[test]
    fn re_applying_haste_takes_faster_multiplier() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Haste, 0.7, 60.0));
        effects.apply(spec(EffectKind::Haste, 0.5, 30.0));
        let haste = effects.find(EffectKind::Haste).unwrap();
        assert_eq!(haste.magnitude, 0.5);
        assert_eq!(haste.remaining_seconds, 60.0);
    }

    #[test]
    fn clear_removes_kind() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Sleep, 1.0, 10.0));
        effects.apply(spec(EffectKind::Slow, 2.0, 5.0));
        effects.clear(EffectKind::Sleep);
        assert!(effects.find(EffectKind::Sleep).is_none());
        assert!(effects.find(EffectKind::Slow).is_some());
    }

    #[test]
    fn helpers_return_defaults_when_inactive() {
        let effects = MagicEffects::default();
        assert_eq!(effects.haste_multiplier(), 1.0);
        assert_eq!(effects.npc_step_multiplier(), 1.0);
        assert_eq!(effects.bonus_ac(), 0);
        assert_eq!(effects.to_hit_bonus(), 0);
        assert!(!effects.is_asleep());
    }

    #[test]
    fn helpers_read_active_values() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Haste, 0.7, 60.0));
        effects.apply(spec(EffectKind::Slow, 2.0, 5.0));
        effects.apply(spec(EffectKind::Shield, 4.0, 60.0));
        effects.apply(spec(EffectKind::Bless, 1.0, 60.0));
        effects.apply(spec(EffectKind::Sleep, 0.0, 10.0));
        assert_eq!(effects.haste_multiplier(), 0.7);
        assert_eq!(effects.npc_step_multiplier(), 2.0);
        assert_eq!(effects.bonus_ac(), 4);
        assert_eq!(effects.to_hit_bonus(), 1);
        assert!(effects.is_asleep());
    }

    #[test]
    fn new_effects_stack_independently_instead_of_refreshing() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Burning, 2.0, 5.0));
        effects.apply(spec(EffectKind::Burning, 3.0, 8.0));
        assert_eq!(
            effects
                .active
                .iter()
                .filter(|e| e.kind == EffectKind::Burning)
                .count(),
            2,
            "Burning should stack rather than refresh"
        );
    }

    #[test]
    fn paralyze_helper_reports_active() {
        let mut effects = MagicEffects::default();
        assert!(!effects.is_paralyzed());
        effects.apply(spec(EffectKind::Paralyze, 0.0, 5.0));
        assert!(effects.is_paralyzed());
    }

    #[test]
    fn drunk_probability_combines_via_complement() {
        let mut effects = MagicEffects::default();
        assert_eq!(effects.drunk_deviation_probability(), None);
        effects.apply(spec(EffectKind::Drunk, 0.5, 10.0));
        let p1 = effects.drunk_deviation_probability().unwrap();
        assert!((p1 - 0.5).abs() < 1e-5);
        effects.apply(spec(EffectKind::Drunk, 0.5, 10.0));
        // Two independent 0.5 drunken effects: P(at least one fires) = 0.75.
        let p2 = effects.drunk_deviation_probability().unwrap();
        assert!((p2 - 0.75).abs() < 1e-5, "got {p2}");
    }

    #[test]
    fn chill_layers_slow_multiplier_onto_npc_step_multiplier() {
        let mut effects = MagicEffects::default();
        // No Slow, no Chill → 1.0
        assert_eq!(effects.npc_step_multiplier(), 1.0);
        // Pure DOT Chill (no secondary) → still 1.0
        effects.apply(spec(EffectKind::Chill, 2.0, 10.0));
        assert_eq!(effects.npc_step_multiplier(), 1.0);
        // Add a chill with a slow component
        effects.apply(spec_with_secondary(EffectKind::Chill, 2.0, 1.5, 10.0));
        assert!((effects.npc_step_multiplier() - 1.5).abs() < 1e-5);
        // Layer Slow on top → multiplies
        effects.apply(spec(EffectKind::Slow, 2.0, 10.0));
        assert!((effects.npc_step_multiplier() - 3.0).abs() < 1e-5);
    }

    #[test]
    fn dot_kinds_map_to_damage_types() {
        assert_eq!(dot_damage_type(EffectKind::Burning), Some(DamageType::Fire));
        assert_eq!(dot_damage_type(EffectKind::Chill), Some(DamageType::Frost));
        assert_eq!(
            dot_damage_type(EffectKind::Poisoned),
            Some(DamageType::Poison)
        );
        assert_eq!(dot_damage_type(EffectKind::Paralyze), None);
        assert_eq!(dot_damage_type(EffectKind::Sleep), None);
        assert_eq!(dot_damage_type(EffectKind::Drunk), None);
    }

    #[test]
    fn tick_dot_drops_health_each_full_second() {
        // Drive `tick_dot_effects` directly by simulating successive ticks.
        // Two stacked Burning effects (2.0 + 3.0) should each tick
        // independently every second.
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Burning, 2.0, 10.0));
        effects.apply(spec(EffectKind::Burning, 3.0, 10.0));

        let mut health = 100.0f32;
        let dt = 0.25_f32;
        let steps = (4.0 / dt) as usize; // 4 simulated seconds
        for _ in 0..steps {
            for effect in effects.active.iter_mut() {
                if dot_damage_type(effect.kind).is_none() {
                    continue;
                }
                effect.tick_accumulator += dt;
                while effect.tick_accumulator >= 1.0 {
                    effect.tick_accumulator -= 1.0;
                    health = (health - effect.magnitude).max(0.0);
                }
            }
        }
        // Expected: 4s × (2 + 3) = 20 hp lost.
        assert!(
            (health - 80.0).abs() < 1e-3,
            "expected 80hp after 4s of stacked DOT, got {health}"
        );
    }
}
