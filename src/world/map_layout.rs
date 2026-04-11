use std::collections::HashMap;
use std::fs;
use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::components::TilePosition;

const MAP_LAYOUTS_PATH: &str = "assets/maps";
const DEFAULT_BOOTSTRAP_SPACE_ID: &str = "overworld";

pub type ObjectProperties = HashMap<String, String>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    pub objects: Vec<MapObjectEntry>,
    #[serde(skip)]
    pub resolved_objects: Vec<MapObjectInstance>,
    #[serde(skip)]
    object_indices: HashMap<u64, usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PortalDefinition {
    pub id: String,
    pub source: TileCoordinate,
    pub destination_space_id: String,
    pub destination_tile: TileCoordinate,
    #[serde(default)]
    pub destination_permanence: Option<SpacePermanence>,
}

#[derive(Clone, Debug, Deserialize)]
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
#[serde(untagged)]
pub enum MapObjectEntry {
    Explicit(MapObjectInstance),
    Anonymous(AnonymousObjectPlacements),
}

#[derive(Clone, Debug, Deserialize)]
pub struct AnonymousObjectPlacements {
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    pub placement: Vec<TileCoordinate>,
}

#[derive(Clone, Debug, Deserialize)]
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
pub struct TileCoordinate {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
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
        let path = Path::new(MAP_LAYOUTS_PATH);
        let mut spaces = HashMap::new();

        let entries = fs::read_dir(path).unwrap_or_else(|error| {
            panic!("Failed to read map layouts directory {}: {error}", path.display())
        });

        for entry in entries {
            let entry = entry.unwrap_or_else(|error| {
                panic!("Failed to read map layout entry in {}: {error}", path.display())
            });
            let file_path = entry.path();
            if !file_path.is_file()
                || file_path.extension().and_then(|ext| ext.to_str()) != Some("yaml")
            {
                continue;
            }

            let yaml = fs::read_to_string(&file_path).unwrap_or_else(|error| {
                panic!("Failed to read map layout {}: {error}", file_path.display())
            });

            let mut definition: SpaceDefinition =
                serde_yaml::from_str(&yaml).unwrap_or_else(|error| {
                    panic!("Failed to parse map layout {}: {error}", file_path.display())
                });

            if definition.authored_id.trim().is_empty() {
                definition.authored_id = file_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "Failed to derive authored space id from {}",
                            file_path.display()
                        )
                    })
                    .to_owned();
            }

            definition.validate();
            let previous = spaces.insert(definition.authored_id.clone(), definition);
            assert!(
                previous.is_none(),
                "Duplicate authored space id found while loading {}",
                file_path.display()
            );
        }

        assert!(
            spaces.contains_key(DEFAULT_BOOTSTRAP_SPACE_ID),
            "Missing bootstrap space definition '{}'",
            DEFAULT_BOOTSTRAP_SPACE_ID
        );

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

    pub fn bootstrap_space(&self) -> &SpaceDefinition {
        self.get(&self.bootstrap_space_id).unwrap_or_else(|| {
            panic!(
                "Missing bootstrap space definition '{}'",
                self.bootstrap_space_id
            )
        })
    }

    pub fn get(&self, authored_id: &str) -> Option<&SpaceDefinition> {
        self.spaces.get(authored_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpaceDefinition> {
        self.spaces.values()
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

    fn validate(&mut self) {
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
