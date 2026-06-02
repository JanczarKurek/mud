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

/// `z` is in half-block units: a real floor (where the minimap label flips,
/// where ceilings live, where `FloorMap`s are keyed) sits at every *even* z.
/// `floor_index(z)` collapses a raw z down to the floor it belongs to —
/// `0/1 → floor 0`, `2/3 → floor 1`, etc. Use this anywhere a "what floor am
/// I on" question is being asked (minimap, indoor occlusion, visible-floor
/// culling, floor map lookups). Raw z is used for stack rendering, auto-
/// climb math, and pickup reach.
pub fn floor_index(z: i32) -> i32 {
    z.div_euclid(2)
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
    /// Monotonic placement counter. Each time this object's tile is set or
    /// changed, it gets a fresh value from `PlacementSeqCounter`. Used as a
    /// tiebreaker in both the renderer and the pickup selector so the
    /// most-recently-placed item at a given `(x, y, z)` ends up visually on
    /// top *and* is the one picked first (LIFO). Runtime-only — re-stamped
    /// in load order on world load, no save-format change.
    pub placement_seq: u64,
}

/// Authoritative discrete-state marker for objects whose definition declares
/// a `states:` block (doors open/closed, torches lit/unlit, etc). Mirrored
/// into `ObjectRegistry::properties[id]["state"]` so persistence captures it
/// for free.
#[derive(Component, Clone, Debug, Eq, PartialEq)]
pub struct ObjectState(pub String);

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
    /// Physical size in half-block units, mirrored from
    /// `RenderMetadata.block_size`. `0` = flat ground item, `1` = half block
    /// (chest), `2` = full block (barrel, wall). Drives stack rendering and
    /// bottom-anchored sprite alignment.
    pub block_size: u8,
    /// Sort key for stacking objects on the same tile, mirrored from
    /// `RenderMetadata.stack_order`.
    pub stack_order: i32,
    /// Which building wall this represents — `South`/`East` get faded when
    /// the player is inside an enclosed area. Mirrored from
    /// `RenderMetadata.hide_when_inside_facing`.
    pub hide_when_inside_facing: Option<Direction>,
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

/// Renderer-only mirror of `OverworldObject::placement_seq` (authoritative
/// entities in EmbeddedClient mode) or `ClientWorldObjectState::placement_seq`
/// (projected entities in TcpClient mode). `y_sort_z` adds a tiny term from
/// this so two items sharing the same `tile.z` render in placement order.
/// Lives on the presentation side so `sync_tile_transforms` can read it via
/// the same `Option<&...>` query for both authoritative and projected entities.
#[derive(Component, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderStackOrder(pub u64);

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

/// Schedules an automatic state revert on a stateful object. Attached to an
/// entity by `process_interact_commands` when its triggering interaction
/// declares `respawn_seconds`. Ticked by `tick_respawn_timers`; on expiry the
/// object transitions to `restore_state` and the component is removed.
#[derive(Component, Clone, Debug)]
pub struct RespawnTimer {
    pub remaining_seconds: f32,
    pub restore_state: String,
}
