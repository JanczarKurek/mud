use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::player::components::{InventoryStack, PlayerId};
use crate::world::direction::Direction;

/// Authoritative facing direction for players, NPCs, and oriented world objects.
/// Replicated to clients via upsert events — presentation code reads this rather
/// than deriving from movement deltas.
#[derive(Component, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Facing(pub Direction);

#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct SpaceId(pub u64);

#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct TilePosition {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub z: i32,
}

impl TilePosition {
    pub const GROUND_FLOOR: i32 = 0;

    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Convenience for the ground floor (z = 0). Preferred at 2D-only call sites
    /// so later floor-aware refactors can grep for `::new(` to find them.
    pub const fn ground(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            z: Self::GROUND_FLOOR,
        }
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

/// Presentation-only position for client-side rendering. Mirrors authoritative
/// `SpaceResident` + `TilePosition` on entities the local process simulates, and
/// is the sole position component on projected entities (see EmbeddedClient
/// Invariant in CLAUDE.md). Every presentation system reads from this, never
/// from the authoritative pair.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewPosition {
    pub space_id: SpaceId,
    pub tile: TilePosition,
}

#[derive(Component)]
#[allow(dead_code)]
pub struct OverworldObject {
    pub object_id: u64,
    pub definition_id: String,
}

#[derive(Component)]
pub struct Collider;

/// Stack size for a world object sitting on the ground. Absent means quantity = 1.
#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Quantity(pub u32);

#[derive(Component)]
pub struct WorldVisual {
    pub z_index: f32,
    pub y_sort: bool,
    pub sprite_height: f32,
    pub rotation_by_facing: bool,
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
pub struct Rotatable;

#[derive(Component)]
pub struct Storable;

#[derive(Component)]
pub struct Container {
    pub slots: Vec<Option<InventoryStack>>,
}
