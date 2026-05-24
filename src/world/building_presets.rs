//! Authoring-time bundles of wall/floor/door ids used by the editor's building
//! tool. A `BuildingPreset` says "for a Stone Building, top edges are `wall`,
//! side edges are `side_wall`, the inside is `cobblestone`, and the default
//! door is `wooden_door`". The runtime game never reads these — once the
//! editor stamps a building, what lands on the map is plain wall + floor +
//! door objects, no preset reference.
//!
//! Loading mirrors `SpellDefinitions::load_from_disk`: scan
//! `assets/building_presets/*.yaml` via `discover_yaml_assets`, deserialize
//! each file as a `BuildingPreset`, panic on a parse error. Validation
//! against the loaded object / floor definitions happens once, at startup,
//! from the editor plugin — same posture as `RecipeDefinitions::validate_against`.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::discover_yaml_assets;
use crate::world::floor_definitions::{FloorTilesetDefinitions, FloorTypeId};
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct BuildingPreset {
    pub id: String,
    pub name: String,
    pub walls: WallSlots,
    #[serde(default)]
    pub default_floor: Option<FloorTypeId>,
    #[serde(default)]
    pub default_door: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct WallSlots {
    pub north: String,
    pub south: String,
    pub east: String,
    pub west: String,
    #[serde(default)]
    pub corner_nw: Option<String>,
    #[serde(default)]
    pub corner_ne: Option<String>,
    #[serde(default)]
    pub corner_sw: Option<String>,
    #[serde(default)]
    pub corner_se: Option<String>,
}

impl WallSlots {
    /// Every wall `type_id` referenced by this preset, including optional
    /// corner overrides. Order is unspecified; callers should not depend on it.
    pub fn all_wall_ids(&self) -> impl Iterator<Item = &str> {
        [
            Some(self.north.as_str()),
            Some(self.south.as_str()),
            Some(self.east.as_str()),
            Some(self.west.as_str()),
            self.corner_nw.as_deref(),
            self.corner_ne.as_deref(),
            self.corner_sw.as_deref(),
            self.corner_se.as_deref(),
        ]
        .into_iter()
        .flatten()
    }
}

#[derive(Resource, Default)]
pub struct BuildingPresets {
    by_id: BTreeMap<String, BuildingPreset>,
}

impl BuildingPresets {
    pub fn load_from_disk() -> Self {
        let mut by_id = BTreeMap::new();
        for asset in discover_yaml_assets("building_presets", "building preset") {
            let preset = serde_yaml::from_str::<BuildingPreset>(&asset.contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse building preset {}: {error}",
                        asset.path.display()
                    )
                });
            assert_eq!(
                preset.id, asset.id,
                "building preset id `{}` does not match file stem `{}`",
                preset.id, asset.id
            );
            by_id.insert(asset.id, preset);
        }
        Self { by_id }
    }

    /// Cross-check every referenced object and floor id. Panics on a typo —
    /// matches the spell/recipe loader posture: a bad authoring file should
    /// stop the world rather than break silently when the editor pulls a
    /// preset off the shelf.
    pub fn validate_against(
        &self,
        objects: &OverworldObjectDefinitions,
        floors: &FloorTilesetDefinitions,
    ) {
        for (id, preset) in &self.by_id {
            for wall_id in preset.walls.all_wall_ids() {
                assert!(
                    objects.get(wall_id).is_some(),
                    "building preset `{id}` references unknown wall object `{wall_id}`",
                );
            }
            if let Some(door) = preset.default_door.as_ref() {
                assert!(
                    objects.get(door).is_some(),
                    "building preset `{id}` references unknown door object `{door}`",
                );
            }
            if let Some(floor) = preset.default_floor.as_ref() {
                assert!(
                    floors.contains(floor),
                    "building preset `{id}` references unknown floor `{floor}`",
                );
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&BuildingPreset> {
        self.by_id.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &BuildingPreset)> {
        self.by_id.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(yaml: &str) -> BuildingPreset {
        serde_yaml::from_str(yaml).expect("preset parses")
    }

    #[test]
    fn minimal_preset_parses() {
        let p = preset(
            r#"
id: stone
name: Stone Building
walls:
  north: wall
  south: wall
  east: side_wall
  west: side_wall
default_floor: cobblestone
default_door: wooden_door
"#,
        );
        assert_eq!(p.id, "stone");
        assert_eq!(p.walls.north, "wall");
        assert!(p.walls.corner_ne.is_none());
        assert_eq!(p.default_floor.as_deref(), Some("cobblestone"));
        assert_eq!(p.default_door.as_deref(), Some("wooden_door"));
    }

    #[test]
    fn preset_with_corners_parses() {
        let p = preset(
            r#"
id: castle
name: Castle
walls:
  north: castle_wall_h
  south: castle_wall_h
  east: castle_wall_v
  west: castle_wall_v
  corner_nw: castle_corner_nw
  corner_ne: castle_corner_ne
  corner_sw: castle_corner_sw
  corner_se: castle_corner_se
"#,
        );
        assert_eq!(p.walls.corner_nw.as_deref(), Some("castle_corner_nw"));
        let collected: Vec<&str> = p.walls.all_wall_ids().collect();
        assert!(collected.contains(&"castle_corner_ne"));
        assert_eq!(collected.len(), 8);
    }

    #[test]
    fn stone_preset_loads_from_disk() {
        // Assumes the working directory is the repo root (cargo test default).
        let presets = BuildingPresets::load_from_disk();
        let stone = presets.get("stone").expect("stone preset exists on disk");
        assert_eq!(stone.walls.north, "wall");
        assert_eq!(stone.walls.east, "side_wall");
        assert_eq!(stone.default_door.as_deref(), Some("wooden_door"));
    }
}
