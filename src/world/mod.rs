pub mod components;
pub mod map_layout;
pub mod object_definitions;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::world::map_layout::MapLayout;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::setup::{load_map_layout, load_overworld_object_definitions, spawn_world};
use crate::world::systems::sync_tile_transforms;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WorldConfig::default())
            .insert_resource(MapLayout::load_from_disk())
            .insert_resource(OverworldObjectDefinitions::default())
            .add_systems(
                Startup,
                (
                    load_map_layout,
                    load_overworld_object_definitions,
                    spawn_world,
                )
                    .chain(),
            )
            .add_systems(Update, sync_tile_transforms);
    }
}

#[derive(Resource)]
pub struct WorldConfig {
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            map_width: 40,
            map_height: 30,
            tile_size: 48.0,
        }
    }
}
