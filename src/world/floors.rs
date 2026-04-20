use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Highest floor index the camera ever renders relative to the player's floor.
/// The lookup terminates earlier when a covering tile is found. Three is enough
/// for overworld buildings; dungeons with deep towers would raise this.
const MAX_FLOORS_ABOVE: i32 = 3;

/// How many floors below the player to keep visible (dimmed) for depth cues.
const MAX_FLOORS_BELOW: i32 = 3;

/// Alpha tint applied to sprites on floors below the player's.
pub const DIMMED_FLOOR_ALPHA: f32 = 0.55;

/// Window of floor indices currently rendered on the client. Recomputed each
/// frame from the local player's position plus the covering tiles above them.
/// Consumed by `sync_tile_transforms` to cull/dim by floor.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct VisibleFloorRange {
    pub player_floor: i32,
    pub lowest_visible: i32,
    pub highest_visible: i32,
}

impl VisibleFloorRange {
    pub fn contains(&self, floor: i32) -> bool {
        floor >= self.lowest_visible && floor <= self.highest_visible
    }
}

pub fn recompute_visible_floors(
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    mut range: ResMut<VisibleFloorRange>,
) {
    let Some(player_pos) = client_state.player_position else {
        return;
    };
    let player_floor = player_pos.tile_position.z;
    let player_x = player_pos.tile_position.x;
    let player_y = player_pos.tile_position.y;
    let space_id = player_pos.space_id;

    let mut highest_visible = player_floor;
    for step in 1..=MAX_FLOORS_ABOVE {
        let floor = player_floor + step;
        let covered = client_state.world_objects.values().any(|object| {
            if object.position.space_id != space_id || object.tile_position.z != floor {
                return false;
            }
            if object.tile_position.x != player_x || object.tile_position.y != player_y {
                return false;
            }
            definitions
                .get(&object.definition_id)
                .is_some_and(|def| def.render.occludes_floor_above)
        });
        if covered {
            break;
        }
        highest_visible = floor;
    }

    range.player_floor = player_floor;
    range.lowest_visible = player_floor - MAX_FLOORS_BELOW;
    range.highest_visible = highest_visible;
}
