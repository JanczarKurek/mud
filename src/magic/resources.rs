use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::discover_yaml_assets;
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct EffectSpec {
    pub kind: EffectKind,
    pub magnitude: f32,
    pub seconds: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpawnObjectSpec {
    pub type_id: String,
    pub lifetime_seconds: f32,
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
    /// (presence is what matters).
    Sleep,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpellTargeting {
    Targeted,
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
    }
}
