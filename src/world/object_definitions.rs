use std::collections::HashMap;
use std::fs;
use std::path::Path;

use bevy::prelude::*;
use serde::Deserialize;

const OBJECT_DEFINITIONS_PATH: &str = "assets/overworld_objects";

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct OverworldObjectDefinition {
    pub name: String,
    pub description: String,
    pub colliding: bool,
    pub render: RenderMetadata,
    #[serde(default)]
    pub sound_paths: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct RenderMetadata {
    pub z_index: f32,
    pub debug_color: [u8; 3],
    pub debug_size: f32,
    #[serde(default)]
    pub sprite_path: Option<String>,
}

impl OverworldObjectDefinition {
    pub fn debug_color(&self) -> Color {
        Color::srgb_u8(
            self.render.debug_color[0],
            self.render.debug_color[1],
            self.render.debug_color[2],
        )
    }
}

#[derive(Resource, Default)]
pub struct OverworldObjectDefinitions {
    definitions: HashMap<String, OverworldObjectDefinition>,
}

impl OverworldObjectDefinitions {
    pub fn load_from_disk() -> Self {
        let base_path = Path::new(OBJECT_DEFINITIONS_PATH);
        let entries = fs::read_dir(base_path).unwrap_or_else(|error| {
            panic!(
                "Failed to read overworld object definitions from {}: {error}",
                base_path.display()
            )
        });

        let mut definitions = HashMap::new();

        for entry in entries {
            let entry = entry.expect("Failed to read overworld object directory entry");
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            let Some(directory_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let metadata_path = path.join("metadata.yaml");
            let metadata_yaml = fs::read_to_string(&metadata_path).unwrap_or_else(|error| {
                panic!(
                    "Failed to read overworld object metadata {}: {error}",
                    metadata_path.display()
                )
            });
            let definition = serde_yaml::from_str::<OverworldObjectDefinition>(&metadata_yaml)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse overworld object metadata {}: {error}",
                        metadata_path.display()
                    )
                });

            definitions.insert(directory_name.to_owned(), definition);
        }

        Self { definitions }
    }

    pub fn get(&self, id: &str) -> Option<&OverworldObjectDefinition> {
        self.definitions.get(id)
    }
}
