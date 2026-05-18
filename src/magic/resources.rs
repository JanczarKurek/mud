use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::discover_yaml_assets;
use crate::combat::damage_type::DamageType;
use crate::player::classes::Class;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpellDefinition {
    pub name: String,
    pub incantation: String,
    pub mana_cost: f32,
    pub targeting: SpellTargeting,
    #[serde(default)]
    pub range_tiles: i32,
    /// Classes permitted to cast this spell directly (via a memorized-spell
    /// path that does not exist yet — Phase E). Empty = any class. Scrolls
    /// bypass this gate; see `check_caster_eligibility` in `game::systems`.
    #[serde(default)]
    pub class_access: Vec<Class>,
    /// Minimum caster level. `0` = anyone. Enforced on every cast path,
    /// including scrolls.
    #[serde(default)]
    pub min_caster_level: u32,
    #[serde(default)]
    pub effects: SpellEffects,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpellEffects {
    #[serde(default)]
    pub damage: f32,
    /// Damage type for `damage > 0` spells. Defaults to `Arcane` when omitted
    /// — see `effective_damage_type`.
    #[serde(default)]
    pub damage_type: Option<DamageType>,
    #[serde(default)]
    pub restore_health: f32,
    #[serde(default)]
    pub restore_mana: f32,
    /// Timed buffs applied to the caster.
    #[serde(default)]
    pub buffs_self: Vec<EffectSpec>,
    /// Timed debuffs applied to the targeted NPC. Ignored for untargeted casts.
    #[serde(default)]
    pub buffs_target: Vec<EffectSpec>,
    /// Effect kinds to remove from the caster after other effects apply.
    /// Drives Cleric "Restore" clearing Slow/Sleep on self.
    #[serde(default)]
    pub clears_self: Vec<EffectKind>,
    /// Spawn a transient world object at the cast location.
    #[serde(default)]
    pub spawns_object: Option<SpawnObjectSpec>,
    /// Deal `damage` to every entity within `aoe.radius_tiles` Chebyshev
    /// distance of the target tile. Only meaningful for tile-target spells.
    #[serde(default)]
    pub aoe: Option<AoeSpec>,
    /// VFX definition id played on the caster at cast time. `None` falls back
    /// to `"cast_flash"` in the trigger code.
    #[serde(default)]
    pub vfx_on_cast: Option<String>,
    /// VFX definition id played on the target on a targeted hit. `None` falls
    /// back to `"hit_flash"` for damaging spells; healing spells should
    /// override with `"heal_sparkle"`.
    #[serde(default)]
    pub vfx_on_target_hit: Option<String>,
}

impl SpellEffects {
    /// Resolve the damage type, defaulting to `Arcane` when unspecified.
    /// Only meaningful for `damage > 0` spells.
    pub fn effective_damage_type(&self) -> DamageType {
        self.damage_type.unwrap_or(DamageType::Arcane)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct EffectSpec {
    pub kind: EffectKind,
    pub magnitude: f32,
    pub seconds: f32,
    /// Optional second parameter. Currently only `Chill` reads it, as the
    /// slow multiplier paired with the DOT magnitude.
    #[serde(default)]
    pub secondary_magnitude: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpawnObjectSpec {
    pub type_id: String,
    pub lifetime_seconds: f32,
    /// How many tiles to spawn and where, relative to the cast target tile.
    #[serde(default)]
    pub pattern: SpawnPattern,
    /// When true, every spawned object inherits a `HazardOwner(caster_id)`
    /// component so damage and DoTs it produces credit the caster via
    /// `DamageSource::OwnedByPlayer`.
    #[serde(default)]
    pub attribute_to_caster: bool,
}

/// Tile pattern for `SpawnObjectSpec`. `Single` is the default and matches
/// pre-existing behavior (one entity at the target tile).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpawnPattern {
    #[default]
    Single,
    /// Three tiles in a straight line perpendicular to the caster→target
    /// axis, centered on the target tile.
    #[serde(rename = "perpendicular_line_3")]
    PerpendicularLine3,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AoeSpec {
    /// Chebyshev radius around the target tile. `0` hits only the target tile.
    pub radius_tiles: i32,
    /// VFX definition id (under `assets/vfx/`) played once on **every** tile
    /// in the AoE — not just on entities hit. Use for explosion-style spells
    /// where the floor itself should flash. `None` skips the per-tile VFX
    /// (only hit entities get `vfx_on_target_hit`).
    #[serde(default)]
    pub vfx_on_tile: Option<String>,
}

/// Kinds of timed magical effects tracked by `MagicEffects`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Player's personal light halo grows. Magnitude = tile radius.
    Glimmer,
    /// Player moves faster. Magnitude = step interval multiplier (e.g. 0.7).
    Haste,
    /// Caster gains tracked AC bonus. Magnitude = AC bonus (combat math reads
    /// this once §7 lands — no-op today).
    Shield,
    /// Caster gains tracked to-hit bonus. Magnitude = to-hit bonus (no-op
    /// today, hooked for Phase B combat math).
    Bless,
    /// Target NPC's roaming step interval lengthens. Magnitude = multiplier
    /// (e.g. 2.0 doubles the interval = half speed).
    Slow,
    /// Target NPC is asleep — its AI tick is skipped. Magnitude unused
    /// (presence is what matters). Cleared on damage by `resolve_battle_turn`.
    Sleep,
    /// Target cannot move or cast spells. Magnitude unused. Unlike Sleep,
    /// damage does *not* clear Paralyze — it only expires on its timer.
    Paralyze,
    /// DOT (cold damage) plus slow movement. Magnitude = damage per tick
    /// (1s cadence); `secondary_magnitude` = NPC step interval multiplier
    /// (Some(2.0) doubles the interval). When omitted, the slow component is
    /// a no-op and Chill behaves as pure cold DOT.
    Chill,
    /// DOT (fire damage). Magnitude = damage per tick (1s cadence).
    Burning,
    /// DOT (poison damage). Magnitude = damage per tick (1s cadence).
    Poisoned,
    /// Player's movement commands are randomly rotated by ±45° to an adjacent
    /// direction. Magnitude = deviation probability in `[0, 1]` (e.g. 0.3 =
    /// 30% chance to fumble each step). NPCs ignore Drunk for now.
    Drunk,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpellTargeting {
    /// Player picks an entity. Range is checked against the entity's tile.
    Targeted,
    /// Player picks a tile (entity optional). Used for AoE and patterned
    /// summons like firewall.
    TargetedTile,
    /// No picker — casts on the caster's tile / self.
    Untargeted,
}

#[derive(Resource, Default)]
pub struct SpellDefinitions {
    definitions: HashMap<String, SpellDefinition>,
}

impl SpellDefinitions {
    pub fn load_from_disk() -> Self {
        let mut definitions = HashMap::new();
        for asset in discover_yaml_assets("spells", "spell definition") {
            let definition = serde_yaml::from_str::<SpellDefinition>(&asset.contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse spell definition {}: {error}",
                        asset.path.display()
                    )
                });
            definitions.insert(asset.id, definition);
        }
        Self { definitions }
    }

    pub fn get(&self, id: &str) -> Option<&SpellDefinition> {
        self.definitions.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_minimal_spell_parses() {
        let yaml = r#"
name: Spark Bolt
incantation: Exori Vis
mana_cost: 12.0
targeting: targeted
range_tiles: 5
effects:
  damage: 18.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.name, "Spark Bolt");
        assert_eq!(spell.range_tiles, 5);
        assert_eq!(spell.effects.damage, 18.0);
        assert!(spell.class_access.is_empty());
        assert_eq!(spell.min_caster_level, 0);
        assert!(spell.effects.buffs_self.is_empty());
    }

    #[test]
    fn full_schema_round_trip() {
        let yaml = r#"
name: Frost Lance
incantation: Frigus Hasta
mana_cost: 16.0
targeting: targeted
range_tiles: 6
class_access: [Wizard]
min_caster_level: 3
effects:
  damage: 7.0
  damage_type: frost
  buffs_target:
    - kind: slow
      magnitude: 2.0
      seconds: 3.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.class_access, vec![Class::Wizard]);
        assert_eq!(spell.min_caster_level, 3);
        assert_eq!(spell.effects.buffs_target.len(), 1);
        assert_eq!(spell.effects.buffs_target[0].kind, EffectKind::Slow);
        assert_eq!(spell.effects.buffs_target[0].magnitude, 2.0);
        assert_eq!(spell.effects.damage_type, Some(DamageType::Frost));
        assert_eq!(spell.effects.effective_damage_type(), DamageType::Frost);
    }

    #[test]
    fn effects_without_damage_type_default_to_arcane() {
        let yaml = r#"
name: Spark Bolt
incantation: Exori Vis
mana_cost: 12.0
targeting: targeted
range_tiles: 5
effects:
  damage: 18.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.effects.damage_type, None);
        assert_eq!(spell.effects.effective_damage_type(), DamageType::Arcane);
    }

    #[test]
    fn all_authored_spells_parse_from_disk() {
        // Sanity-check that every YAML file in `assets/spells/` parses with
        // the current schema. Catches typos in newly-authored spells.
        let defs = SpellDefinitions::load_from_disk();
        let ids: Vec<&str> = defs.ids().collect();
        assert!(
            ids.contains(&"spark_bolt") && ids.contains(&"lesser_heal"),
            "expected baseline spells; got {ids:?}"
        );
        for new_id in [
            "glimmer",
            "light",
            "magic_dart",
            "frost_lance",
            "sleep",
            "shield",
            "slow",
            "cure_wounds",
            "restore",
            "bless",
            "swiftness",
            "immolation",
            "frost_bolt",
            "venom",
            "paralysis",
            "befuddle",
            "fireball",
            "firewall",
        ] {
            assert!(
                ids.contains(&new_id),
                "missing newly-authored spell {new_id}; got {ids:?}"
            );
        }
    }

    #[test]
    fn untargeted_self_buff_with_spawn_object() {
        let yaml = r#"
name: Glimmer
incantation: Lux Minima
mana_cost: 2.0
targeting: untargeted
class_access: [Wizard, Cleric]
min_caster_level: 1
effects:
  buffs_self:
    - kind: glimmer
      magnitude: 4.0
      seconds: 600.0
  spawns_object:
    type_id: magic_light
    lifetime_seconds: 1800.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.effects.buffs_self.len(), 1);
        let obj = spell.effects.spawns_object.as_ref().unwrap();
        assert_eq!(obj.type_id, "magic_light");
        assert_eq!(obj.lifetime_seconds, 1800.0);
        assert_eq!(obj.pattern, SpawnPattern::Single);
        assert!(!obj.attribute_to_caster);
    }

    #[test]
    fn firewall_pattern_with_owner_attribution() {
        let yaml = r#"
name: Firewall
incantation: Adori Flam
mana_cost: 28.0
targeting: targeted_tile
range_tiles: 5
effects:
  spawns_object:
    type_id: blazing_fire
    lifetime_seconds: 10.0
    pattern: perpendicular_line_3
    attribute_to_caster: true
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.targeting, SpellTargeting::TargetedTile);
        let obj = spell.effects.spawns_object.as_ref().unwrap();
        assert_eq!(obj.pattern, SpawnPattern::PerpendicularLine3);
        assert!(obj.attribute_to_caster);
    }

    #[test]
    fn aoe_field_round_trip() {
        let yaml = r#"
name: Fireball
incantation: Exori Flam
mana_cost: 22.0
targeting: targeted_tile
range_tiles: 6
effects:
  damage: 14.0
  damage_type: fire
  aoe:
    radius_tiles: 1
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        let aoe = spell.effects.aoe.as_ref().unwrap();
        assert_eq!(aoe.radius_tiles, 1);
        assert_eq!(spell.effects.effective_damage_type(), DamageType::Fire);
    }
}
