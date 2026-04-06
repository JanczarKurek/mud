use bevy::prelude::*;

use crate::world::components::{Collider, OverworldObject, TilePosition, WorldVisual};
use crate::world::map_layout::MapLayout;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::WorldConfig;

pub fn load_map_layout(mut map_layout: ResMut<MapLayout>, mut world_config: ResMut<WorldConfig>) {
    *map_layout = MapLayout::load_from_disk();
    world_config.map_width = map_layout.width;
    world_config.map_height = map_layout.height;
}

pub fn load_overworld_object_definitions(mut definitions: ResMut<OverworldObjectDefinitions>) {
    *definitions = OverworldObjectDefinitions::load_from_disk();
}

pub fn spawn_world(
    mut commands: Commands,
    map_layout: Res<MapLayout>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
) {
    for y in 0..world_config.map_height {
        for x in 0..world_config.map_width {
            spawn_overworld_object(
                &mut commands,
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
                &definitions,
                &world_config,
                &placement_group.object_id,
                tile.to_tile_position(),
            );
        }
    }
}

fn spawn_overworld_object(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    definition_id: &str,
    tile_position: TilePosition,
) {
    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));

    let mut entity = commands.spawn((
        OverworldObject {
            definition_id: definition_id.to_owned(),
        },
        tile_position,
        WorldVisual {
            z_index: definition.render.z_index,
        },
        Sprite::from_color(
            definition.debug_color(),
            Vec2::splat(world_config.tile_size * definition.render.debug_size),
        ),
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));

    if definition.colliding {
        entity.insert(Collider);
    }
}
