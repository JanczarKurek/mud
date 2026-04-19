use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::AssetResolver;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpellDefinition {
    pub name: String,
    pub incantation: String,
    pub mana_cost: f32,
    pub targeting: SpellTargeting,
    #[serde(default)]
    pub range_tiles: i32,
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
        let resolver = AssetResolver::new();
        let mut definitions = HashMap::new();

        for scan_dir in resolver.scan_dirs("spells") {
            let Ok(entries) = fs::read_dir(&scan_dir) else {
                continue;
            };

            for entry in entries {
                let entry = entry.expect("Failed to read spell definition entry");
                let path = entry.path();

                if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("yaml")
                {
                    continue;
                }

                let spell_id = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .expect("Spell definition file has invalid name")
                    .to_owned();
                let yaml = fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!(
                        "Failed to read spell definition {}: {error}",
                        path.display()
                    )
                });
                let definition =
                    serde_yaml::from_str::<SpellDefinition>(&yaml).unwrap_or_else(|error| {
                        panic!(
                            "Failed to parse spell definition {}: {error}",
                            path.display()
                        )
                    });
                definitions.insert(spell_id, definition);
            }
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
