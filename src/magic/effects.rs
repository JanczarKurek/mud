//! Generic timed-buff/debuff component used by both player and NPC entities.
//!
//! Modeled on the existing `RegenBuffs` → `PlayerRegenBuffChanged` replication
//! path. Server-authoritative; the projection (`game::projection`) diffs the
//! caster's `MagicEffects` at integer-second resolution and emits
//! `PlayerEffectsChanged` so the client HUD can render the active buffs and
//! presentation systems can react (e.g., Glimmer expands the player's
//! `LightSource` radius).
//!
//! Stacking model: every `apply()` pushes a new `ActiveEffect`; multiple
//! entries of the same `kind` coexist with independent durations. Readers
//! aggregate them sublinearly:
//!
//! * DoTs and unbounded positive magnitudes (Shield AC, Bless to-hit, Glimmer
//!   radius) — L2 norm `√(Σ mᵢ²)`. Re-cast amplification: 1→1×, 2→1.41×,
//!   4→2×, 9→3×.
//! * Capped probabilities (Drunk) — complement `1 − Π(1 − mᵢ)`.
//! * Speed multipliers (Haste, Slow, Chill secondary) — product, with the
//!   existing `.max(0.05)` floor on the result.
//! * Boolean CC (Sleep, Paralyze) — presence-by-any-entry; at `apply` time
//!   the incoming duration is scaled by `CC_DR_FACTOR^N` where `N` = number
//!   of currently-active entries of the same kind, providing diminishing
//!   returns on chain-stuns. DR resets naturally once older entries expire.
//!
//! DoT damage is delivered once per kind per `DOT_TICK_INTERVAL_SECONDS` using
//! the aggregated L2 magnitude; the tick clock lives on `MagicEffects`
//! (`kind_tick_accumulators`) — not on individual entries — so stacked DoTs
//! share a single tick phase rather than firing independently.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use crate::combat::damage_type::DamageType;
use crate::magic::resources::{EffectKind, EffectSpec};
use crate::npc::components::Npc;
use crate::player::components::{Player, PlayerId, VitalStats};

/// Active timed magical effects on a single entity.
#[derive(Component, Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MagicEffects {
    pub active: Vec<ActiveEffect>,
    /// Per-kind DoT tick accumulators. Shared across all entries of a given
    /// kind so stacked DoTs deliver one aggregated L2 tick per second instead
    /// of one tick per entry. Entries with no surviving active effects of
    /// their kind are pruned by `tick_magic_effects`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kind_tick_accumulators: Vec<(EffectKind, f32)>,
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
    /// Player who applied this effect, used for XP attribution when a DoT
    /// tick delivers the killing blow. `None` means no player attribution
    /// (NPC-applied effects, environmental sources).
    #[serde(default)]
    pub caster: Option<PlayerId>,
}

/// Boolean-CC duration multiplier per existing active stack of the same kind.
const CC_DR_FACTOR: f32 = 0.5;

/// Boolean crowd-control kinds (no magnitude; presence is the effect).
/// Incoming durations are scaled by `CC_DR_FACTOR^N` against active stacks.
fn is_boolean_cc(kind: EffectKind) -> bool {
    matches!(kind, EffectKind::Sleep | EffectKind::Paralyze)
}

impl MagicEffects {
    /// Apply a timed effect. Always appends a new entry (no in-place merge).
    /// For boolean CC kinds the incoming `seconds` is first scaled by
    /// `CC_DR_FACTOR^N` where `N` is the number of currently active entries
    /// of the same kind, providing diminishing returns on chain-stuns.
    ///
    /// `caster` is the player who applied this effect (used for XP
    /// attribution when a DoT tick from this entry delivers a killing blow).
    /// Pass `None` for NPC-applied effects and environmental sources.
    pub fn apply(&mut self, spec: EffectSpec, caster: Option<PlayerId>) {
        if spec.seconds <= 0.0 {
            return;
        }
        let mut seconds = spec.seconds;
        if is_boolean_cc(spec.kind) {
            let n_active = self.active.iter().filter(|e| e.kind == spec.kind).count();
            seconds *= CC_DR_FACTOR.powi(n_active as i32);
        }
        if seconds <= 0.0 {
            return;
        }
        self.active.push(ActiveEffect {
            kind: spec.kind,
            magnitude: spec.magnitude,
            remaining_seconds: seconds,
            secondary_magnitude: spec.secondary_magnitude,
            caster,
        });
    }

    pub fn clear(&mut self, kind: EffectKind) {
        self.active.retain(|e| e.kind != kind);
        self.kind_tick_accumulators.retain(|(k, _)| *k != kind);
    }

    pub fn find(&self, kind: EffectKind) -> Option<&ActiveEffect> {
        self.active.iter().find(|e| e.kind == kind)
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// L2 norm of all active magnitudes for `kind`: `√(Σ mᵢ²)`. Used for
    /// DoT damage, Shield AC, Bless to-hit, and Glimmer radius — sublinear
    /// in the number of stacks. Returns 0.0 when no entries are active.
    pub fn l2_magnitude(&self, kind: EffectKind) -> f32 {
        self.active
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.magnitude * e.magnitude)
            .sum::<f32>()
            .sqrt()
    }

    /// Step-interval multiplier for the player. Haste shrinks it; multiple
    /// stacked Haste entries multiply (a 0.7 followed by a 0.7 → 0.49).
    /// Floor at 0.05 so casters can't reach zero. Returns 1.0 when no Haste
    /// is active.
    pub fn haste_multiplier(&self) -> f32 {
        let product: f32 = self
            .active
            .iter()
            .filter(|e| e.kind == EffectKind::Haste)
            .map(|e| e.magnitude)
            .product();
        if !self.active.iter().any(|e| e.kind == EffectKind::Haste) {
            return 1.0;
        }
        product.max(0.05)
    }

    /// Step-interval multiplier for an NPC. Slow extends it; Chill's
    /// `secondary_magnitude` (when set) layers on top by multiplying.
    /// Multiple stacked Slow entries and multiple Chill secondaries all
    /// multiply together. Returns 1.0 when nothing is slowing the NPC.
    pub fn npc_step_multiplier(&self) -> f32 {
        let slow_product: f32 = self
            .active
            .iter()
            .filter(|e| e.kind == EffectKind::Slow)
            .map(|e| e.magnitude)
            .product();
        let any_slow = self.active.iter().any(|e| e.kind == EffectKind::Slow);
        let slow = if any_slow {
            slow_product.max(0.05)
        } else {
            1.0
        };
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
        self.active.iter().any(|e| e.kind == EffectKind::Sleep)
    }

    pub fn is_paralyzed(&self) -> bool {
        self.active.iter().any(|e| e.kind == EffectKind::Paralyze)
    }

    /// Probability (in `[0, 1]`) that the next move command fumbles into a
    /// ±45° adjacent direction. With multiple stacked Drunk effects the
    /// probabilities combine via the complement rule (independent rolls).
    /// Returns `None` when no Drunk is active.
    pub fn drunk_deviation_probability(&self) -> Option<f32> {
        let mut any = false;
        let none_chance: f32 = self
            .active
            .iter()
            .filter(|e| e.kind == EffectKind::Drunk)
            .inspect(|_| any = true)
            .map(|e| 1.0 - e.magnitude.clamp(0.0, 1.0))
            .product();
        if !any {
            None
        } else {
            Some((1.0 - none_chance).clamp(0.0, 1.0))
        }
    }

    /// Aggregated AC bonus from active `Shield` effects. L2 norm of all
    /// Shield magnitudes, truncated to integer for the combat math contract.
    pub fn bonus_ac(&self) -> i32 {
        self.l2_magnitude(EffectKind::Shield) as i32
    }

    /// Aggregated to-hit bonus from active `Bless` effects. L2 norm of all
    /// Bless magnitudes, truncated to integer.
    pub fn to_hit_bonus(&self) -> i32 {
        self.l2_magnitude(EffectKind::Bless) as i32
    }

    /// Aggregated Glimmer radius. L2 norm of all Glimmer magnitudes. Returns
    /// 0.0 when no Glimmer is active — callers fall back to a baseline.
    pub fn glimmer_radius(&self) -> f32 {
        self.l2_magnitude(EffectKind::Glimmer)
    }
}

/// DoT effects (Burning, Chill, Poisoned) deal an aggregated L2 magnitude
/// of damage of their associated type every `DOT_TICK_INTERVAL_SECONDS`.
const DOT_TICK_INTERVAL_SECONDS: f32 = 1.0;

/// Map a DoT-bearing `EffectKind` to the damage type it deals. Non-DoT kinds
/// return `None`.
pub fn dot_damage_type(kind: EffectKind) -> Option<DamageType> {
    match kind {
        EffectKind::Burning => Some(DamageType::Fire),
        EffectKind::Chill => Some(DamageType::Frost),
        EffectKind::Poisoned => Some(DamageType::Poison),
        _ => None,
    }
}

/// All DoT kinds the tick system iterates each frame.
const DOT_KINDS: [EffectKind; 3] = [EffectKind::Burning, EffectKind::Chill, EffectKind::Poisoned];

fn accumulator_for_kind(accs: &mut Vec<(EffectKind, f32)>, kind: EffectKind) -> &mut f32 {
    if let Some(idx) = accs.iter().position(|(k, _)| *k == kind) {
        &mut accs[idx].1
    } else {
        accs.push((kind, 0.0));
        &mut accs.last_mut().unwrap().1
    }
}

/// Decrement `remaining_seconds` for every entity carrying `MagicEffects`,
/// drop expired entries, and prune per-kind tick accumulators whose kind has
/// no surviving entries. Server-side only; gated by `simulation_active`.
pub fn tick_magic_effects(time: Res<Time>, mut query: Query<&mut MagicEffects>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut effects in query.iter_mut() {
        if effects.active.is_empty() && effects.kind_tick_accumulators.is_empty() {
            continue;
        }
        for effect in effects.active.iter_mut() {
            effect.remaining_seconds -= dt;
        }
        effects.active.retain(|e| e.remaining_seconds > 0.0);
        // Drop tick accumulators for kinds with no surviving entries so the
        // next cast of that kind starts on a fresh phase.
        let surviving_kinds: Vec<EffectKind> = effects.active.iter().map(|e| e.kind).collect();
        effects
            .kind_tick_accumulators
            .retain(|(k, _)| surviving_kinds.contains(k));
    }
}

/// Apply DoT damage from active Burning / Chill / Poisoned effects. For each
/// DoT kind with any active entries on the target, advance a shared per-kind
/// tick accumulator; on every full `DOT_TICK_INTERVAL_SECONDS` worth, push a
/// `DamageEvent` carrying the L2-aggregated magnitude of damage. Attribution
/// uses the most-recently-applied entry's `caster` of that kind (last-cast
/// wins for the kill credit); entries with no caster fall back to
/// `DamageSource::Environment`. Server-side only; gated by `simulation_active`.
pub fn tick_dot_effects(
    time: Res<Time>,
    mut query: Query<
        (Entity, &mut MagicEffects, &VitalStats),
        bevy::ecs::query::Or<(With<Player>, With<Npc>)>,
    >,
    mut pending_damage: ResMut<PendingDamageEvents>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (entity, mut effects, vitals) in query.iter_mut() {
        if effects.active.is_empty() {
            continue;
        }
        if vitals.health <= 0.0 {
            continue;
        }
        for kind in DOT_KINDS {
            let dps = effects.l2_magnitude(kind);
            if dps <= 0.0 {
                continue;
            }
            // Most-recent (last-pushed) active entry of this kind owns the
            // attribution. Matches the "last hit wins" rule for direct hits.
            let caster = effects
                .active
                .iter()
                .rev()
                .find(|e| e.kind == kind)
                .and_then(|e| e.caster);
            let source = match caster {
                Some(player_id) => DamageSource::OwnedByPlayer(player_id),
                None => DamageSource::Environment,
            };
            let damage_type = dot_damage_type(kind).unwrap_or(DamageType::Arcane);
            let acc = accumulator_for_kind(&mut effects.kind_tick_accumulators, kind);
            *acc += dt;
            while *acc >= DOT_TICK_INTERVAL_SECONDS {
                *acc -= DOT_TICK_INTERVAL_SECONDS;
                pending_damage.push(DamageEvent {
                    target: entity,
                    amount: dps,
                    source,
                    damage_type,
                    vfx_override: None,
                });
            }
        }
    }
}

/// Apply each spec in `specs` to `entity`'s `MagicEffects`, lazily attaching
/// a fresh component via `Commands::insert` when `existing` is `None`. The
/// canonical "apply a magical effect to any actor" entry point — callers
/// don't need to know whether the entity (player, NPC, future summon) was
/// spawned with a `MagicEffects` component or not, which closes the gap
/// where step triggers and NPC self-buff casts silently dropped effects
/// against entities that hadn't been hit before.
///
/// `existing` should be the result of a `&mut MagicEffects` query on
/// `entity` (or `None` when no such component exists). The helper does no
/// querying itself, so it composes with both standalone
/// `Query<&mut MagicEffects>` (use `q.get_mut(e).ok().as_deref_mut()`) and
/// `ParamSet` joins that already carry `Option<&mut MagicEffects>`.
pub fn apply_effects_lazy(
    entity: Entity,
    specs: &[EffectSpec],
    caster: Option<PlayerId>,
    existing: Option<&mut MagicEffects>,
    commands: &mut Commands,
) {
    if specs.is_empty() {
        return;
    }
    match existing {
        Some(effects) => {
            for spec in specs {
                effects.apply(*spec, caster);
            }
        }
        None => {
            let mut new_effects = MagicEffects::default();
            for spec in specs {
                new_effects.apply(*spec, caster);
            }
            // Only attach when something actually landed — a list of all
            // zero-duration specs must not insert an empty component.
            if !new_effects.is_empty() {
                commands.entity(entity).insert(new_effects);
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
        effects.apply(spec(EffectKind::Glimmer, 4.0, 600.0), None);
        assert_eq!(effects.active.len(), 1);
        assert_eq!(effects.find(EffectKind::Glimmer).unwrap().magnitude, 4.0);
    }

    #[test]
    fn clear_removes_kind_and_accumulator() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Sleep, 1.0, 10.0), None);
        effects.apply(spec(EffectKind::Slow, 2.0, 5.0), None);
        // Seed a Burning entry + force its accumulator to exist by faking a
        // tick path (we use `apply` then a direct seed).
        effects.apply(spec(EffectKind::Burning, 3.0, 10.0), None);
        accumulator_for_kind(&mut effects.kind_tick_accumulators, EffectKind::Burning);
        effects.clear(EffectKind::Sleep);
        effects.clear(EffectKind::Burning);
        assert!(effects.find(EffectKind::Sleep).is_none());
        assert!(effects.find(EffectKind::Burning).is_none());
        assert!(effects.find(EffectKind::Slow).is_some());
        assert!(!effects
            .kind_tick_accumulators
            .iter()
            .any(|(k, _)| *k == EffectKind::Burning));
    }

    #[test]
    fn helpers_return_defaults_when_inactive() {
        let effects = MagicEffects::default();
        assert_eq!(effects.haste_multiplier(), 1.0);
        assert_eq!(effects.npc_step_multiplier(), 1.0);
        assert_eq!(effects.bonus_ac(), 0);
        assert_eq!(effects.to_hit_bonus(), 0);
        assert_eq!(effects.glimmer_radius(), 0.0);
        assert!(!effects.is_asleep());
        assert!(!effects.is_paralyzed());
        assert_eq!(effects.drunk_deviation_probability(), None);
    }

    #[test]
    fn shield_and_bless_aggregate_via_l2() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Shield, 3.0, 60.0), None);
        effects.apply(spec(EffectKind::Shield, 4.0, 60.0), None);
        // √(9 + 16) = 5 → truncates to 5
        assert_eq!(effects.bonus_ac(), 5);
        effects.apply(spec(EffectKind::Bless, 3.0, 60.0), None);
        effects.apply(spec(EffectKind::Bless, 4.0, 60.0), None);
        assert_eq!(effects.to_hit_bonus(), 5);
    }

    #[test]
    fn glimmer_aggregates_via_l2() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Glimmer, 4.0, 600.0), None);
        effects.apply(spec(EffectKind::Glimmer, 3.0, 300.0), None);
        // √(16 + 9) = 5
        assert!((effects.glimmer_radius() - 5.0).abs() < 1e-5);
    }

    #[test]
    fn dot_recast_same_spell_amplifies_by_sqrt2() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Burning, 10.0, 10.0), None);
        effects.apply(spec(EffectKind::Burning, 10.0, 10.0), None);
        let dps = effects.l2_magnitude(EffectKind::Burning);
        assert!(
            (dps - (200.0f32).sqrt()).abs() < 1e-4,
            "expected √200 ≈ 14.14 DPS, got {dps}"
        );
    }

    #[test]
    fn dot_strong_short_plus_weak_long_is_subadditive() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Burning, 10.0, 1.0), None);
        effects.apply(spec(EffectKind::Burning, 1.0, 10.0), None);
        let dps = effects.l2_magnitude(EffectKind::Burning);
        // √(100 + 1) ≈ 10.05 — far less than 11 (naive additive).
        assert!(
            (dps - (101.0f32).sqrt()).abs() < 1e-4,
            "expected √101 ≈ 10.05, got {dps}"
        );
    }

    #[test]
    fn dot_per_entry_durations_are_independent() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Burning, 5.0, 2.0), None);
        effects.apply(spec(EffectKind::Burning, 5.0, 10.0), None);
        // Hand-roll the duration-tick to avoid wiring up a Bevy world here.
        for effect in effects.active.iter_mut() {
            effect.remaining_seconds -= 3.0;
        }
        effects.active.retain(|e| e.remaining_seconds > 0.0);
        assert_eq!(effects.active.len(), 1);
        assert!((effects.active[0].remaining_seconds - 7.0).abs() < 1e-5);
    }

    #[test]
    fn aggregator_is_commutative_in_apply_order() {
        let mut a = MagicEffects::default();
        a.apply(spec(EffectKind::Burning, 3.0, 10.0), None);
        a.apply(spec(EffectKind::Burning, 7.0, 5.0), None);

        let mut b = MagicEffects::default();
        b.apply(spec(EffectKind::Burning, 7.0, 5.0), None);
        b.apply(spec(EffectKind::Burning, 3.0, 10.0), None);

        assert!(
            (a.l2_magnitude(EffectKind::Burning) - b.l2_magnitude(EffectKind::Burning)).abs()
                < 1e-5
        );
    }

    #[test]
    fn drunk_complement_saturates_below_one() {
        let mut effects = MagicEffects::default();
        for _ in 0..3 {
            effects.apply(spec(EffectKind::Drunk, 0.5, 10.0), None);
        }
        // 1 - 0.5^3 = 0.875
        let p = effects.drunk_deviation_probability().unwrap();
        assert!((p - 0.875).abs() < 1e-5, "got {p}");
        // Adding more Drunks pushes toward 1 and saturates there (the
        // clamp guarantees we never exceed 1).
        for _ in 0..10 {
            effects.apply(spec(EffectKind::Drunk, 0.9, 10.0), None);
        }
        let p2 = effects.drunk_deviation_probability().unwrap();
        assert!(p2 <= 1.0 && p2 > 0.999, "got {p2}");
    }

    #[test]
    fn slow_product_stacks_multiplicatively() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Slow, 2.0, 10.0), None);
        effects.apply(spec(EffectKind::Slow, 2.0, 10.0), None);
        assert!((effects.npc_step_multiplier() - 4.0).abs() < 1e-5);
    }

    #[test]
    fn haste_product_stacks_multiplicatively() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Haste, 0.7, 60.0), None);
        effects.apply(spec(EffectKind::Haste, 0.7, 60.0), None);
        // 0.49, well above the 0.05 floor.
        assert!((effects.haste_multiplier() - 0.49).abs() < 1e-5);
    }

    #[test]
    fn sleep_dr_halves_each_recast() {
        let mut effects = MagicEffects::default();
        for _ in 0..4 {
            effects.apply(spec(EffectKind::Sleep, 10.0, 10.0), None);
        }
        let durations: Vec<f32> = effects
            .active
            .iter()
            .filter(|e| e.kind == EffectKind::Sleep)
            .map(|e| e.remaining_seconds)
            .collect();
        assert_eq!(durations.len(), 4);
        assert!((durations[0] - 10.0).abs() < 1e-5, "first {durations:?}");
        assert!((durations[1] - 5.0).abs() < 1e-5, "second {durations:?}");
        assert!((durations[2] - 2.5).abs() < 1e-5, "third {durations:?}");
        assert!((durations[3] - 1.25).abs() < 1e-5, "fourth {durations:?}");
    }

    #[test]
    fn paralyze_helper_reports_active() {
        let mut effects = MagicEffects::default();
        assert!(!effects.is_paralyzed());
        effects.apply(spec(EffectKind::Paralyze, 0.0, 5.0), None);
        assert!(effects.is_paralyzed());
    }

    #[test]
    fn chill_layers_slow_multiplier_onto_npc_step_multiplier() {
        let mut effects = MagicEffects::default();
        // No Slow, no Chill → 1.0
        assert_eq!(effects.npc_step_multiplier(), 1.0);
        // Pure DoT Chill (no secondary) → still 1.0
        effects.apply(spec(EffectKind::Chill, 2.0, 10.0), None);
        assert_eq!(effects.npc_step_multiplier(), 1.0);
        // Add a Chill with a slow component
        effects.apply(spec_with_secondary(EffectKind::Chill, 2.0, 1.5, 10.0), None);
        assert!((effects.npc_step_multiplier() - 1.5).abs() < 1e-5);
        // Layer Slow on top → multiplies
        effects.apply(spec(EffectKind::Slow, 2.0, 10.0), None);
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

    /// Two Burning entries applied 0.4s apart fire a single combined tick at
    /// game-time 1.0s, dealing the L2-aggregated damage in one hit rather
    /// than two staggered ticks. The per-kind shared accumulator is what
    /// guarantees this.
    #[test]
    fn dot_tick_accumulator_is_shared_per_kind() {
        let mut effects = MagicEffects::default();
        let mut health = 100.0f32;

        // First entry arrives at t=0.
        effects.apply(spec(EffectKind::Burning, 3.0, 10.0), None);
        // Advance 0.4s — no tick yet.
        let dps = effects.l2_magnitude(EffectKind::Burning);
        *accumulator_for_kind(&mut effects.kind_tick_accumulators, EffectKind::Burning) += 0.4;
        let _ = dps;

        // Second entry arrives at t=0.4.
        effects.apply(spec(EffectKind::Burning, 4.0, 10.0), None);

        // Advance another 0.6s → t=1.0, accumulator hits the tick threshold.
        let dps = effects.l2_magnitude(EffectKind::Burning);
        let acc = accumulator_for_kind(&mut effects.kind_tick_accumulators, EffectKind::Burning);
        *acc += 0.6;
        if *acc >= 1.0 {
            *acc -= 1.0;
            health -= dps;
        }
        // dps = √(9 + 16) = 5, so one combined tick of 5.
        assert!(
            (health - 95.0).abs() < 1e-4,
            "expected 95.0hp after one combined tick, got {health}"
        );
    }

    #[test]
    fn applying_zero_or_negative_seconds_is_a_noop() {
        let mut effects = MagicEffects::default();
        effects.apply(spec(EffectKind::Burning, 5.0, 0.0), None);
        effects.apply(spec(EffectKind::Sleep, 0.0, 0.0), None);
        assert!(effects.active.is_empty());
    }

    #[test]
    fn apply_effects_lazy_mutates_existing_in_place() {
        // When the target already carries `MagicEffects`, the helper must
        // funnel into it directly — the lazy-attach branch is only for
        // entities without the component.
        let mut app = App::new();
        let entity = app.world_mut().spawn(MagicEffects::default()).id();
        let specs = vec![spec(EffectKind::Burning, 4.0, 2.0)];
        app.add_systems(
            Update,
            move |mut commands: Commands, mut q: Query<&mut MagicEffects>| {
                let existing = q.get_mut(entity).ok();
                apply_effects_lazy(
                    entity,
                    &specs,
                    None,
                    existing.map(|m| m.into_inner()),
                    &mut commands,
                );
            },
        );
        app.update();
        let effects = app.world().get::<MagicEffects>(entity).unwrap();
        assert_eq!(effects.active.len(), 1);
        assert_eq!(effects.active[0].kind, EffectKind::Burning);
    }

    #[test]
    fn apply_effects_lazy_attaches_when_missing() {
        // NPCs spawn without `MagicEffects`; an on_stepped trigger applying
        // `burning` to one must still land. The helper inserts the component
        // via Commands on the next flush.
        let mut app = App::new();
        let entity = app.world_mut().spawn_empty().id();
        let specs = vec![spec(EffectKind::Burning, 3.0, 2.0)];
        app.add_systems(
            Update,
            move |mut commands: Commands, mut q: Query<&mut MagicEffects>| {
                let existing = q.get_mut(entity).ok();
                apply_effects_lazy(
                    entity,
                    &specs,
                    None,
                    existing.map(|m| m.into_inner()),
                    &mut commands,
                );
            },
        );
        app.update();
        let effects = app
            .world()
            .get::<MagicEffects>(entity)
            .expect("MagicEffects should have been lazily attached");
        assert_eq!(effects.active.len(), 1);
        assert_eq!(effects.active[0].kind, EffectKind::Burning);
    }

    #[test]
    fn apply_effects_lazy_does_not_attach_empty_component() {
        // A list of all-zero-duration specs is a no-op inside
        // `MagicEffects::apply`. The helper must not leave an empty
        // `MagicEffects` component on entities that didn't already have one,
        // since the projection layer would then start replicating it.
        let mut app = App::new();
        let entity = app.world_mut().spawn_empty().id();
        let specs = vec![spec(EffectKind::Burning, 5.0, 0.0)];
        app.add_systems(
            Update,
            move |mut commands: Commands, mut q: Query<&mut MagicEffects>| {
                let existing = q.get_mut(entity).ok();
                apply_effects_lazy(
                    entity,
                    &specs,
                    None,
                    existing.map(|m| m.into_inner()),
                    &mut commands,
                );
            },
        );
        app.update();
        assert!(app.world().get::<MagicEffects>(entity).is_none());
    }
}
