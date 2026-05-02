pub mod animation;
pub mod components;
pub mod darkness;
pub mod direction;
pub mod floor_definitions;
pub mod floor_map;
pub mod floor_render;
pub mod floors;
pub mod interactions;
pub mod lighting;
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
use crate::game::projection::apply_game_events_to_client_state;
use crate::magic::resources::SpellDefinitions;
use crate::world::animation::{
    advance_animation_timers, attach_animated_sprite, cleanup_just_moved, detect_player_movement,
    return_to_idle_animation, tick_view_scroll, tick_visual_offsets, trigger_movement_animation,
};
use crate::world::darkness::{
    setup_darkness_overlay, update_darkness_overlay, DarknessOverlayMaterial,
};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::floor_render::{
    build_floor_render_cells, consume_floor_render_dirty, sync_floor_render_transforms,
    FloorRenderDirty, FloorRenderState, FloorTilesetAtlases,
};
use crate::world::floors::{recompute_visible_floors, VisibleFloorRange};
use crate::world::lighting::{advance_world_clock, sync_object_light_components, WorldClock};
use crate::world::map_layout::SpaceDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::{
    ClientRemotePlayerProjectionState, ClientWorldProjectionState, SpaceManager, ViewScrollOffset,
};
use crate::world::setup::{initialize_runtime_spaces, WorldStartupSet};
use crate::world::systems::{
    cleanup_empty_ephemeral_spaces, sync_authoritative_world_object_position_view,
    sync_client_world_projection, sync_combat_health_bars, sync_player_z,
    sync_remote_player_projection, sync_tile_transforms,
};

pub struct WorldServerPlugin;

pub struct WorldClientPlugin;

impl Plugin for WorldServerPlugin {
    fn build(&self, app: &mut App) {
        let mut authored_spaces = SpaceDefinitions::load_from_disk();
        let object_definitions = OverworldObjectDefinitions::load_from_disk();
        // wires_to resolution rewrites authored target ids in object
        // properties to runtime u64 strings. Must happen before the
        // ObjectRegistry snapshots properties.
        authored_spaces.resolve_wiring(&object_definitions);
        let bootstrap_space = authored_spaces
            .bootstrap_space()
            .expect("Server requires a bootstrap space definition");

        app.insert_resource(WorldConfig {
            current_space_id: crate::world::components::SpaceId(0),
            map_width: bootstrap_space.width,
            map_height: bootstrap_space.height,
            tile_size: 48.0,
            fill_floor_type: bootstrap_space.fill_floor_type.clone(),
        })
        .insert_resource(authored_spaces.clone())
        .insert_resource(SpaceManager::default())
        .insert_resource(FloorMaps::default())
        .insert_resource(FloorRenderDirty::default())
        .insert_resource(WorldClock::default())
        .insert_resource(ObjectRegistry::from_space_definitions(&authored_spaces))
        .insert_resource(object_definitions)
        .insert_resource(FloorTilesetDefinitions::load_from_disk())
        .add_systems(
            Startup,
            initialize_runtime_spaces.in_set(WorldStartupSet::InitializeRuntimeSpaces),
        )
        .add_systems(
            Update,
            advance_world_clock.run_if(crate::app::state::simulation_active),
        )
        .add_systems(Update, cleanup_empty_ephemeral_spaces)
        .add_plugins(crate::world::loot::LootPlugin);
    }
}

impl Plugin for WorldClientPlugin {
    fn build(&self, app: &mut App) {
        let mut authored_spaces = SpaceDefinitions::load_from_disk();
        let object_definitions = OverworldObjectDefinitions::load_from_disk();
        authored_spaces.resolve_wiring(&object_definitions);
        let world_config = authored_spaces
            .bootstrap_space()
            .map(|bs| WorldConfig {
                current_space_id: crate::world::components::SpaceId(0),
                map_width: bs.width,
                map_height: bs.height,
                tile_size: 48.0,
                fill_floor_type: bs.fill_floor_type.clone(),
            })
            .unwrap_or_else(|| WorldConfig {
                current_space_id: crate::world::components::SpaceId(0),
                map_width: 1,
                map_height: 1,
                tile_size: 48.0,
                fill_floor_type: String::new(),
            });
        let object_registry = ObjectRegistry::from_space_definitions(&authored_spaces);

        app.insert_resource(AssetResolver::new())
            .insert_resource(world_config)
            .insert_resource(authored_spaces)
            .insert_resource(object_registry)
            .insert_resource(object_definitions)
            .insert_resource(FloorTilesetDefinitions::load_from_disk())
            .insert_resource(FloorTilesetAtlases::default())
            .insert_resource(FloorRenderState::default())
            .insert_resource(FloorRenderDirty::default())
            .insert_resource(ClientWorldProjectionState::default())
            .insert_resource(ClientRemotePlayerProjectionState::default())
            .insert_resource(ViewScrollOffset::default())
            .insert_resource(VisibleFloorRange::default())
            .add_plugins(bevy::sprite_render::Material2dPlugin::<
                DarknessOverlayMaterial,
            >::default())
            .add_systems(
                OnEnter(ClientAppState::InGame),
                (
                    reload_client_definitions.before(crate::player::setup::spawn_player_visual),
                    setup_darkness_overlay,
                ),
            )
            .add_systems(
                Update,
                (
                    sync_client_world_projection.after(apply_game_events_to_client_state),
                    sync_remote_player_projection.after(apply_game_events_to_client_state),
                    sync_authoritative_world_object_position_view
                        .after(apply_game_events_to_client_state)
                        .before(sync_tile_transforms),
                    recompute_visible_floors
                        .after(apply_game_events_to_client_state)
                        .before(sync_tile_transforms),
                    sync_tile_transforms.after(detect_player_movement),
                    sync_player_z,
                    sync_combat_health_bars,
                    build_floor_render_cells.after(apply_game_events_to_client_state),
                    consume_floor_render_dirty
                        .after(apply_game_events_to_client_state)
                        .after(build_floor_render_cells),
                    sync_floor_render_transforms.after(detect_player_movement),
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
            )
            .add_systems(
                Update,
                (
                    sync_object_light_components.after(sync_client_world_projection),
                    // Reads each LightSource source's finalized Transform
                    // (post-`sync_tile_transforms`), the active space's
                    // ambient config, and the world clock — produces the
                    // GPU uniforms for the fullscreen darkness quad.
                    update_darkness_overlay
                        .after(sync_object_light_components)
                        .after(sync_tile_transforms)
                        .after(sync_player_z)
                        .after(recompute_visible_floors),
                )
                    .run_if(in_state(ClientAppState::InGame)),
            );
    }
}

fn reload_client_definitions(
    mut object_defs: ResMut<OverworldObjectDefinitions>,
    mut floor_defs: ResMut<FloorTilesetDefinitions>,
    mut space_defs: ResMut<SpaceDefinitions>,
    mut spell_defs: ResMut<SpellDefinitions>,
    mut world_config: ResMut<WorldConfig>,
) {
    *object_defs = OverworldObjectDefinitions::load_from_disk();
    *floor_defs = FloorTilesetDefinitions::load_from_disk();
    let mut new_space_defs = SpaceDefinitions::load_from_disk();
    new_space_defs.resolve_wiring(&object_defs);
    if let Some(bs) = new_space_defs.bootstrap_space() {
        world_config.map_width = bs.width;
        world_config.map_height = bs.height;
        world_config.fill_floor_type = bs.fill_floor_type.clone();
    }
    *space_defs = new_space_defs;
    *spell_defs = SpellDefinitions::load_from_disk();
}

#[derive(Resource, Clone, Debug)]
pub struct WorldConfig {
    pub current_space_id: crate::world::components::SpaceId,
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
    pub fill_floor_type: String,
}
