use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::discover_yaml_assets;
use crate::world::components::TilePosition;

const DEFAULT_BOOTSTRAP_SPACE_ID: &str = "overworld";

pub type ObjectProperties = HashMap<String, String>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpacePermanence {
    Persistent,
    Ephemeral,
}

impl SpacePermanence {
    pub const fn is_persistent(self) -> bool {
        matches!(self, Self::Persistent)
    }
}

#[derive(Clone, Debug, Deserialize, Resource)]
pub struct SpaceDefinitions {
    pub bootstrap_space_id: String,
    pub spaces: HashMap<String, SpaceDefinition>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpaceDefinition {
    pub authored_id: String,
    pub width: i32,
    pub height: i32,
    #[serde(alias = "fill_object")]
    pub fill_object_type: String,
    #[serde(default = "default_persistent_permanence")]
    pub permanence: SpacePermanence,
    #[serde(default)]
    pub portals: Vec<PortalDefinition>,
    #[serde(default)]
    pub objects: Vec<MapObjectEntry>,
    /// Single-character keys mapping to object type IDs for use in `tiles`.
    #[serde(default)]
    pub legend: HashMap<String, String>,
    /// ASCII grid of tiles, row-major with y=0 at top. Each character maps
    /// via `legend`; unmapped characters are skipped (fill_object_type applies).
    #[serde(default)]
    pub tiles: Option<String>,
    #[serde(skip)]
    pub resolved_objects: Vec<MapObjectInstance>,
    #[serde(skip)]
    object_indices: HashMap<u64, usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct PortalDefinition {
    pub id: String,
    pub source: TileCoordinate,
    pub destination_space_id: String,
    pub destination_tile: TileCoordinate,
    #[serde(default)]
    pub destination_permanence: Option<SpacePermanence>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct MapObjectInstance {
    pub id: u64,
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    #[serde(default)]
    pub placement: Option<TileCoordinate>,
    #[serde(default)]
    pub contents: Vec<u64>,
    #[serde(default)]
    pub behavior: Option<MapBehavior>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum MapObjectEntry {
    Explicit(MapObjectInstance),
    Anonymous(AnonymousObjectPlacements),
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AnonymousObjectPlacements {
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    pub placement: Vec<TileCoordinate>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapBehavior {
    Roam {
        step_interval_seconds: f32,
        bounds: TileRectangle,
    },
    RoamAndChase {
        step_interval_seconds: f32,
        bounds: TileRectangle,
        detect_distance_tiles: i32,
        disengage_distance_tiles: i32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct TileCoordinate {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct TileRectangle {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl TileCoordinate {
    pub const fn to_tile_position(self) -> TilePosition {
        TilePosition::new(self.x, self.y)
    }
}

impl SpaceDefinitions {
    pub fn load_from_disk() -> Self {
        let mut spaces = HashMap::new();

        for asset in discover_yaml_assets("maps", "map layout") {
            let mut definition: SpaceDefinition = serde_yaml::from_str(&asset.contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse map layout {}: {error}",
                        asset.path.display()
                    )
                });

            if definition.authored_id.trim().is_empty() {
                definition.authored_id = asset.id.clone();
            }

            definition.validate();
            spaces.insert(definition.authored_id.clone(), definition);
            info!("loaded map layout {}", asset.path.display());
        }

        if !spaces.is_empty() {
            assert!(
                spaces.contains_key(DEFAULT_BOOTSTRAP_SPACE_ID),
                "Missing bootstrap space definition '{}'",
                DEFAULT_BOOTSTRAP_SPACE_ID
            );
        }

        for definition in spaces.values() {
            for portal in &definition.portals {
                assert!(
                    spaces.contains_key(&portal.destination_space_id),
                    "Space '{}' portal '{}' points to missing destination '{}'",
                    definition.authored_id,
                    portal.id,
                    portal.destination_space_id
                );
            }
        }

        Self {
            bootstrap_space_id: DEFAULT_BOOTSTRAP_SPACE_ID.to_owned(),
            spaces,
        }
    }

    pub fn bootstrap_space(&self) -> Option<&SpaceDefinition> {
        self.get(&self.bootstrap_space_id)
    }

    pub fn get(&self, authored_id: &str) -> Option<&SpaceDefinition> {
        self.spaces.get(authored_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpaceDefinition> {
        self.spaces.values()
    }

    /// Insert or replace a space definition (e.g. after editor Save As or New Map).
    pub fn insert_or_replace(&mut self, def: SpaceDefinition) {
        self.spaces.insert(def.authored_id.clone(), def);
    }

    /// Load a single map YAML from `assets/maps/{authored_id}.yaml` and insert it.
    /// Returns `true` if successful. Skips validation assertions for portal destinations.
    pub fn load_single_from_disk(&mut self, authored_id: &str) -> bool {
        let path = format!("assets/maps/{authored_id}.yaml");
        let Ok(yaml) = std::fs::read_to_string(&path) else {
            return false;
        };
        let Ok(mut def) = serde_yaml::from_str::<SpaceDefinition>(&yaml) else {
            return false;
        };
        if def.authored_id.trim().is_empty() {
            def.authored_id = authored_id.to_owned();
        }
        def.resolve_objects();
        self.spaces.insert(def.authored_id.clone(), def);
        true
    }
}

impl SpaceDefinition {
    pub fn portal_at(&self, tile_position: TilePosition) -> Option<&PortalDefinition> {
        self.portals
            .iter()
            .find(|portal| portal.source.to_tile_position() == tile_position)
    }

    pub fn is_contained(&self, object_id: u64) -> bool {
        self.resolved_objects
            .iter()
            .any(|object| object.contents.contains(&object_id))
    }

    /// Expand tile grid + anonymous objects and build internal indices.
    /// Call this when constructing a SpaceDefinition outside of `load_from_disk`.
    pub fn resolve_objects(&mut self) {
        self.expand_tile_grid();
        self.expand_anonymous_objects();
        let mut object_indices = HashMap::new();
        for (index, object) in self.resolved_objects.iter().enumerate() {
            object_indices.insert(object.id, index);
        }
        self.object_indices = object_indices;
    }

    /// Create a blank space definition (no objects, no portals).
    pub fn new_empty(
        authored_id: String,
        width: i32,
        height: i32,
        fill_object_type: String,
    ) -> Self {
        Self {
            authored_id,
            width,
            height,
            fill_object_type,
            permanence: SpacePermanence::Persistent,
            portals: Vec::new(),
            objects: Vec::new(),
            legend: HashMap::new(),
            tiles: None,
            resolved_objects: Vec::new(),
            object_indices: HashMap::new(),
        }
    }

    fn validate(&mut self) {
        self.expand_tile_grid();
        self.expand_anonymous_objects();

        let mut object_indices = HashMap::new();

        for (index, object) in self.resolved_objects.iter().enumerate() {
            let previous = object_indices.insert(object.id, index);
            assert!(
                previous.is_none(),
                "Duplicate object id {} found in space '{}'",
                object.id,
                self.authored_id
            );
        }

        let mut location_counts: HashMap<u64, usize> = HashMap::new();

        for object in &self.resolved_objects {
            if let Some(placement) = object.placement {
                assert!(
                    placement.x >= 0
                        && placement.y >= 0
                        && placement.x < self.width
                        && placement.y < self.height,
                    "Object {} placement is outside space '{}'",
                    object.id,
                    self.authored_id
                );
                *location_counts.entry(object.id).or_default() += 1;
            }
        }

        for object in &self.resolved_objects {
            for contained_id in &object.contents {
                assert!(
                    *contained_id != object.id,
                    "Object {} cannot contain itself in '{}'",
                    object.id,
                    self.authored_id
                );
                assert!(
                    object_indices.contains_key(contained_id),
                    "Object {} references missing contained object id {} in '{}'",
                    object.id,
                    contained_id,
                    self.authored_id
                );
                *location_counts.entry(*contained_id).or_default() += 1;
            }
        }

        for portal in &self.portals {
            assert!(
                portal.source.x >= 0
                    && portal.source.y >= 0
                    && portal.source.x < self.width
                    && portal.source.y < self.height,
                "Portal '{}' source is outside space '{}'",
                portal.id,
                self.authored_id
            );
        }

        for (object_id, count) in location_counts {
            assert!(
                count <= 1,
                "Object {} appears in more than one place in '{}'",
                object_id,
                self.authored_id
            );
        }

        self.object_indices = object_indices;
    }

    fn expand_tile_grid(&mut self) {
        let Some(tiles_str) = self.tiles.clone() else {
            return;
        };

        let lines: Vec<&str> = tiles_str.lines().collect();
        assert!(
            lines.len() == self.height as usize,
            "Space '{}' tiles grid has {} rows but height is {}",
            self.authored_id,
            lines.len(),
            self.height
        );
        for key in self.legend.keys() {
            assert!(
                key.chars().count() == 1,
                "Space '{}' legend key '{}' must be exactly one character",
                self.authored_id,
                key
            );
        }

        let char_map: HashMap<char, &str> = self
            .legend
            .iter()
            .map(|(k, v)| (k.chars().next().unwrap(), v.as_str()))
            .collect();

        let mut type_to_tiles: HashMap<String, Vec<TileCoordinate>> = HashMap::new();
        for (row_idx, line) in lines.iter().enumerate() {
            assert!(
                line.chars().count() == self.width as usize,
                "Space '{}' tiles row {} has {} chars but width is {}",
                self.authored_id,
                row_idx,
                line.chars().count(),
                self.width
            );
            for (col_idx, ch) in line.chars().enumerate() {
                if let Some(&type_id) = char_map.get(&ch) {
                    type_to_tiles
                        .entry(type_id.to_owned())
                        .or_default()
                        .push(TileCoordinate {
                            x: col_idx as i32,
                            y: row_idx as i32,
                        });
                }
            }
        }

        for (type_id, placements) in type_to_tiles {
            self.objects
                .push(MapObjectEntry::Anonymous(AnonymousObjectPlacements {
                    type_id,
                    properties: HashMap::new(),
                    placement: placements,
                }));
        }
    }

    fn expand_anonymous_objects(&mut self) {
        let mut next_generated_id = self
            .objects
            .iter()
            .filter_map(|entry| match entry {
                MapObjectEntry::Explicit(object) => Some(object.id),
                MapObjectEntry::Anonymous(_) => None,
            })
            .max()
            .unwrap_or(0)
            + 1;

        let mut resolved_objects = Vec::new();

        for entry in &self.objects {
            match entry {
                MapObjectEntry::Explicit(object) => resolved_objects.push(object.clone()),
                MapObjectEntry::Anonymous(group) => {
                    for tile in &group.placement {
                        resolved_objects.push(MapObjectInstance {
                            id: next_generated_id,
                            type_id: group.type_id.clone(),
                            properties: group.properties.clone(),
                            placement: Some(*tile),
                            contents: Vec::new(),
                            behavior: None,
                        });
                        next_generated_id += 1;
                    }
                }
            }
        }

        self.resolved_objects = resolved_objects;
    }
}

const fn default_persistent_permanence() -> SpacePermanence {
    SpacePermanence::Persistent
}
