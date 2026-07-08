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
    /// Per-side door ids matching the directional wall slabs. When set, the
    /// door-swap tool picks the variant for the side of the clicked wall and
    /// refuses corners; `default_door` remains the fallback for presets
    /// without side variants.
    #[serde(default)]
    pub doors: Option<DoorSlots>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct DoorSlots {
    pub north: String,
    pub south: String,
    pub east: String,
    pub west: String,
}

impl DoorSlots {
    pub fn all_door_ids(&self) -> impl Iterator<Item = &str> {
        [
            self.north.as_str(),
            self.south.as_str(),
            self.east.as_str(),
            self.west.as_str(),
        ]
        .into_iter()
    }
}

impl BuildingPreset {
    /// The door id to swap in for a clicked wall, or `None` if that wall may
    /// not take a door. With per-side `doors` configured, only the four
    /// straight sides map (corners are never swappable — a corner door has
    /// no sensible orientation). Without `doors`, any preset wall falls back
    /// to `default_door` (legacy behavior, corners included). If two sides
    /// share a wall id, precedence is north, south, east, west.
    pub fn door_for_wall(&self, wall_type_id: &str) -> Option<String> {
        let Some(doors) = &self.doors else {
            return self.default_door.clone();
        };
        let walls = &self.walls;
        let door = if wall_type_id == walls.north {
            &doors.north
        } else if wall_type_id == walls.south {
            &doors.south
        } else if wall_type_id == walls.east {
            &doors.east
        } else if wall_type_id == walls.west {
            &doors.west
        } else {
            return None;
        };
        Some(door.clone())
    }
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
            let preset =
                serde_yaml::from_str::<BuildingPreset>(&asset.contents).unwrap_or_else(|error| {
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
            if let Some(doors) = preset.doors.as_ref() {
                for door_id in doors.all_door_ids() {
                    assert!(
                        objects.get(door_id).is_some(),
                        "building preset `{id}` references unknown door object `{door_id}`",
                    );
                }
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
  north: wall_n
  south: wall_s
  east: wall_e
  west: wall_w
default_floor: cobblestone
default_door: wooden_door
"#,
        );
        assert_eq!(p.id, "stone");
        assert_eq!(p.walls.north, "wall_n");
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
        assert_eq!(stone.walls.north, "wall_n");
        assert_eq!(stone.walls.east, "wall_e");
        assert_eq!(stone.default_door.as_deref(), Some("wooden_door"));
        let doors = stone.doors.as_ref().expect("stone preset has door slots");
        assert_eq!(doors.north, "wooden_door_n");
        assert_eq!(doors.west, "wooden_door_w");
    }

    fn preset_with_doors() -> BuildingPreset {
        preset(
            r#"
id: stone
name: Stone Building
walls:
  north: wall_n
  south: wall_s
  east: wall_e
  west: wall_w
  corner_nw: wall_corner_nw
default_door: wooden_door
doors:
  north: wooden_door_n
  south: wooden_door_s
  east: wooden_door_e
  west: wooden_door_w
"#,
        )
    }

    #[test]
    fn door_for_wall_picks_side_variant() {
        let p = preset_with_doors();
        assert_eq!(p.door_for_wall("wall_n").as_deref(), Some("wooden_door_n"));
        assert_eq!(p.door_for_wall("wall_s").as_deref(), Some("wooden_door_s"));
        assert_eq!(p.door_for_wall("wall_e").as_deref(), Some("wooden_door_e"));
        assert_eq!(p.door_for_wall("wall_w").as_deref(), Some("wooden_door_w"));
    }

    #[test]
    fn door_for_wall_rejects_corners_when_doors_configured() {
        let p = preset_with_doors();
        assert_eq!(p.door_for_wall("wall_corner_nw"), None);
        assert_eq!(p.door_for_wall("not_a_wall"), None);
    }

    #[test]
    fn door_for_wall_falls_back_to_default_door_without_slots() {
        let mut p = preset_with_doors();
        p.doors = None;
        // Legacy behavior: any wall (corners included) takes the default door.
        assert_eq!(p.door_for_wall("wall_n").as_deref(), Some("wooden_door"));
        assert_eq!(
            p.door_for_wall("wall_corner_nw").as_deref(),
            Some("wooden_door")
        );
    }
}
