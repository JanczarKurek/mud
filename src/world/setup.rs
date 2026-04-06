use bevy::prelude::*;

use crate::world::components::{
    Collectible, Collider, Container, OverworldObject, TilePosition, WorldVisual,
};
use crate::world::map_layout::MapLayout;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::WorldConfig;

pub fn spawn_world(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    map_layout: Res<MapLayout>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
) {
    for y in 0..world_config.map_height {
        for x in 0..world_config.map_width {
            spawn_overworld_object(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                &map_layout.fill_object,
                TilePosition::new(x, y),
            );
        }
    }

    for placement_group in &map_layout.placements {
        for tile in &placement_group.tiles {
            spawn_overworld_object(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                &placement_group.object_id,
                tile.to_tile_position(),
            );
        }
    }
}

pub fn spawn_overworld_object(
    commands: &mut Commands,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    definition_id: &str,
    tile_position: TilePosition,
) {
    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));

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

    let mut entity = commands.spawn((
        OverworldObject {
            definition_id: definition_id.to_owned(),
        },
        tile_position,
        WorldVisual {
            z_index: definition.render.z_index,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));

    if definition.colliding {
        entity.insert(Collider);
    }

    if definition.collectible {
        entity.insert(Collectible);
    }

    if let Some(container_capacity) = definition.container_capacity {
        entity.insert(Container {
            slots: vec![None; container_capacity],
        });
    }
}
