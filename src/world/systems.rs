use bevy::prelude::*;

use crate::player::components::Player;
use crate::world::components::{TilePosition, WorldVisual};
use crate::world::WorldConfig;

pub fn sync_tile_transforms(
    world_config: Res<WorldConfig>,
    player_query: Query<&TilePosition, With<Player>>,
    mut query: Query<(&TilePosition, &WorldVisual, &mut Transform), Without<Player>>,
) {
    let Ok(player_position) = player_query.single() else {
        return;
    };

    for (tile_position, world_visual, mut transform) in &mut query {
        transform.translation = Vec3::new(
            (tile_position.x - player_position.x) as f32 * world_config.tile_size,
            (tile_position.y - player_position.y) as f32 * world_config.tile_size,
            world_visual.z_index,
        );
    }
}
