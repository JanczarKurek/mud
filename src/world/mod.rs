pub mod animation;
pub mod attached;
pub mod building_presets;
pub mod camera;
pub mod components;
pub mod darkness;
pub mod direction;
pub mod dungeon_gen;
pub mod floor_animation;
pub mod floor_definitions;
pub mod floor_map;
pub mod floor_render;
pub mod floors;
pub mod fog_render;
pub mod hidden;
pub mod hide_action;
pub mod interactions;
pub mod lerp_anim;
pub mod lighting;
pub mod loot;
pub mod map_layout;
pub mod object_definitions;
pub mod object_registry;
pub mod resources;
pub mod setup;
pub mod stacks;
pub mod step_triggers;
pub mod systems;
pub mod ttl;
pub mod vfx;

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::assets::AssetResolver;
use crate::game::projection::apply_game_events_to_client_state;
use crate::magic::resources::SpellDefinitions;
use crate::world::animation::{
    advance_animation_timers, attach_animated_sprite, cleanup_just_moved, detect_player_movement,
    return_to_idle_animation, tick_floor_transition, tick_view_scroll, tick_visual_offsets,
    trigger_movement_animation,
};
use crate::world::attached::sync_attached_object_visuals;
use crate::world::camera::camera_follow;
use crate::world::darkness::{
    setup_darkness_overlay, update_darkness_overlay, DarknessOverlayMaterial,
};
use crate::world::floor_animation::{
    despawn_finished_ripples, sync_ripple_overlay_transforms, tick_floor_ripple_scheduler,
    FloorRippleAtlases, FloorRippleScheduler,
};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::floor_render::{
    build_floor_render_cells, consume_floor_render_dirty, sync_floor_render_transforms,
    FloorRenderDirty, FloorRenderState, FloorTilesetAtlases,
};
use crate::world::floors::{
    recompute_indoor_tile_map, recompute_visible_floors, IndoorTileMap, VisibleFloorRange,
};
use crate::world::fog_render::{setup_fog_overlay, update_fog_overlay, FogOfWarMaterial};
use crate::world::lighting::{advance_world_clock, sync_object_light_components, WorldClock};
use crate::world::map_layout::SpaceDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::{
    ClientRemotePlayerProjectionState, ClientWorldProjectionState, FloorTransitionOffset,
    SpaceManager, ViewScrollOffset,
};
use crate::world::setup::{initialize_runtime_spaces, WorldStartupSet};
use crate::world::step_triggers::{
    process_continuous_step_triggers, process_step_triggers, PendingStepEvents,
};
use crate::world::systems::{
    cleanup_empty_ephemeral_spaces, sync_authoritative_world_object_position_view,
    sync_client_world_projection, sync_combat_health_bars, sync_player_z,
    sync_remote_player_projection, sync_tile_transforms,
};
use crate::world::vfx::VfxDefinitions;

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
        .insert_resource(VfxDefinitions::load_from_disk())
        .insert_resource(FloorTilesetDefinitions::load_from_disk())
        .insert_resource(PendingStepEvents::default())
        .insert_resource(crate::world::stacks::PendingStackSettleEvents::default())
        .add_systems(
            Startup,
            initialize_runtime_spaces.in_set(WorldStartupSet::InitializeRuntimeSpaces),
        )
        .add_systems(
            Update,
            advance_world_clock.run_if(crate::app::state::simulation_active),
        )
        .add_systems(
            Update,
            // Drain the step-event queue after every movement site has pushed
            // and before damage events are resolved, so trap damage lands in
            // the same frame as the step that triggered it.
            process_step_triggers
                .after(crate::game::systems::process_game_commands)
                .after(crate::npc::systems::update_roaming_npcs)
                .before(crate::combat::damage::apply_pending_damage)
                .run_if(crate::app::state::simulation_active),
        )
        .add_systems(
            Update,
            // Drain pending stack-settle requests after the command handlers
            // (pickup / move) push them, and before event collection so the
            // re-stacked positions get replicated to clients in the same frame.
            crate::world::stacks::settle_pending_stacks
                .after(crate::game::systems::process_game_commands)
                .before(crate::game::projection::collect_game_events_from_authority)
                .run_if(crate::app::state::simulation_active),
        )
        .add_systems(
            Update,
            // Stamp `placement_seq` onto freshly-spawned `OverworldObject`s
            // before the projection serializes them, so the very first
            // `WorldObjectUpserted` for a new item already carries the right
            // LIFO ordering. Independent of `simulation_active` so items
            // spawned at world-load (before sim starts) are also stamped.
            crate::world::stacks::stamp_placement_seq_on_spawn
                .after(crate::game::systems::process_game_commands)
                .before(crate::game::projection::collect_game_events_from_authority),
        )
        .add_systems(
            Update,
            // Mirror the authoritative placement_seq to a presentation-side
            // `RenderStackOrder` component so the renderer can break z-ties
            // (e.g. multiple flat items at z=0) in LIFO order. Must run after
            // the stamp system so the fresh seq is visible to `Changed`.
            crate::world::stacks::sync_render_stack_order
                .after(crate::world::stacks::stamp_placement_seq_on_spawn),
        )
        .add_systems(
            Update,
            // The "while standing on" half of the step-trigger pipeline.
            // Ordered after the one-shot path so an entry hit always lands
            // before the very next periodic tick on the same frame.
            process_continuous_step_triggers
                .after(process_step_triggers)
                .before(crate::combat::damage::apply_pending_damage)
                .run_if(crate::app::state::simulation_active),
        )
        .add_systems(
            Update,
            // Per-(player, hidden-object) Perception rolls. Runs after step
            // triggers (so an auto-reveal lands first) and before event
            // collection so any reveal that fires this frame appears to the
            // client in the same tick.
            crate::world::hidden::passive_perception_tick
                .after(process_continuous_step_triggers)
                .before(crate::game::projection::collect_game_events_from_authority)
                .run_if(crate::app::state::simulation_active),
        )
        .add_systems(Update, cleanup_empty_ephemeral_spaces)
        .add_plugins(crate::world::ttl::TtlPlugin);
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
            .insert_resource(VfxDefinitions::load_from_disk())
            .insert_resource(FloorTilesetDefinitions::load_from_disk())
            .insert_resource(FloorTilesetAtlases::default())
            .insert_resource(FloorRenderState::default())
            .insert_resource(FloorRenderDirty::default())
            .insert_resource(FloorRippleAtlases::default())
            .insert_resource(FloorRippleScheduler::default())
            .insert_resource(ClientWorldProjectionState::default())
            .insert_resource(ClientRemotePlayerProjectionState::default())
            .insert_resource(ViewScrollOffset::default())
            .insert_resource(FloorTransitionOffset::default())
            .insert_resource(VisibleFloorRange::default())
            .insert_resource(IndoorTileMap::default())
            .add_plugins(bevy::sprite_render::Material2dPlugin::<
                DarknessOverlayMaterial,
            >::default())
            .add_plugins(bevy::sprite_render::Material2dPlugin::<FogOfWarMaterial>::default())
            .add_systems(
                OnEnter(ClientAppState::InGame),
                (
                    reload_client_definitions.before(crate::player::setup::spawn_player_visual),
                    setup_darkness_overlay,
                    setup_fog_overlay,
                ),
            )
            // Darkness overlay also lives in the map editor so authoring the
            // day/night curve is WYSIWYG. Fog is intentionally InGame-only —
            // the editor wants a clean view of the whole map.
            .add_systems(OnEnter(ClientAppState::MapEditor), setup_darkness_overlay)
            .add_systems(
                Update,
                (
                    (
                        sync_client_world_projection.after(apply_game_events_to_client_state),
                        sync_remote_player_projection.after(apply_game_events_to_client_state),
                        sync_authoritative_world_object_position_view
                            .after(apply_game_events_to_client_state)
                            .before(sync_tile_transforms),
                        recompute_visible_floors
                            .after(apply_game_events_to_client_state)
                            .before(sync_tile_transforms),
                        // Build the indoor-tile lookup once per frame so the
                        // tint consumers (sync_tile_transforms,
                        // sync_floor_render_transforms) get O(1) per-tile
                        // lookups instead of O(world_objects) per call.
                        recompute_indoor_tile_map
                            .after(apply_game_events_to_client_state)
                            .before(sync_tile_transforms)
                            .before(sync_floor_render_transforms),
                        sync_tile_transforms.after(detect_player_movement),
                        sync_player_z,
                        sync_combat_health_bars,
                        build_floor_render_cells
                            .after(apply_game_events_to_client_state)
                            .after(recompute_visible_floors),
                        consume_floor_render_dirty
                            .after(apply_game_events_to_client_state)
                            .after(build_floor_render_cells),
                        sync_floor_render_transforms
                            .after(detect_player_movement)
                            .after(recompute_visible_floors),
                    ),
                    // Animation + camera systems
                    attach_animated_sprite.after(sync_client_world_projection),
                    detect_player_movement.after(apply_game_events_to_client_state),
                    trigger_movement_animation
                        .after(sync_client_world_projection)
                        .after(detect_player_movement),
                    return_to_idle_animation.after(trigger_movement_animation),
                    cleanup_just_moved
                        .after(return_to_idle_animation)
                        .after(tick_view_scroll)
                        .after(tick_visual_offsets),
                    tick_view_scroll.after(detect_player_movement),
                    tick_floor_transition.after(detect_player_movement),
                    tick_visual_offsets.after(detect_player_movement),
                    camera_follow
                        .after(tick_view_scroll)
                        .after(detect_player_movement),
                    sync_attached_object_visuals
                        .after(sync_tile_transforms)
                        .after(sync_player_z)
                        .after(camera_follow),
                    // Sparse Poisson-driven floor ripples (e.g. water).
                    (
                        tick_floor_ripple_scheduler
                            .after(apply_game_events_to_client_state)
                            .after(recompute_visible_floors),
                        sync_ripple_overlay_transforms.after(detect_player_movement),
                        despawn_finished_ripples.after(advance_animation_timers),
                    ),
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
                    // GPU uniforms for the fullscreen darkness quad. Runs in
                    // editor mode too so the lighting panel previews live.
                    update_darkness_overlay
                        .after(sync_object_light_components)
                        .after(sync_tile_transforms)
                        .after(sync_player_z)
                        .after(recompute_visible_floors)
                        .after(camera_follow)
                        .run_if(in_game_or_editor),
                    // Fog overlay: reads the replicated `discovered_tiles`
                    // set from `ClientGameState` and packs the bitmask each
                    // frame. Camera-follow keeps the quad over the viewport.
                    update_fog_overlay
                        .after(apply_game_events_to_client_state)
                        .after(camera_follow)
                        .run_if(in_state(ClientAppState::InGame)),
                    // Frame cycling runs in both gameplay and the editor so
                    // authored objects animate during map editing too.
                    // Single registration avoids ambiguous SystemTypeSet for
                    // `despawn_finished_ripples.after(advance_animation_timers)`.
                    advance_animation_timers.run_if(in_game_or_editor),
                ),
            );

        // `TtlPlugin` ticks down both server-authoritative TTL entities (corpses,
        // spell summons) and presentation-only client-spawned VFX. EmbeddedClient
        // already gets it from `WorldServerPlugin`; TcpClient (which has no
        // server-side plugins) needs it here so hit/effect sprites despawn after
        // their animation ends instead of lingering forever.
        if !app.is_plugin_added::<crate::world::ttl::TtlPlugin>() {
            app.add_plugins(crate::world::ttl::TtlPlugin);
        }

        // Mirror authoritative (object_id → definition_id) mappings from the
        // replicated `ClientGameState.world_objects` into the local
        // `ObjectRegistry`. Without this, every UI lookup that goes through
        // `object_registry.type_id(server_runtime_id)` (container titles,
        // context-menu probes, drag previews, `object_is_usable`, …) returns
        // whatever the client's *authored* registry happens to have at that
        // numeric slot — which can collide with a totally different type and
        // produce "goblin corpse → Wall" / "spark wand → flower" style
        // mislabels. EmbeddedClient mode is unaffected (the server mutates the
        // shared registry directly), so guard on `simulation_active` and only
        // refresh when the projection changes.
        app.add_systems(
            Update,
            mirror_client_world_objects_into_registry
                .after(apply_game_events_to_client_state)
                .run_if(in_state(ClientAppState::InGame)),
        );
    }
}

/// Run condition: gameplay or the map editor. Used to keep the darkness
/// overlay, sprite animation timers, and any future "show me the world like
/// the player sees it" systems running while the user is authoring in the
/// editor.
pub(crate) fn in_game_or_editor(state: Res<State<ClientAppState>>) -> bool {
    matches!(
        *state.get(),
        ClientAppState::InGame | ClientAppState::MapEditor
    )
}

fn mirror_client_world_objects_into_registry(
    client_state: Res<crate::game::resources::ClientGameState>,
    mut object_registry: ResMut<ObjectRegistry>,
) {
    if !client_state.is_changed() {
        return;
    }
    for (object_id, state) in &client_state.world_objects {
        let needs_update = object_registry
            .type_id(*object_id)
            .is_none_or(|existing| existing != state.definition_id);
        if needs_update {
            object_registry.register_existing(*object_id, state.definition_id.clone());
        }
    }
}

fn reload_client_definitions(
    mut object_defs: ResMut<OverworldObjectDefinitions>,
    mut floor_defs: ResMut<FloorTilesetDefinitions>,
    mut space_defs: ResMut<SpaceDefinitions>,
    mut spell_defs: ResMut<SpellDefinitions>,
    mut vfx_defs: ResMut<VfxDefinitions>,
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
    *vfx_defs = VfxDefinitions::load_from_disk();
}

#[derive(Resource, Clone, Debug)]
pub struct WorldConfig {
    pub current_space_id: crate::world::components::SpaceId,
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
    pub fill_floor_type: String,
}
