use bevy::prelude::*;

use crate::player::components::{MovementCooldown, Player, VitalStats};
use crate::world::components::{OverworldObject, TilePosition, WorldVisual};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::WorldConfig;

pub fn spawn_player(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
) {
    let definition = definitions
        .get("player")
        .unwrap_or_else(|| panic!("Missing overworld object definition for id 'player'"));

    let mut sprite = if let Some(sprite_path) = &definition.render.sprite_path {
        let mut sprite = Sprite::from_image(asset_server.load(sprite_path));
        sprite.custom_size = Some(Vec2::splat(
            world_config.tile_size * definition.render.debug_size,
        ));
        sprite
    } else {
        Sprite::from_color(
            definition.debug_color(),
            Vec2::splat(world_config.tile_size * definition.render.debug_size),
        )
    };
    sprite.image_mode = SpriteImageMode::Auto;

    commands.spawn((
        Player,
        VitalStats::default(),
        MovementCooldown::default(),
        OverworldObject {
            definition_id: "player".to_owned(),
        },
        TilePosition::new(world_config.map_width / 2, world_config.map_height / 2),
        WorldVisual {
            z_index: definition.render.z_index,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));
}
