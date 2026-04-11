use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::player::components::PlayerId;

#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct SpaceId(pub u64);

#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TilePosition {
    pub x: i32,
    pub y: i32,
}

impl TilePosition {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpacePosition {
    pub space_id: SpaceId,
    pub tile_position: TilePosition,
}

impl SpacePosition {
    pub const fn new(space_id: SpaceId, tile_position: TilePosition) -> Self {
        Self {
            space_id,
            tile_position,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpaceResident {
    pub space_id: SpaceId,
}

#[derive(Component)]
#[allow(dead_code)]
pub struct OverworldObject {
    pub object_id: u64,
    pub definition_id: String,
}

#[derive(Component)]
pub struct Collider;

#[derive(Component)]
pub struct WorldVisual {
    pub z_index: f32,
}

#[derive(Component)]
pub struct CombatHealthBar {
    pub root_entity: Entity,
    pub fill_entity: Entity,
    pub fill_width: f32,
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct DisplayedVitalStats {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthBarDisplayPolicy {
    pub always_visible: bool,
}

#[derive(Component, Clone, Debug, Eq, PartialEq)]
pub struct ClientProjectedWorldObject {
    pub object_id: u64,
    pub definition_id: String,
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientRemotePlayerVisual {
    pub player_id: PlayerId,
    pub object_id: u64,
}

#[derive(Component)]
pub struct Movable;

#[derive(Component)]
pub struct Storable;

#[derive(Component)]
pub struct Container {
    pub slots: Vec<Option<u64>>,
}

#[derive(Component)]
pub struct ClientGroundTile;
