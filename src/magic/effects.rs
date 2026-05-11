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

use crate::magic::resources::{EffectKind, EffectSpec};

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
}

impl MagicEffects {
    /// Apply (or refresh) a timed effect by `kind`. Refresh policy: take the
    /// longer of the two `remaining_seconds`, and for `magnitude` keep
    /// "stronger" — which for `Slow` means the larger multiplier (slower
    /// target), for `Haste` means the smaller multiplier (faster caster),
    /// and for everything else means the larger magnitude.
    pub fn apply(&mut self, spec: EffectSpec) {
        if spec.seconds <= 0.0 {
            return;
        }
        if let Some(existing) = self.active.iter_mut().find(|e| e.kind == spec.kind) {
            existing.remaining_seconds = existing.remaining_seconds.max(spec.seconds);
            existing.magnitude = combine_magnitude(spec.kind, existing.magnitude, spec.magnitude);
        } else {
            self.active.push(ActiveEffect {
                kind: spec.kind,
                magnitude: spec.magnitude,
                remaining_seconds: spec.seconds,
            });
        }
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

    /// Step-interval multiplier for an NPC. Slow extends it (e.g. 2.0).
    /// Returns 1.0 when no Slow is active.
    pub fn npc_step_multiplier(&self) -> f32 {
        self.find(EffectKind::Slow)
            .map(|e| e.magnitude.max(0.05))
            .unwrap_or(1.0)
    }

    pub fn is_asleep(&self) -> bool {
        self.find(EffectKind::Sleep).is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: EffectKind, magnitude: f32, seconds: f32) -> EffectSpec {
        EffectSpec {
            kind,
            magnitude,
            seconds,
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
}
