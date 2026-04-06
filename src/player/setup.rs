use bevy::prelude::*;

use crate::player::components::{MovementCooldown, Player};
use crate::world::components::TilePosition;
use crate::world::WorldConfig;

pub fn spawn_player(mut commands: Commands, world_config: Res<WorldConfig>) {
    commands.spawn((
        Player,
        MovementCooldown::default(),
        TilePosition::new(world_config.map_width / 2, world_config.map_height / 2),
        Sprite::from_color(
            Color::srgb(0.75, 0.82, 0.29),
            Vec2::splat(world_config.tile_size * 0.7),
        ),
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));
}
