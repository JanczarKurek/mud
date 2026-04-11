pub mod components;
pub mod map_layout;
pub mod object_definitions;
pub mod object_registry;
pub mod resources;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::game::systems::apply_game_events_to_client_state;
use crate::world::map_layout::MapLayout;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::{ClientRemotePlayerProjectionState, ClientWorldProjectionState};
use crate::world::setup::{spawn_ground_tiles, spawn_world};
use crate::world::systems::{
    sync_client_world_projection, sync_combat_health_bars, sync_remote_player_projection,
    sync_tile_transforms,
};

pub struct WorldServerPlugin;

pub struct WorldClientPlugin;

impl Plugin for WorldServerPlugin {
    fn build(&self, app: &mut App) {
        let map_layout = MapLayout::load_from_disk();
        let world_config = WorldConfig {
            map_width: map_layout.width,
            map_height: map_layout.height,
            tile_size: 48.0,
        };
        let object_registry = ObjectRegistry::from_map_layout(&map_layout);

        app.insert_resource(world_config)
            .insert_resource(map_layout)
            .insert_resource(object_registry)
            .insert_resource(OverworldObjectDefinitions::load_from_disk())
            .add_systems(Startup, spawn_world);
    }
}

impl Plugin for WorldClientPlugin {
    fn build(&self, app: &mut App) {
        let map_layout = MapLayout::load_from_disk();
        let world_config = WorldConfig {
            map_width: map_layout.width,
            map_height: map_layout.height,
            tile_size: 48.0,
        };
        let object_registry = ObjectRegistry::from_map_layout(&map_layout);

        app.insert_resource(world_config)
            .insert_resource(map_layout)
            .insert_resource(object_registry)
            .insert_resource(OverworldObjectDefinitions::load_from_disk())
            .insert_resource(ClientWorldProjectionState::default())
            .insert_resource(ClientRemotePlayerProjectionState::default())
            .add_systems(Startup, spawn_ground_tiles)
            .add_systems(
                Update,
                (
                    sync_client_world_projection.after(apply_game_events_to_client_state),
                    sync_remote_player_projection.after(apply_game_events_to_client_state),
                    sync_tile_transforms,
                    sync_combat_health_bars,
                ),
            );
    }
}

#[derive(Resource)]
pub struct WorldConfig {
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
}
