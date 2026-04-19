pub mod animation;
pub mod components;
pub mod loot;
pub mod map_layout;
pub mod object_definitions;
pub mod object_registry;
pub mod resources;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::assets::AssetResolver;
use crate::game::systems::apply_game_events_to_client_state;
use crate::magic::resources::SpellDefinitions;
use crate::world::animation::{
    advance_animation_timers, attach_animated_sprite, cleanup_just_moved, detect_player_movement,
    return_to_idle_animation, tick_view_scroll, tick_visual_offsets, trigger_movement_animation,
};
use crate::world::map_layout::SpaceDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::{
    ClientRemotePlayerProjectionState, ClientWorldProjectionState, GroundTileConfig, SpaceManager,
    ViewScrollOffset,
};
use crate::world::setup::{
    initialize_runtime_spaces, spawn_ground_tiles_for_current_space, WorldStartupSet,
};
use crate::world::systems::{
    cleanup_empty_ephemeral_spaces, sync_client_world_projection, sync_combat_health_bars,
    sync_player_z, sync_remote_player_projection, sync_tile_transforms,
};

pub struct WorldServerPlugin;

pub struct WorldClientPlugin;

impl Plugin for WorldServerPlugin {
    fn build(&self, app: &mut App) {
        let authored_spaces = SpaceDefinitions::load_from_disk();
        let bootstrap_space = authored_spaces
            .bootstrap_space()
            .expect("Server requires a bootstrap space definition");

        app.insert_resource(WorldConfig {
            current_space_id: crate::world::components::SpaceId(0),
            map_width: bootstrap_space.width,
            map_height: bootstrap_space.height,
            tile_size: 48.0,
            fill_object_type: bootstrap_space.fill_object_type.clone(),
        })
        .insert_resource(authored_spaces.clone())
        .insert_resource(SpaceManager::default())
        .insert_resource(ObjectRegistry::from_space_definitions(&authored_spaces))
        .insert_resource(OverworldObjectDefinitions::load_from_disk())
        .add_systems(
            Startup,
            initialize_runtime_spaces.in_set(WorldStartupSet::InitializeRuntimeSpaces),
        )
        .add_systems(Update, cleanup_empty_ephemeral_spaces)
        .add_plugins(crate::world::loot::LootPlugin);
    }
}

impl Plugin for WorldClientPlugin {
    fn build(&self, app: &mut App) {
        let authored_spaces = SpaceDefinitions::load_from_disk();
        let world_config = authored_spaces
            .bootstrap_space()
            .map(|bs| WorldConfig {
                current_space_id: crate::world::components::SpaceId(0),
                map_width: bs.width,
                map_height: bs.height,
                tile_size: 48.0,
                fill_object_type: bs.fill_object_type.clone(),
            })
            .unwrap_or_else(|| WorldConfig {
                current_space_id: crate::world::components::SpaceId(0),
                map_width: 1,
                map_height: 1,
                tile_size: 48.0,
                fill_object_type: String::new(),
            });
        let object_registry = ObjectRegistry::from_space_definitions(&authored_spaces);

        app.insert_resource(AssetResolver::new())
        .insert_resource(world_config)
        .insert_resource(authored_spaces)
        .insert_resource(object_registry)
        .insert_resource(OverworldObjectDefinitions::load_from_disk())
        .insert_resource(ClientWorldProjectionState::default())
        .insert_resource(ClientRemotePlayerProjectionState::default())
        .insert_resource(ViewScrollOffset::default())
        .insert_resource(GroundTileConfig::default())
        .add_systems(
            OnEnter(ClientAppState::InGame),
            (
                reload_client_definitions.before(crate::player::setup::spawn_player_visual),
                spawn_ground_tiles_for_current_space.after(reload_client_definitions),
            ),
        )
        .add_systems(
            Update,
            (
                sync_client_world_projection.after(apply_game_events_to_client_state),
                sync_remote_player_projection.after(apply_game_events_to_client_state),
                sync_tile_transforms.after(detect_player_movement),
                sync_player_z,
                sync_combat_health_bars,
                spawn_ground_tiles_for_current_space,
                // Animation systems
                attach_animated_sprite.after(sync_client_world_projection),
                advance_animation_timers,
                detect_player_movement.after(apply_game_events_to_client_state),
                trigger_movement_animation
                    .after(sync_client_world_projection)
                    .after(detect_player_movement),
                return_to_idle_animation.after(trigger_movement_animation),
                cleanup_just_moved.after(return_to_idle_animation),
                tick_view_scroll,
                tick_visual_offsets,
            )
                .run_if(in_state(ClientAppState::InGame)),
        );
    }
}

fn reload_client_definitions(
    mut object_defs: ResMut<OverworldObjectDefinitions>,
    mut space_defs: ResMut<SpaceDefinitions>,
    mut spell_defs: ResMut<SpellDefinitions>,
    mut world_config: ResMut<WorldConfig>,
    mut object_registry: ResMut<ObjectRegistry>,
) {
    *object_defs = OverworldObjectDefinitions::load_from_disk();
    let new_space_defs = SpaceDefinitions::load_from_disk();
    if let Some(bs) = new_space_defs.bootstrap_space() {
        world_config.map_width = bs.width;
        world_config.map_height = bs.height;
        world_config.fill_object_type = bs.fill_object_type.clone();
    }
    *object_registry = ObjectRegistry::from_space_definitions(&new_space_defs);
    *space_defs = new_space_defs;
    *spell_defs = SpellDefinitions::load_from_disk();
}

#[derive(Resource, Clone, Debug)]
pub struct WorldConfig {
    pub current_space_id: crate::world::components::SpaceId,
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
    pub fill_object_type: String,
}
