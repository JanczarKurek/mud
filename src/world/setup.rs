use bevy::prelude::*;

use crate::world::components::{
    Collectible, Collider, Container, OverworldObject, TilePosition, WorldVisual,
};
use crate::world::map_layout::{MapLayout, MapObjectInstance};
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
            spawn_ground_tile(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                &map_layout.fill_object_type,
                TilePosition::new(x, y),
            );
        }
    }

    for object in &map_layout.resolved_objects {
        if map_layout.is_contained(object.id) {
            continue;
        }

        let Some(placement) = object.placement else {
            continue;
        };

        spawn_overworld_object_instance(
            &mut commands,
            &asset_server,
            &map_layout,
            &definitions,
            &world_config,
            object,
            placement.to_tile_position(),
        );
    }
}

pub fn spawn_overworld_object_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    map_layout: &MapLayout,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    object: &MapObjectInstance,
    tile_position: TilePosition,
) {
    let container_contents = if object.contents.is_empty() {
        None
    } else {
        Some(object.contents.clone())
    };

    spawn_overworld_object(
        commands,
        asset_server,
        definitions,
        world_config,
        object.id,
        &object.type_id,
        container_contents,
        tile_position,
    );
    let _ = map_layout;
}

fn spawn_ground_tile(
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

    commands.spawn((
        tile_position,
        WorldVisual {
            z_index: definition.render.z_index,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));
}

pub fn spawn_overworld_object(
    commands: &mut Commands,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    object_id: u64,
    definition_id: &str,
    container_contents: Option<Vec<u64>>,
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
            object_id,
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
        let mut slots = vec![None; container_capacity];
        if let Some(contents) = container_contents {
            assert!(
                contents.len() <= container_capacity,
                "Container object {} exceeds capacity {}",
                object_id,
                container_capacity
            );
            for (index, contained_object_id) in contents.into_iter().enumerate() {
                slots[index] = Some(contained_object_id);
            }
        }

        entity.insert(Container { slots });
    }
}
