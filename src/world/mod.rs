pub mod components;
pub mod map_layout;
pub mod object_definitions;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::world::map_layout::MapLayout;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::setup::spawn_world;
use crate::world::systems::sync_tile_transforms;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let map_layout = MapLayout::load_from_disk();
        let world_config = WorldConfig {
            map_width: map_layout.width,
            map_height: map_layout.height,
            tile_size: 48.0,
        };

        app.insert_resource(world_config)
            .insert_resource(map_layout)
            .insert_resource(OverworldObjectDefinitions::load_from_disk())
            .add_systems(Startup, spawn_world)
            .add_systems(Update, sync_tile_transforms);
    }
}

#[derive(Resource)]
pub struct WorldConfig {
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
}
