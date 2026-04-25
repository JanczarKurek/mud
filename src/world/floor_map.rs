use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::components::SpaceId;
use crate::world::floor_definitions::FloorTypeId;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct FloorMap {
    pub width: i32,
    pub height: i32,
    /// Row-major: index = y * width + x. `None` = no floor (transparent void).
    pub tiles: Vec<Option<FloorTypeId>>,
}

impl FloorMap {
    pub fn new_filled(width: i32, height: i32, fill: Option<FloorTypeId>) -> Self {
        assert!(
            width >= 0 && height >= 0,
            "FloorMap dimensions must be non-negative"
        );
        let len = (width as usize) * (height as usize);
        Self {
            width,
            height,
            tiles: vec![fill; len],
        }
    }

    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        Some((y as usize) * (self.width as usize) + (x as usize))
    }

    pub fn get(&self, x: i32, y: i32) -> Option<&FloorTypeId> {
        self.idx(x, y).and_then(|i| self.tiles[i].as_ref())
    }

    /// Returns true on a successful set; false when (x,y) is out of bounds.
    pub fn set(&mut self, x: i32, y: i32, value: Option<FloorTypeId>) -> bool {
        match self.idx(x, y) {
            Some(i) => {
                self.tiles[i] = value;
                true
            }
            None => false,
        }
    }

    pub fn dimensions(&self) -> (i32, i32) {
        (self.width, self.height)
    }
}

#[derive(Resource, Default, Clone, Debug)]
pub struct FloorMaps {
    maps: HashMap<(SpaceId, i32), FloorMap>,
}

impl FloorMaps {
    pub fn insert(&mut self, space_id: SpaceId, z: i32, map: FloorMap) {
        self.maps.insert((space_id, z), map);
    }

    pub fn get(&self, space_id: SpaceId, z: i32) -> Option<&FloorMap> {
        self.maps.get(&(space_id, z))
    }

    pub fn get_mut(&mut self, space_id: SpaceId, z: i32) -> Option<&mut FloorMap> {
        self.maps.get_mut(&(space_id, z))
    }

    pub fn remove_space(&mut self, space_id: SpaceId) {
        self.maps.retain(|(sid, _), _| *sid != space_id);
    }

    pub fn iter(&self) -> impl Iterator<Item = (SpaceId, i32, &FloorMap)> {
        self.maps.iter().map(|((s, z), m)| (*s, *z, m))
    }
}
