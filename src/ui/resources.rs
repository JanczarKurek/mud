use bevy::prelude::*;

use crate::world::components::TilePosition;

#[derive(Resource)]
pub struct InventoryState {
    pub backpack_slots: Vec<Option<u64>>,
}

impl Default for InventoryState {
    fn default() -> Self {
        Self {
            backpack_slots: vec![None; 8],
        }
    }
}

pub enum DragSource {
    World(Entity),
    Backpack(usize),
    OpenContainer(Entity, usize),
}

#[derive(Resource, Default)]
pub struct OpenContainerState {
    pub entity: Option<Entity>,
}

#[derive(Resource, Default)]
pub struct DragState {
    pub source: Option<DragSource>,
    pub object_id: Option<u64>,
    pub world_origin: Option<TilePosition>,
}
