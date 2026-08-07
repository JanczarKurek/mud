//! Data-driven debug character presets.
//!
//! Each `assets/debug_characters/*.yaml` describes one ready-made character
//! (name, class, level, attributes, optional loadout) that debug mode
//! auto-creates on the Character Select screen, so higher-level content can be
//! tested without grinding. Loading mirrors `BuildingPresets`: parse every
//! file into a map keyed by file stem and panic on any authoring error at
//! startup.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Deserialize;

use crate::assets::discover_yaml_assets;
use crate::player::classes::Class;
use crate::player::components::{validate_point_buy, AttributeSet, PlayerAppearance};
use crate::player::loadout::{Loadouts, STARTER_LOADOUT_ID};
use crate::player::progression::LEVEL_CAP;

fn default_loadout_id() -> String {
    STARTER_LOADOUT_ID.to_owned()
}

/// One authored debug character. `attributes` must satisfy the same point-buy
/// rules as the Character Create form — presets are legal builds, just
/// pre-leveled.
#[derive(Debug, Clone, Deserialize)]
pub struct DebugCharacterPreset {
    pub name: String,
    pub class: Class,
    pub level: u32,
    pub attributes: AttributeSet,
    #[serde(default)]
    pub appearance: PlayerAppearance,
    /// File stem of a loadout under `assets/loadouts/` to seed the inventory.
    #[serde(default = "default_loadout_id")]
    pub loadout: String,
}

/// All debug character presets, keyed by file stem.
#[derive(Resource, Default)]
pub struct DebugCharacterPresets {
    by_id: BTreeMap<String, DebugCharacterPreset>,
}

impl DebugCharacterPresets {
    /// Load every `debug_characters/*.yaml`. Panics on parse errors, an
    /// out-of-range level, or attributes that fail point-buy — a bad authoring
    /// file should stop the world at startup rather than half-create
    /// characters later.
    pub fn load_from_disk() -> Self {
        let mut by_id = BTreeMap::new();
        for asset in discover_yaml_assets("debug_characters", "debug character preset") {
            let preset = serde_yaml::from_str::<DebugCharacterPreset>(&asset.contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse debug character preset {}: {error}",
                        asset.path.display()
                    )
                });
            assert!(
                (1..=LEVEL_CAP).contains(&preset.level),
                "debug character preset `{}`: level {} outside 1..={LEVEL_CAP}",
                asset.id,
                preset.level
            );
            if let Err(error) = validate_point_buy(&preset.attributes) {
                panic!(
                    "debug character preset `{}`: attributes fail point-buy: {error}",
                    asset.id
                );
            }
            by_id.insert(asset.id, preset);
        }
        Self { by_id }
    }

    /// Cross-check every referenced loadout id. Panics on a typo — same
    /// posture as `BuildingPresets::validate_against`.
    pub fn validate_against(&self, loadouts: &Loadouts) {
        for (id, preset) in &self.by_id {
            assert!(
                loadouts.get(&preset.loadout).is_some(),
                "debug character preset `{id}` references unknown loadout `{}`",
                preset.loadout
            );
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &DebugCharacterPreset)> {
        self.by_id.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_presets_load_and_validate() {
        // The shipped `assets/debug_characters/*.yaml` must always parse,
        // pass point-buy, and reference existing loadouts.
        let presets = DebugCharacterPresets::load_from_disk();
        assert!(
            presets.iter().count() >= 4,
            "expected the four bundled debug presets"
        );
        presets.validate_against(&Loadouts::load_from_disk());
    }

    #[test]
    fn preset_kits_are_carryable_by_their_builds() {
        use crate::player::components::{Inventory, MaxCarryWeight};
        use crate::world::object_definitions::OverworldObjectDefinitions;

        // A kit heavier than the build's soft carry cap spawns the character
        // Encumbered (2× movement cooldown); one within ~2 kg of the cap gets
        // encumbered by the first loot pickup. Both defeat the point of a
        // ready-to-play debug character, so demand real headroom.
        let presets = DebugCharacterPresets::load_from_disk();
        let loadouts = Loadouts::load_from_disk();
        let objects = OverworldObjectDefinitions::load_from_disk();
        for (id, preset) in presets.iter() {
            let mut inventory = Inventory::default();
            loadouts
                .get(&preset.loadout)
                .unwrap()
                .apply_to(&mut inventory);
            let weight = inventory.total_weight(&objects);
            let soft_cap = MaxCarryWeight::from_strength(preset.attributes.strength).soft_cap;
            assert!(
                weight <= soft_cap - 2.0,
                "preset `{id}` kit weighs {weight:.1} kg against a {soft_cap:.1} kg \
                 soft carry cap (STR {}) — the character would spawn encumbered \
                 or one pickup away from it",
                preset.attributes.strength
            );
        }
    }

    #[test]
    fn minimal_preset_defaults() {
        let yaml = r#"
name: Minimal
class: Wizard
level: 3
attributes:
  strength: 12
  agility: 12
  constitution: 12
  willpower: 12
  charisma: 12
  focus: 12
"#;
        let preset: DebugCharacterPreset = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(preset.loadout, STARTER_LOADOUT_ID);
        assert_eq!(preset.appearance, PlayerAppearance::default());
        assert_eq!(preset.class, Class::Wizard);
        assert_eq!(preset.level, 3);
    }
}
