use std::collections::HashMap;
use std::fs;
use std::path::Path;

use bevy::prelude::*;
use serde::Deserialize;

use crate::world::components::TilePosition;

const MAP_LAYOUT_PATH: &str = "assets/maps/overworld.yaml";

#[derive(Resource, Clone, Debug, Deserialize)]
pub struct MapLayout {
    pub width: i32,
    pub height: i32,
    #[serde(alias = "fill_object")]
    pub fill_object_type: String,
    pub objects: Vec<MapObjectEntry>,
    #[serde(skip)]
    pub resolved_objects: Vec<MapObjectInstance>,
    #[serde(skip)]
    object_indices: HashMap<u64, usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MapObjectInstance {
    pub id: u64,
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default)]
    pub placement: Option<TileCoordinate>,
    #[serde(default)]
    pub contents: Vec<u64>,
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
    pub placement: Vec<TileCoordinate>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct TileCoordinate {
    pub x: i32,
    pub y: i32,
}

impl TileCoordinate {
    pub const fn to_tile_position(self) -> TilePosition {
        TilePosition::new(self.x, self.y)
    }
}

impl MapLayout {
    pub fn load_from_disk() -> Self {
        let path = Path::new(MAP_LAYOUT_PATH);
        let yaml = fs::read_to_string(path).unwrap_or_else(|error| {
            panic!("Failed to read map layout {}: {error}", path.display())
        });

        let mut layout: Self = serde_yaml::from_str(&yaml).unwrap_or_else(|error| {
            panic!("Failed to parse map layout {}: {error}", path.display())
        });

        layout.validate();
        layout
    }

    pub fn get_object(&self, object_id: u64) -> Option<&MapObjectInstance> {
        self.object_indices
            .get(&object_id)
            .and_then(|index| self.resolved_objects.get(*index))
    }

    pub fn object_type_id(&self, object_id: u64) -> Option<&str> {
        self.get_object(object_id)
            .map(|object| object.type_id.as_str())
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
                "Duplicate object id {} found in map layout",
                object.id
            );
        }

        let mut location_counts: HashMap<u64, usize> = HashMap::new();

        for object in &self.resolved_objects {
            if object.placement.is_some() {
                *location_counts.entry(object.id).or_default() += 1;
            }
        }

        for object in &self.resolved_objects {
            for contained_id in &object.contents {
                assert!(
                    *contained_id != object.id,
                    "Object {} cannot contain itself",
                    object.id
                );
                assert!(
                    object_indices.contains_key(contained_id),
                    "Object {} references missing contained object id {}",
                    object.id,
                    contained_id
                );
                *location_counts.entry(*contained_id).or_default() += 1;
            }
        }

        for (object_id, count) in location_counts {
            assert!(
                count <= 1,
                "Object {} appears in more than one place in the map layout",
                object_id
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
                            placement: Some(*tile),
                            contents: Vec::new(),
                        });
                        next_generated_id += 1;
                    }
                }
            }
        }

        self.resolved_objects = resolved_objects;
    }
}
