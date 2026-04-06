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
    pub fill_object: String,
    pub placements: Vec<ObjectPlacementGroup>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ObjectPlacementGroup {
    pub object_id: String,
    pub tiles: Vec<TileCoordinate>,
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

        serde_yaml::from_str(&yaml).unwrap_or_else(|error| {
            panic!("Failed to parse map layout {}: {error}", path.display())
        })
    }
}
