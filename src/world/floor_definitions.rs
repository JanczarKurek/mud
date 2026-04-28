use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::AssetResolver;

pub type FloorTypeId = String;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct FloorTilesetDefinition {
    pub id: FloorTypeId,
    pub name: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_tile_size_px")]
    pub tile_size_px: u32,
    #[serde(default)]
    pub atlas_path: Option<String>,
    pub debug_color: [u8; 3],
    /// Reserved for upper-storey floors; unused at z=0 in this slice.
    #[serde(default)]
    pub occludes_floor_above: bool,
    /// Reserved; ground floor is always walkable.
    #[serde(default = "default_true")]
    pub walkable_surface: bool,
    /// Per-bitmask variant weights. Key = corner-bitmask in `1..=15`
    /// (NW=1, NE=2, SW=4, SE=8). Value = list of positive weights, one per
    /// available variant of that bitmask in the atlas. Variant 0 occupies
    /// rows 0..=3 of the atlas (the base block); variant `i` occupies rows
    /// `4*i..=4*i+3`. Bitmasks omitted from the map have a single variant.
    #[serde(default)]
    pub variants: HashMap<u8, Vec<u32>>,
}

const fn default_tile_size_px() -> u32 {
    16
}

const fn default_true() -> bool {
    true
}

impl FloorTilesetDefinition {
    pub fn debug_color(&self) -> Color {
        Color::srgb_u8(
            self.debug_color[0],
            self.debug_color[1],
            self.debug_color[2],
        )
    }

    pub fn variant_weights(&self, mask: u8) -> &[u32] {
        const SINGLE: &[u32] = &[1];
        self.variants
            .get(&mask)
            .map(|v| v.as_slice())
            .unwrap_or(SINGLE)
    }

    pub fn max_variants(&self) -> usize {
        self.variants
            .values()
            .map(|v| v.len())
            .max()
            .unwrap_or(1)
            .max(1)
    }
}

#[derive(Resource, Default, Clone, Debug)]
pub struct FloorTilesetDefinitions {
    by_id: HashMap<FloorTypeId, FloorTilesetDefinition>,
}

impl FloorTilesetDefinitions {
    pub fn load_from_disk() -> Self {
        let resolver = AssetResolver::new();
        let scan_dirs = resolver.scan_dirs("floors");
        let mut by_id = HashMap::new();

        for scan_dir in &scan_dirs {
            info!(
                "loading floor tileset definitions from {}",
                scan_dir.display()
            );
            let Ok(entries) = fs::read_dir(scan_dir) else {
                continue;
            };
            for entry in entries {
                let entry = entry.expect("Failed to read floor tileset directory entry");
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(directory_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let metadata_path = path.join("metadata.yaml");
                if !metadata_path.is_file() {
                    continue;
                }
                let yaml = fs::read_to_string(&metadata_path).unwrap_or_else(|error| {
                    panic!(
                        "Failed to read floor tileset metadata {}: {error}",
                        metadata_path.display()
                    )
                });
                let mut def: FloorTilesetDefinition =
                    serde_yaml::from_str(&yaml).unwrap_or_else(|error| {
                        panic!(
                            "Failed to parse floor tileset metadata {}: {error}",
                            metadata_path.display()
                        )
                    });
                if def.id.trim().is_empty() {
                    def.id = directory_name.to_owned();
                }
                assert!(
                    def.id == directory_name,
                    "Floor tileset id '{}' does not match directory name '{}'",
                    def.id,
                    directory_name
                );
                assert!(
                    def.tile_size_px > 0,
                    "Floor tileset '{}' has tile_size_px = 0",
                    def.id
                );
                for (mask, weights) in &def.variants {
                    assert!(
                        (1..=15).contains(mask),
                        "Floor tileset '{}': variant key {} out of range 1..=15",
                        def.id,
                        mask
                    );
                    assert!(
                        !weights.is_empty(),
                        "Floor tileset '{}': variant {} has empty weights list",
                        def.id,
                        mask
                    );
                    assert!(
                        weights.iter().all(|w| *w > 0),
                        "Floor tileset '{}': variant {} has a zero weight",
                        def.id,
                        mask
                    );
                }
                info!(
                    "floor tileset '{}': priority={}, atlas={:?}, tile_size_px={}, max_variants={}",
                    def.id,
                    def.priority,
                    def.atlas_path,
                    def.tile_size_px,
                    def.max_variants(),
                );
                by_id.insert(def.id.clone(), def);
            }
        }
        Self { by_id }
    }

    pub fn get(&self, id: &str) -> Option<&FloorTilesetDefinition> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = &FloorTilesetDefinition> {
        self.by_id.values()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }
}
