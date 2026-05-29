//! NPC spellcasting profile + supporting types.
//!
//! Mirrors how `AttackProfile` describes melee/ranged combat: the spawn flow
//! reads `OverworldObjectDefinition::spellcasting` and attaches a
//! `SpellcastingProfile` component to NPCs that have one. The combat turn
//! reads this component (see `combat::npc_casting`) and picks the first spell
//! whose cooldown is ready and whose conditions all pass. First-match wins —
//! authors order their spell list from highest to lowest priority (heal > CC
//! > nuke > filler), giving deterministic, designable casting behavior.
//!
//! `last_cast_at` is wall-clock based (`Time::elapsed_secs`); on save/load it
//! is intentionally not persisted so reloaded NPCs always start with every
//! spell off cooldown. Cooldowns aren't long enough for that to matter.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::magic::resources::EffectKind;

/// Spells an NPC can cast in combat. The combat turn evaluates `spells` in
/// declaration order and casts the first one whose cooldown is ready and
/// whose conditions all pass; otherwise it falls back to the NPC's physical
/// `AttackProfile`.
#[derive(Component, Clone, Debug)]
pub struct SpellcastingProfile {
    pub spells: Vec<NpcSpellEntry>,
}

#[derive(Clone, Debug)]
pub struct NpcSpellEntry {
    /// Id of the spell in `assets/spells/`.
    pub spell_id: String,
    /// Seconds that must elapse between successful casts of this entry.
    pub cooldown_seconds: f32,
    /// Wall-clock time (`Time::elapsed_secs`) of the last successful cast.
    /// `f32::NEG_INFINITY` marks "never cast" so the first turn always
    /// satisfies the cooldown.
    pub last_cast_at: f32,
    pub target_kind: NpcSpellTargetKind,
    pub conditions: Vec<NpcSpellCondition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpcSpellTargetKind {
    /// Single-target spell aimed at `CombatTarget`'s entity tile.
    Target,
    /// AoE / tile-target spell centered on `CombatTarget`'s tile.
    TargetTile,
    /// Untargeted spell cast on the NPC's own tile (e.g. self-heal).
    SelfCast,
}

#[derive(Clone, Copy, Debug)]
pub enum NpcSpellCondition {
    /// Chebyshev distance to the combat target ≤ `i32`.
    TargetWithinRange(i32),
    /// `combat::systems::chebyshev_distance` line-of-sight isn't checked here;
    /// `combat::systems::is_target_in_range` already gated us into this turn.
    /// `TargetVisible` re-runs the Bresenham LoS to refuse casting through
    /// walls (mages should peek before they zap).
    TargetVisible,
    /// Target HP fraction (current / max) ≤ value. Useful for "execute" picks.
    TargetHpBelowFraction(f32),
    /// NPC's own HP fraction ≤ value. Triggers panic heals.
    SelfHpBelowFraction(f32),
    /// Target does NOT have an active effect of this kind. Stops re-Sleeping
    /// an already-asleep player.
    TargetWithoutEffect(EffectKind),
    /// NPC does NOT have an active effect of this kind on itself.
    SelfWithoutEffect(EffectKind),
}

// ── YAML schema (deserialized off NPC metadata.yaml) ──────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpellcastingDef {
    pub spells: Vec<NpcSpellEntryDef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct NpcSpellEntryDef {
    pub spell_id: String,
    pub cooldown_seconds: f32,
    #[serde(default = "default_target_kind")]
    pub target: NpcSpellTargetKindDef,
    #[serde(default)]
    pub conditions: Vec<NpcSpellConditionDef>,
}

fn default_target_kind() -> NpcSpellTargetKindDef {
    NpcSpellTargetKindDef::Target
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NpcSpellTargetKindDef {
    Target,
    TargetTile,
    #[serde(rename = "self")]
    SelfCast,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NpcSpellConditionDef {
    TargetWithinRange(i32),
    TargetVisible(bool),
    TargetHpBelowFraction(f32),
    SelfHpBelowFraction(f32),
    TargetWithoutEffect(EffectKind),
    SelfWithoutEffect(EffectKind),
}

impl SpellcastingDef {
    /// Build the runtime component. `last_cast_at` is initialized to
    /// `NEG_INFINITY` so every spell is castable on the NPC's first turn.
    pub fn to_component(&self) -> SpellcastingProfile {
        SpellcastingProfile {
            spells: self
                .spells
                .iter()
                .map(|entry| NpcSpellEntry {
                    spell_id: entry.spell_id.clone(),
                    cooldown_seconds: entry.cooldown_seconds.max(0.0),
                    last_cast_at: f32::NEG_INFINITY,
                    target_kind: match entry.target {
                        NpcSpellTargetKindDef::Target => NpcSpellTargetKind::Target,
                        NpcSpellTargetKindDef::TargetTile => NpcSpellTargetKind::TargetTile,
                        NpcSpellTargetKindDef::SelfCast => NpcSpellTargetKind::SelfCast,
                    },
                    conditions: entry
                        .conditions
                        .iter()
                        .map(|c| match *c {
                            NpcSpellConditionDef::TargetWithinRange(n) => {
                                NpcSpellCondition::TargetWithinRange(n)
                            }
                            NpcSpellConditionDef::TargetVisible(_) => {
                                NpcSpellCondition::TargetVisible
                            }
                            NpcSpellConditionDef::TargetHpBelowFraction(f) => {
                                NpcSpellCondition::TargetHpBelowFraction(f)
                            }
                            NpcSpellConditionDef::SelfHpBelowFraction(f) => {
                                NpcSpellCondition::SelfHpBelowFraction(f)
                            }
                            NpcSpellConditionDef::TargetWithoutEffect(k) => {
                                NpcSpellCondition::TargetWithoutEffect(k)
                            }
                            NpcSpellConditionDef::SelfWithoutEffect(k) => {
                                NpcSpellCondition::SelfWithoutEffect(k)
                            }
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_block_parses() {
        let yaml = r#"
spells:
  - spell_id: goblin_heal
    cooldown_seconds: 25.0
    target: self
    conditions:
      - !self_hp_below_fraction 0.4
  - spell_id: sleep
    cooldown_seconds: 18.0
    target: target
    conditions:
      - !target_within_range 4
      - !target_visible true
      - !target_without_effect sleep
"#;
        let def: SpellcastingDef = serde_yaml::from_str(yaml).unwrap();
        let profile = def.to_component();
        assert_eq!(profile.spells.len(), 2);
        assert_eq!(profile.spells[0].spell_id, "goblin_heal");
        assert!(matches!(
            profile.spells[0].target_kind,
            NpcSpellTargetKind::SelfCast
        ));
        assert_eq!(profile.spells[0].conditions.len(), 1);
        assert!(matches!(
            profile.spells[1].target_kind,
            NpcSpellTargetKind::Target
        ));
        // Newly-loaded entries are off-cooldown so the first turn can cast.
        assert_eq!(profile.spells[0].last_cast_at, f32::NEG_INFINITY);
    }
}
