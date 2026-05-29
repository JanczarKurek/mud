#![allow(clippy::type_complexity)]
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::clipboard::cancel_paste;
use crate::editor::resources::{
    BehaviorKind, ConfirmedLightingKeyframe, ConfirmedSpawnGroup, EditingField, EditorCamera,
    EditorContext, EditorLightingBuffer, EditorMapBuffers, EditorPortalBuffer,
    EditorPropertyEditBuffer, EditorSpaceResetDeps, EditorSpawnGroupBuffer, EditorState,
    EditorTool, EditorViewState, LightingKeyframeDraft, ModalConfirmed, ModalKind,
    ModalPickerField, ModalPickerOption, ModalState, ModalTextField, SpawnAreaKind,
    SpawnGroupDraft, UndoOp, UndoStack,
};
use crate::editor::serializer::serialize_and_save;
use crate::editor::templates::{save_template, EditorTemplatesIndex};
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::npc::components::SpawnGroupMember;
use crate::npc::spawn_groups::SpawnGroupRegistry;
use crate::player::components::Player;
use crate::world::animation::VisualOffset;
use crate::world::components::{
    OverworldObject, SpaceId, SpaceResident, TilePosition, ViewPosition, WorldVisual,
};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::map_layout::{
    AmbientKeyframe, MapBehavior, PortalDefinition, SpaceDefinitions, SpawnArea, SpawnGroupDef,
    TileCoordinate, TileRectangle,
};
use crate::world::object_definitions::{OverworldObjectDefinition, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::{RuntimeSpace, SpaceManager};
use crate::world::setup::{build_object_visual_bundle, instantiate_space, spawn_overworld_object};
use crate::world::WorldConfig;

// ── Visuals helper (public so undo.rs can use it) ────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn insert_editor_visuals_pub(
    entity_commands: &mut EntityCommands,
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    def: &OverworldObjectDefinition,
    world_config: &WorldConfig,
    tile: TilePosition,
    camera: &EditorCamera,
) {
    insert_editor_visuals(
        entity_commands,
        asset_server,
        texture_atlas_layouts,
        def,
        world_config,
        tile,
        camera,
    );
}

fn insert_editor_visuals(
    entity_commands: &mut EntityCommands,
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    def: &OverworldObjectDefinition,
    world_config: &WorldConfig,
    tile: TilePosition,
    camera: &EditorCamera,
) {
    let effective_size = world_config.tile_size * camera.zoom_level;
    let bundle = build_object_visual_bundle(
        asset_server,
        texture_atlas_layouts,
        def,
        world_config,
        None,
        1,
    );
    let bottom_anchored = bundle.anchor.is_some();
    let anchor_y_offset = if bottom_anchored {
        -effective_size * 0.5
    } else {
        0.0
    };
    let x = (tile.x as f32 - camera.center.x) * effective_size;
    let y = (tile.y as f32 - camera.center.y) * effective_size + anchor_y_offset;
    entity_commands.try_insert((
        bundle.sprite,
        bundle.world_visual,
        Transform::from_xyz(x, y, def.render.z_index).with_scale(Vec3::splat(camera.zoom_level)),
    ));
    if let Some(animated) = bundle.animated {
        entity_commands.try_insert(animated);
    }
    if let Some(anchor) = bundle.anchor {
        entity_commands.try_insert(anchor);
    }
}

// ── Space reset ───────────────────────────────────────────────────────────────

/// Despawn everything in `space_id` except the player, rebuild the floor map
/// from `def`, drop any spawn-group runtime state for the space, then re-spawn
/// the authored objects from `def.resolved_objects`. Keeps the same `space_id`
/// (so the camera, editor context, and other dangling references stay valid);
/// only the *contents* of the space are rebuilt.
///
/// Called when entering the editor and on file-open so the view reflects the
/// YAML rather than whatever runtime state happened to be in the world (spawn-
/// group NPCs, dropped items, gameplay-mutated state).
#[allow(clippy::too_many_arguments)]
pub fn reset_space_contents_from_def(
    commands: &mut Commands,
    space_id: SpaceId,
    def: &crate::world::map_layout::SpaceDefinition,
    object_definitions: &OverworldObjectDefinitions,
    object_registry: &ObjectRegistry,
    floor_maps: &mut FloorMaps,
    spawn_group_registry: &mut SpawnGroupRegistry,
    residents: &Query<(Entity, &SpaceResident), Without<Player>>,
    portal_markers: &Query<Entity, With<crate::editor::resources::EditorPortalMarker>>,
) {
    for (entity, resident) in residents.iter() {
        if resident.space_id == space_id {
            commands.entity(entity).despawn();
        }
    }
    for entity in portal_markers.iter() {
        commands.entity(entity).despawn();
    }

    spawn_group_registry
        .groups
        .retain(|key, _| key.space_id != space_id);

    floor_maps.insert(
        space_id,
        TilePosition::GROUND_FLOOR,
        def.build_floor_map(TilePosition::GROUND_FLOOR),
    );

    for object in &def.resolved_objects {
        if def.is_contained(object.id) {
            continue;
        }
        let Some(placement) = object.placement else {
            continue;
        };
        crate::world::setup::spawn_overworld_object_instance(
            commands,
            object_definitions,
            object_registry,
            def,
            object,
            space_id,
            placement.to_tile_position(),
        );
    }
}

/// `OnEnter(MapEditor)` system: refresh the editor's space from its YAML
/// definition. Runs after `init_editor_context` so `EditorContext.space_id` is
/// populated. Without this the editor inherits whatever was in the world (e.g.
/// spawn-group NPCs that ran in `InGame`), and saving would round-trip those
/// runtime entities into the YAML as static placements.
pub fn reset_space_to_authored(
    mut commands: Commands,
    editor_context: Res<EditorContext>,
    space_definitions: Res<SpaceDefinitions>,
    object_definitions: Res<OverworldObjectDefinitions>,
    object_registry: Res<ObjectRegistry>,
    mut floor_maps: ResMut<FloorMaps>,
    mut reset_deps: EditorSpaceResetDeps,
) {
    let Some(def) = space_definitions.get(&editor_context.authored_id) else {
        return;
    };
    reset_space_contents_from_def(
        &mut commands,
        editor_context.space_id,
        def,
        &object_definitions,
        &object_registry,
        &mut floor_maps,
        &mut reset_deps.spawn_group_registry,
        &reset_deps.residents,
        &reset_deps.portal_markers,
    );
}

// ── Initialization ────────────────────────────────────────────────────────────

pub fn init_editor_context(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    space_manager: Res<SpaceManager>,
    space_definitions: Res<SpaceDefinitions>,
    mut editor_camera: ResMut<EditorCamera>,
) {
    let space_id = world_config.current_space_id;
    let authored_id = space_manager
        .get(space_id)
        .map(|s| s.authored_id.clone())
        .unwrap_or_else(|| space_definitions.bootstrap_space_id.clone());

    editor_camera.center = Vec2::new(
        world_config.map_width as f32 * 0.5,
        world_config.map_height as f32 * 0.5,
    );

    commands.insert_resource(EditorContext {
        space_id,
        authored_id,
        map_width: world_config.map_width,
        map_height: world_config.map_height,
        fill_floor_type: world_config.fill_floor_type.clone(),
    });
}

pub fn init_portal_buffer(
    editor_context: Res<EditorContext>,
    space_definitions: Res<SpaceDefinitions>,
    mut portal_buffer: ResMut<EditorPortalBuffer>,
    mut spawn_group_buffer: ResMut<EditorSpawnGroupBuffer>,
    mut lighting_buffer: ResMut<EditorLightingBuffer>,
    mut vendor_stash_buffer: ResMut<crate::editor::resources::EditorVendorStashBuffer>,
) {
    let def = space_definitions.get(&editor_context.authored_id);
    portal_buffer.portals = def.map(|d| d.portals.clone()).unwrap_or_default();
    spawn_group_buffer.groups = def.map(|d| d.spawn_groups.clone()).unwrap_or_default();
    spawn_group_buffer.selected = None;
    lighting_buffer.config = def.map(|d| d.lighting.clone()).unwrap_or_default();
    lighting_buffer.selected_keyframe = None;
    vendor_stash_buffer.stashes = def.map(|d| d.vendor_stashes.clone()).unwrap_or_default();
    vendor_stash_buffer.selected = None;
    vendor_stash_buffer.editing = None;
    vendor_stash_buffer.edit_text.clear();
    vendor_stash_buffer.pending_ware_pick = None;
}

#[allow(clippy::too_many_arguments)]
pub fn attach_editor_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    objects: Query<
        (Entity, &OverworldObject, &TilePosition, &SpaceResident),
        (Without<Transform>, Without<Player>),
    >,
) {
    for (entity, obj, tile, resident) in &objects {
        // Only attach visuals for objects in the active editing space
        if resident.space_id != editor_context.space_id {
            continue;
        }
        let Some(def) = definitions.get(&obj.definition_id) else {
            continue;
        };
        insert_editor_visuals(
            &mut commands.entity(entity),
            &asset_server,
            &mut texture_atlas_layouts,
            def,
            &world_config,
            *tile,
            &editor_camera,
        );
    }
}

// ── Camera ────────────────────────────────────────────────────────────────────

pub fn handle_editor_camera_pan(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    editor_state: Res<EditorState>,
    mut editor_camera: ResMut<EditorCamera>,
    editor_context: Res<EditorContext>,
) {
    if modal_state.active.is_some() || editor_state.palette_filter_focused {
        return;
    }

    let mut delta = Vec2::ZERO;
    if keyboard.pressed(KeyCode::KeyW) || keyboard.pressed(KeyCode::ArrowUp) {
        delta.y += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) || keyboard.pressed(KeyCode::ArrowDown) {
        delta.y -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) || keyboard.pressed(KeyCode::ArrowLeft) {
        delta.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) || keyboard.pressed(KeyCode::ArrowRight) {
        delta.x += 1.0;
    }

    let pan_speed = editor_camera.pan_speed_tiles_per_sec;
    editor_camera.center += delta * pan_speed * time.delta_secs();
    editor_camera.center = editor_camera.center.clamp(
        Vec2::ZERO,
        Vec2::new(
            (editor_context.map_width - 1) as f32,
            (editor_context.map_height - 1) as f32,
        ),
    );
}

/// Atlas texel size assumed for all floor tilesets (matches
/// `default_tile_size_px` in `floor_definitions.rs`). Editor zoom is snapped
/// to integer values of `world.tile_size * zoom_level / ATLAS_TEXEL_PX`; at
/// fractional ratios, nearest-filtered atlas sampling pulls in pixels from
/// neighbouring cells at tile borders ("texture atlas bleeding").
const ATLAS_TEXEL_PX: f32 = 16.0;
const ZOOM_RATIO_MIN: i32 = 1;
const ZOOM_RATIO_MAX: i32 = 12;

fn zoom_to_ratio(zoom: f32, tile_size: f32) -> i32 {
    (tile_size * zoom / ATLAS_TEXEL_PX)
        .round()
        .clamp(ZOOM_RATIO_MIN as f32, ZOOM_RATIO_MAX as f32) as i32
}

fn ratio_to_zoom(ratio: i32, tile_size: f32) -> f32 {
    ratio as f32 * ATLAS_TEXEL_PX / tile_size
}

pub fn handle_editor_zoom(
    mut mouse_wheel: bevy::ecs::message::MessageReader<MouseWheel>,
    modal_state: Res<ModalState>,
    mut editor_camera: ResMut<EditorCamera>,
    world_config: Res<WorldConfig>,
    windows: Query<&Window, With<PrimaryWindow>>,
    palette_root: Query<
        (&bevy::ui::ComputedNode, &bevy::ui::UiGlobalTransform),
        With<crate::editor::ui::palette::EditorPaletteRoot>,
    >,
) {
    if modal_state.active.is_some() {
        return;
    }
    // Don't zoom when the cursor is over the palette panel — there the wheel
    // belongs to the palette scroll handler.
    if let Ok(window) = windows.single() {
        if let Some(cursor) = window.cursor_position() {
            let physical = cursor * window.scale_factor();
            if palette_root
                .iter()
                .any(|(computed, transform)| computed.contains_point(*transform, physical))
            {
                mouse_wheel.clear();
                return;
            }
        }
    }
    let mut ratio = zoom_to_ratio(editor_camera.zoom_level, world_config.tile_size);
    for event in mouse_wheel.read() {
        if event.y > 0.0 {
            ratio += 1;
        } else if event.y < 0.0 {
            ratio -= 1;
        }
    }
    ratio = ratio.clamp(ZOOM_RATIO_MIN, ZOOM_RATIO_MAX);
    editor_camera.zoom_level = ratio_to_zoom(ratio, world_config.tile_size);
}

pub fn sync_tile_transforms_editor(
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut query: Query<
        (
            &SpaceResident,
            &TilePosition,
            &WorldVisual,
            &mut Transform,
            Option<&VisualOffset>,
        ),
        Without<Player>,
    >,
) {
    let effective_size = world_config.tile_size * editor_camera.zoom_level;
    for (space_resident, tile_position, world_visual, mut transform, visual_offset) in &mut query {
        let is_active = space_resident.space_id == editor_context.space_id;
        let z = if !is_active {
            -10_000.0
        } else if world_visual.y_sort {
            let floor = crate::world::components::floor_index(tile_position.z);
            crate::world::systems::y_sort_z(tile_position.x, tile_position.y, floor, 0)
        } else {
            let floor = crate::world::components::floor_index(tile_position.z);
            crate::world::systems::flat_floor_z(world_visual.z_index, floor)
        };
        let bottom_anchored = (world_visual.y_sort || world_visual.block_size > 0)
            && !world_visual.rotation_by_facing;
        let anchor_y_offset = if bottom_anchored {
            -effective_size * 0.5
        } else {
            0.0
        };
        let entity_offset = visual_offset.map_or(Vec2::ZERO, |o| o.current);
        transform.translation = Vec3::new(
            (tile_position.x as f32 - editor_camera.center.x) * effective_size + entity_offset.x,
            (tile_position.y as f32 - editor_camera.center.y) * effective_size
                + anchor_y_offset
                + entity_offset.y,
            z,
        );
        transform.scale = Vec3::splat(editor_camera.zoom_level);
    }
}

fn cursor_to_tile(
    cursor: Vec2,
    window: &Window,
    world_config: &WorldConfig,
    camera: &EditorCamera,
) -> TilePosition {
    let effective_size = world_config.tile_size * camera.zoom_level;
    let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let offset = cursor - center;
    TilePosition::ground(
        (camera.center.x + offset.x / effective_size).round() as i32,
        (camera.center.y - offset.y / effective_size).round() as i32,
    )
}

// ── Mouse drag-pan ───────────────────────────────────────────────────────────

#[derive(Default)]
pub struct DragPanState {
    active: bool,
    last_cursor: Option<Vec2>,
}

/// Middle-mouse-button drag pans the editor camera so the world tracks the
/// cursor 1:1 in tile units. Composes with `handle_editor_camera_pan`
/// (keyboard) — both can run in the same frame. A press that begins over UI
/// is ignored for the full duration of the hold.
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_middle_drag_pan(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_context: Res<EditorContext>,
    mut editor_camera: ResMut<EditorCamera>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    mut state: Local<DragPanState>,
) {
    if !mouse.pressed(MouseButton::Middle) {
        state.active = false;
        state.last_cursor = None;
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        // Cursor left the window mid-hold: pause panning but keep `active` so
        // the drag resumes on re-entry. Clear `last_cursor` so the first
        // re-entry frame produces no spike delta.
        state.last_cursor = None;
        return;
    };

    if mouse.just_pressed(MouseButton::Middle) {
        if panel_roots.cursor_over(cursor, window.scale_factor()) {
            state.active = false;
            state.last_cursor = None;
            return;
        }
        state.active = true;
        state.last_cursor = Some(cursor);
        return;
    }

    if !state.active {
        return;
    }

    let effective = world_config.tile_size * editor_camera.zoom_level;
    if effective <= f32::EPSILON {
        return;
    }
    if let Some(prev) = state.last_cursor {
        let dpx = cursor - prev;
        // `cursor_to_tile` flips screen-y to tile-y (`tile.y = center.y -
        // offset.y / eff`), so to keep the same tile under the cursor on a
        // drag: Δcenter.x = -dpx.x / eff, Δcenter.y = +dpx.y / eff.
        editor_camera.center += Vec2::new(-dpx.x / effective, dpx.y / effective);
        editor_camera.center = editor_camera.center.clamp(
            Vec2::ZERO,
            Vec2::new(
                (editor_context.map_width - 1) as f32,
                (editor_context.map_height - 1) as f32,
            ),
        );
    }
    state.last_cursor = Some(cursor);
}

// ── Tile cursor + ghost preview ──────────────────────────────────────────────

/// Draws a yellow tile-outline at the cursor's tile each frame, plus a
/// translucent ghost of the currently-selected brush (object sprite, or floor
/// debug-color rect) so the user can see what would be placed before
/// committing. Despawn-and-respawn each frame keeps it stateless — tool /
/// selection / camera changes show up immediately on the next frame.
#[allow(clippy::too_many_arguments)]
pub fn update_editor_cursor_ghost(
    mut commands: Commands,
    mut gizmos: Gizmos,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    editor_state: Res<EditorState>,
    definitions: Res<OverworldObjectDefinitions>,
    floor_defs: Res<crate::world::floor_definitions::FloorTilesetDefinitions>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    existing_ghost: Query<Entity, With<crate::editor::resources::EditorCursorMarker>>,
) {
    // Always despawn previous-frame ghosts before any early return so a tool
    // or selection change can't leave a stale sprite behind.
    for entity in &existing_ghost {
        commands.entity(entity).despawn();
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }

    let effective = world_config.tile_size * editor_camera.zoom_level;
    if effective <= f32::EPSILON {
        return;
    }
    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }

    let tile_center = Vec2::new(
        (tile.x as f32 - editor_camera.center.x) * effective,
        (tile.y as f32 - editor_camera.center.y) * effective,
    );

    // When paste mode is active, defer to `render_paste_ghost` (separate
    // system) — it has its own access to the clipboard and avoids pushing
    // this system over Bevy's per-system parameter cap. We also suppress
    // the brush ghost so the two previews don't overlap.
    if editor_state.paste_state.active {
        return;
    }

    let outline_color = if editor_state.current_tool == EditorTool::FloorBrush
        && editor_state.selected_floor_type.is_none()
    {
        Color::srgba(1.0, 0.35, 0.35, 0.9)
    } else {
        Color::srgba(1.0, 1.0, 0.4, 0.9)
    };
    gizmos.rect_2d(
        Isometry2d::from_translation(tile_center),
        Vec2::splat(effective),
        outline_color,
    );

    match editor_state.current_tool {
        EditorTool::Brush => {
            let Some(type_id) = editor_state.selected_type_id.as_ref() else {
                return;
            };
            let Some(def) = definitions.get(type_id) else {
                return;
            };
            let mut bundle = build_object_visual_bundle(
                &asset_server,
                &mut texture_atlas_layouts,
                def,
                &world_config,
                None,
                1,
            );
            bundle.sprite.color = bundle.sprite.color.with_alpha(0.5);
            let bottom_anchored = bundle.anchor.is_some();
            let anchor_y_offset = if bottom_anchored {
                -effective * 0.5
            } else {
                0.0
            };
            // Sit just above any object on the same tile so the ghost is
            // always visible. Y-sort objects use a dynamic z-band, so add to
            // the same band; flat objects use their static z_index.
            let z_base = if def.render.y_sort {
                crate::world::systems::y_sort_z(tile.x, tile.y, tile.z, 0)
            } else {
                crate::world::systems::flat_floor_z(def.render.z_index, tile.z)
            };
            let z = z_base + 50.0;
            let mut entity = commands.spawn((
                crate::editor::resources::EditorCursorMarker,
                bundle.sprite,
                Transform::from_xyz(tile_center.x, tile_center.y + anchor_y_offset, z)
                    .with_scale(Vec3::splat(editor_camera.zoom_level)),
            ));
            if let Some(animated) = bundle.animated {
                entity.insert(animated);
            }
            if let Some(anchor) = bundle.anchor {
                entity.insert(anchor);
            }
        }
        EditorTool::FloorBrush => {
            let Some(id) = editor_state.selected_floor_type.as_ref() else {
                return;
            };
            let Some(def) = floor_defs.get(id) else {
                return;
            };
            let fill = def.debug_color().with_alpha(0.35);
            commands.spawn((
                crate::editor::resources::EditorCursorMarker,
                Sprite::from_color(fill, Vec2::splat(effective * 0.92)),
                Transform::from_xyz(tile_center.x, tile_center.y, 100.0),
            ));
        }
        EditorTool::Portal => {}
        // Select tool has no per-tile ghost; the selection rectangle is
        // drawn elsewhere (`crate::editor::selection::render_selection`).
        EditorTool::Select => {}
        // PickRect mode reuses `render_selection`'s overlay for the live drag
        // rectangle, so no extra ghost needed here.
        EditorTool::PickRect { .. } => {}
        // BuildingDraw reuses the same selection rectangle for its drag
        // preview; per-tile ghost would just add noise.
        EditorTool::BuildingDraw => {}
    }
}

// ── Left / right click ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn handle_editor_left_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut undo_stack: ResMut<UndoStack>,
    mut modal_state: ResMut<ModalState>,
    existing_objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }
    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }

    // Paste-mode commit lives in `handle_editor_paste_click` (separate
    // system) so this fn stays under Bevy's system-param arity limit. If
    // paste is active, it ran first this frame and already handled the
    // click; bail so we don't double-process.
    if editor_state.paste_state.active {
        return;
    }

    // Select tool reads via `handle_editor_select_drag`; left-click here is
    // the start-of-drag and is consumed by the drag system.
    if editor_state.current_tool == EditorTool::Select {
        return;
    }
    // PickRect mode owns the click; let `handle_editor_pick_rect_drag` write
    // the result.
    if matches!(editor_state.current_tool, EditorTool::PickRect { .. }) {
        return;
    }
    // BuildingDraw owns its own click flow: the drag handler captures the
    // rectangle, and `handle_editor_building_door_swap_click` (a separate
    // system that runs *before* this one) swaps a wall for a door when
    // `place_door_armed` is set. Either way, the brush spawn / object-select
    // logic below should not fire while the building tool is active.
    if editor_state.current_tool == EditorTool::BuildingDraw {
        return;
    }

    if editor_state.current_tool == EditorTool::FloorBrush {
        // FloorBrush painting is driven by `handle_editor_floor_brush_drag`,
        // which supports both clicks and drags via `mouse.pressed(...)`. Skip
        // here so the click doesn't paint twice.
        return;
    }

    if editor_state.current_tool == EditorTool::Portal {
        modal_state.active = Some(ModalKind::PortalCreate);
        modal_state.portal_source_tile = Some(tile);
        modal_state.text_fields = vec![
            ModalTextField {
                label: "Portal ID".into(),
                value: String::new(),
                placeholder: "portal_to_dungeon".into(),
                numeric_only: false,
            },
            ModalTextField {
                label: "Destination Space ID".into(),
                value: String::new(),
                placeholder: "starter_cellar".into(),
                numeric_only: false,
            },
            ModalTextField {
                label: "Destination Tile X".into(),
                value: String::new(),
                placeholder: "7".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Destination Tile Y".into(),
                value: String::new(),
                placeholder: "9".into(),
                numeric_only: true,
            },
        ];
        modal_state.focused_field = 0;
        modal_state.error_message = None;
        modal_state.confirm_triggered = false;
        modal_state.confirmed = None;
        return;
    }

    let existing = existing_objects
        .iter()
        .find(|(_, resident, pos)| resident.space_id == editor_context.space_id && **pos == tile);
    if let Some((obj, _, _)) = existing {
        editor_state.selected_object_id = Some(obj.object_id);
        editor_state.selected_type_id = None;
        if let Some(props) = object_registry.properties(obj.object_id) {
            prop_buffer.object_id = Some(obj.object_id);
            prop_buffer.entries = props.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            prop_buffer.entries.sort_by(|a, b| a.0.cmp(&b.0));
        } else {
            prop_buffer.object_id = Some(obj.object_id);
            prop_buffer.entries.clear();
        }
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
        return;
    }

    // Empty tile: drop any single-object selection so Delete/Ctrl+C don't
    // act on a stale object the user has clicked away from. The properties
    // panel reads `selected_object_id` and naturally empties.
    if editor_state.selected_object_id.is_some() {
        editor_state.selected_object_id = None;
        prop_buffer.object_id = None;
        prop_buffer.entries.clear();
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
    }

    let Some(ref type_id) = editor_state.selected_type_id.clone() else {
        return;
    };
    let Some(def) = definitions.get(type_id) else {
        return;
    };

    let object_id = object_registry.allocate_runtime_id(type_id.clone());
    let entity = spawn_overworld_object(
        &mut commands,
        &definitions,
        &object_registry,
        object_id,
        type_id,
        None,
        editor_context.space_id,
        tile,
        None,
    );
    insert_editor_visuals(
        &mut commands.entity(entity),
        &asset_server,
        &mut texture_atlas_layouts,
        def,
        &world_config,
        tile,
        &editor_camera,
    );
    undo_stack.push_undo(UndoOp::Despawn { object_id });
    editor_state.dirty = true;
}

#[allow(clippy::too_many_arguments)]
pub fn handle_editor_right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut undo_stack: ResMut<UndoStack>,
    mut portal_buffer: ResMut<EditorPortalBuffer>,
    objects: Query<(Entity, &OverworldObject, &SpaceResident, &TilePosition)>,
    object_registry: Res<ObjectRegistry>,
    mut commands: Commands,
    panel_roots: crate::editor::ui::EditorPanelRoots,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }

    // Paste-mode cancel always wins so RMB doesn't double as "delete object
    // under cursor" while pasting.
    if editor_state.paste_state.active {
        cancel_paste(&mut editor_state);
        return;
    }

    // PickRect mode: RMB cancels the pick and restores the previous tool
    // without writing a result.
    if matches!(editor_state.current_tool, EditorTool::PickRect { .. }) {
        editor_state.selection = None;
        editor_state.current_tool = editor_state
            .tool_before_pick
            .take()
            .unwrap_or(EditorTool::Brush);
        return;
    }

    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);

    if editor_state.current_tool == EditorTool::FloorBrush {
        // FloorBrush erase is handled by `handle_editor_floor_brush_drag`.
        return;
    }

    if editor_state.current_tool == EditorTool::Portal {
        if let Some(idx) = portal_buffer
            .portals
            .iter()
            .position(|p| p.source.x == tile.x && p.source.y == tile.y)
        {
            let portal = portal_buffer.portals.remove(idx);
            undo_stack.push_undo(UndoOp::AddPortal { portal });
            editor_state.dirty = true;
        }
        return;
    }

    let hit = objects.iter().find(|(_, _, resident, pos)| {
        resident.space_id == editor_context.space_id && **pos == tile
    });
    if let Some((entity, obj, _, _)) = hit {
        let deleted_id = obj.object_id;
        let type_id = object_registry
            .type_id(deleted_id)
            .unwrap_or(&obj.definition_id)
            .to_owned();
        let properties = object_registry
            .properties(deleted_id)
            .cloned()
            .unwrap_or_default();
        let behavior = object_registry.behavior(deleted_id).cloned();
        undo_stack.push_undo(UndoOp::Spawn {
            type_id,
            space_id: editor_context.space_id,
            tile,
            properties,
            behavior,
        });
        commands.entity(entity).despawn();
        if editor_state.selected_object_id == Some(deleted_id) {
            editor_state.selected_object_id = None;
            prop_buffer.object_id = None;
            prop_buffer.entries.clear();
            prop_buffer.editing_index = None;
        }
        editor_state.dirty = true;
    }
}

// ── Floor brush dragging ─────────────────────────────────────────────────────

/// Continuous floor painting while LMB (paint with `selected_floor_type`) or
/// RMB (erase) is held. The cursor often moves multiple tiles per frame on a
/// fast drag, so we Bresenham-interpolate from the last painted tile to the
/// current one and emit one paint command per cell along the way. The
/// `last_painted` local resets on mouse-up or tool change so a fresh drag
/// doesn't draw a line back to wherever the previous drag ended.
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_floor_brush_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut editor_state: ResMut<EditorState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    mut last_painted: Local<Option<TilePosition>>,
) {
    if editor_state.current_tool != EditorTool::FloorBrush {
        *last_painted = None;
        return;
    }
    let left = mouse.pressed(MouseButton::Left);
    let right = mouse.pressed(MouseButton::Right);
    if !left && !right {
        *last_painted = None;
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }
    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);

    // Build the ordered list of tiles to paint this frame: a Bresenham line
    // from the previous cursor tile to the current one, skipping the
    // already-painted previous tile.
    let mut to_paint: Vec<TilePosition> = match *last_painted {
        Some(prev) if prev == tile => return,
        Some(prev) => {
            let mut tiles = bresenham_line_tiles(prev, tile);
            if !tiles.is_empty() {
                tiles.remove(0);
            }
            tiles
        }
        None => vec![tile],
    };
    to_paint.retain(|t| {
        t.x >= 0 && t.y >= 0 && t.x < editor_context.map_width && t.y < editor_context.map_height
    });
    if to_paint.is_empty() {
        // Cursor jumped off-map but we still need to remember where it was so
        // the next on-map sample doesn't draw a line from across the void.
        *last_painted = Some(tile);
        return;
    }

    // LMB takes precedence if both buttons are held — matches typical
    // tile-editor expectations (paint over erase).
    let floor_type = if left {
        editor_state.selected_floor_type.clone()
    } else {
        None
    };
    for t in &to_paint {
        pending_commands.push(GameCommand::EditorSetFloorTile {
            space_id: editor_context.space_id,
            z: TilePosition::GROUND_FLOOR,
            x: t.x,
            y: t.y,
            floor_type: floor_type.clone(),
        });
    }
    editor_state.dirty = true;
    *last_painted = Some(tile);
}

/// Inclusive integer-grid line from `from` to `to` via Bresenham. Used by the
/// floor brush to fill the gaps between two consecutive cursor samples on a
/// fast drag.
fn bresenham_line_tiles(from: TilePosition, to: TilePosition) -> Vec<TilePosition> {
    let mut tiles = Vec::new();
    let mut x = from.x;
    let mut y = from.y;
    let z = from.z;
    let dx = (to.x - x).abs();
    let dy = -(to.y - y).abs();
    let sx = if from.x < to.x { 1 } else { -1 };
    let sy = if from.y < to.y { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        tiles.push(TilePosition { x, y, z });
        if x == to.x && y == to.y {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    tiles
}

// ── Keyboard ──────────────────────────────────────────────────────────────────

pub fn handle_editor_keyboard_input(
    mut keyboard_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut editor_state: ResMut<EditorState>,
) {
    if editor_state.palette_filter_focused {
        for event in keyboard_events.read() {
            if !event.state.is_pressed() {
                continue;
            }
            match event.key_code {
                KeyCode::Escape => {
                    editor_state.palette_filter_focused = false;
                }
                KeyCode::Backspace => {
                    editor_state.palette_filter.pop();
                }
                _ => {
                    if event.repeat {
                        continue;
                    }
                    match &event.logical_key {
                        Key::Character(ch) => {
                            editor_state.palette_filter.push_str(ch.as_str());
                        }
                        Key::Space => {
                            editor_state.palette_filter.push(' ');
                        }
                        _ => {}
                    }
                }
            }
        }
        return;
    }

    let Some(editing_index) = prop_buffer.editing_index else {
        return;
    };
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::Escape => {
                prop_buffer.editing_index = None;
                prop_buffer.edit_text.clear();
            }
            KeyCode::Enter | KeyCode::Tab => {
                commit_edit(
                    &mut prop_buffer,
                    &mut object_registry,
                    &mut editor_state,
                    editing_index,
                );
            }
            KeyCode::Backspace => {
                prop_buffer.edit_text.pop();
            }
            _ => {
                if event.repeat {
                    continue;
                }
                match &event.logical_key {
                    Key::Character(ch) => {
                        prop_buffer.edit_text.push_str(ch.as_str());
                    }
                    Key::Space => {
                        prop_buffer.edit_text.push(' ');
                    }
                    _ => {}
                }
            }
        }
    }
}

fn commit_edit(
    prop_buffer: &mut EditorPropertyEditBuffer,
    object_registry: &mut ObjectRegistry,
    editor_state: &mut EditorState,
    editing_index: usize,
) {
    let text = prop_buffer.edit_text.clone();
    prop_buffer.editing_index = None;
    prop_buffer.edit_text.clear();
    if let Some(entry) = prop_buffer.entries.get_mut(editing_index) {
        match prop_buffer.editing_field {
            EditingField::Value => entry.1 = text,
            EditingField::Key => entry.0 = text,
        }
    }
    if let Some(object_id) = prop_buffer.object_id {
        let props = prop_buffer
            .entries
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        object_registry.set_properties(object_id, props);
        editor_state.dirty = true;
    }
}

pub fn handle_editor_escape(
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
) {
    if !keyboard.just_pressed(KeyCode::Escape) {
        return;
    }
    if modal_state.active.is_some() {
        return;
    }
    if prop_buffer.editing_index.is_some() {
        return;
    }
    if vendor_stash_buffer.editing.is_some() {
        // The vendor-stash keyboard handler owns Esc while a field is being
        // edited; skip the tool/selection-clear cascade in that case.
        return;
    }
    if editor_state.palette_filter_focused {
        return;
    }

    // Esc priority: (1) leave paste mode, (1.5) cancel PickRect mode and
    // restore prior tool, (2) clear marquee selection, (3) drop back to
    // Brush, (4) deselect type, (5) deselect object.
    if editor_state.paste_state.active {
        editor_state.paste_state.active = false;
    } else if matches!(editor_state.current_tool, EditorTool::PickRect { .. }) {
        editor_state.selection = None;
        editor_state.current_tool = editor_state
            .tool_before_pick
            .take()
            .unwrap_or(EditorTool::Brush);
    } else if editor_state.selection.is_some() {
        editor_state.selection = None;
    } else if editor_state.current_tool != EditorTool::Brush {
        editor_state.current_tool = EditorTool::Brush;
    } else if editor_state.selected_type_id.is_some() {
        editor_state.selected_type_id = None;
    } else if editor_state.selected_object_id.is_some() {
        editor_state.selected_object_id = None;
        prop_buffer.object_id = None;
        prop_buffer.entries.clear();
    }
}

/// `F` toggles into FloorBrush mode or cycles the selected floor type:
/// off → grass → sand → cobblestone → cave_floor → dirt_path → clear (None) → off.
/// `B` switches back to the object Brush.
pub fn handle_editor_floor_brush_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    mut editor_state: ResMut<EditorState>,
    floor_defs: Res<crate::world::floor_definitions::FloorTilesetDefinitions>,
) {
    if modal_state.active.is_some() || editor_state.palette_filter_focused {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if ctrl {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyB) {
        editor_state.current_tool = EditorTool::Brush;
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyF) {
        // Collect floor ids in priority order for a stable cycle.
        let mut ids: Vec<(i32, String)> = floor_defs
            .iter()
            .map(|d| (d.priority, d.id.clone()))
            .collect();
        ids.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let cycle: Vec<String> = ids.into_iter().map(|(_, id)| id).collect();

        if editor_state.current_tool != EditorTool::FloorBrush {
            editor_state.current_tool = EditorTool::FloorBrush;
            editor_state.selected_floor_type = cycle.first().cloned();
            return;
        }

        let next = match editor_state.selected_floor_type.as_ref() {
            None => cycle.first().cloned(),
            Some(current) => {
                let i = cycle.iter().position(|id| id == current);
                match i {
                    Some(idx) if idx + 1 < cycle.len() => Some(cycle[idx + 1].clone()),
                    _ => None, // wrap to "clear" mode (right-click equivalent)
                }
            }
        };
        editor_state.selected_floor_type = next;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn handle_editor_save(
    keyboard: Res<ButtonInput<KeyCode>>,
    modal_state: Res<ModalState>,
    mut editor_state: ResMut<EditorState>,
    editor_context: Res<EditorContext>,
    portal_buffer: Res<EditorPortalBuffer>,
    spawn_group_buffer: Res<EditorSpawnGroupBuffer>,
    lighting_buffer: Res<EditorLightingBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    object_registry: Res<ObjectRegistry>,
    floor_maps: Res<FloorMaps>,
    objects: Query<
        (&OverworldObject, &SpaceResident, &TilePosition),
        (Without<SpawnGroupMember>, Without<Player>),
    >,
    mut space_definitions: ResMut<SpaceDefinitions>,
) {
    if modal_state.active.is_some() {
        return;
    }
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if ctrl && !shift && keyboard.just_pressed(KeyCode::KeyS) {
        serialize_and_save(
            &editor_context,
            &portal_buffer,
            &spawn_group_buffer,
            &lighting_buffer,
            &vendor_stash_buffer,
            &object_registry,
            &objects,
            &floor_maps,
        );
        space_definitions.load_single_from_disk(&editor_context.authored_id);
        editor_state.dirty = false;
        info!("Saved map '{}'", editor_context.authored_id);
    }
}

// ── Dialog openers (keyboard shortcuts) ──────────────────────────────────────

pub fn open_file_dialog_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_context: Res<EditorContext>,
    mut modal_state: ResMut<ModalState>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl || !keyboard.just_pressed(KeyCode::KeyO) || modal_state.active.is_some() {
        return;
    }
    open_file_dialog_impl(&editor_context, &mut modal_state);
}

pub fn open_save_as_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_context: Res<EditorContext>,
    mut modal_state: ResMut<ModalState>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if !(ctrl && shift && keyboard.just_pressed(KeyCode::KeyS)) || modal_state.active.is_some() {
        return;
    }
    open_save_as_impl(&editor_context, &mut modal_state);
}

pub fn open_file_dialog_impl(editor_context: &EditorContext, modal_state: &mut ModalState) {
    let mut items: Vec<String> = std::fs::read_dir("assets/maps")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("yaml") {
                        p.file_stem()?.to_str().map(|s| s.to_owned())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort();
    let selected = items.iter().position(|s| s == &editor_context.authored_id);
    *modal_state = ModalState {
        active: Some(ModalKind::FileOpen),
        list_items: items,
        selected_list_item: selected,
        ..default()
    };
}

pub fn open_save_as_impl(editor_context: &EditorContext, modal_state: &mut ModalState) {
    *modal_state = ModalState {
        active: Some(ModalKind::SaveAs),
        text_fields: vec![ModalTextField {
            label: "Map ID".into(),
            value: editor_context.authored_id.clone(),
            placeholder: "my_map".into(),
            numeric_only: false,
        }],
        ..default()
    };
}

pub fn open_new_map_dialog_impl(
    modal_state: &mut ModalState,
    floor_defs: &FloorTilesetDefinitions,
) {
    let picker = build_floor_picker("Floor Fill", floor_defs, true, Some("grass"));
    *modal_state = ModalState {
        active: Some(ModalKind::NewMap),
        text_fields: vec![
            ModalTextField {
                label: "Map ID".into(),
                value: String::new(),
                placeholder: "my_dungeon".into(),
                numeric_only: false,
            },
            ModalTextField {
                label: "Width".into(),
                value: String::new(),
                placeholder: "32".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Height".into(),
                value: String::new(),
                placeholder: "24".into(),
                numeric_only: true,
            },
        ],
        picker_fields: vec![picker],
        ..default()
    };
}

/// Read the selected `id` from a picker at `index` (None if no selection or
/// the picker doesn't exist). Used by `process_modal_confirm`.
fn picker_value(pickers: &[ModalPickerField], index: usize) -> Option<String> {
    pickers
        .get(index)
        .and_then(|p| p.options.get(p.selected))
        .and_then(|opt| opt.id.clone())
}

/// Build a floor picker with an optional `(No fill)` sentinel at index 0.
/// Floors are sorted by priority then id (mirrors the left-side palette).
fn build_floor_picker(
    label: &str,
    floor_defs: &FloorTilesetDefinitions,
    include_none: bool,
    default_id: Option<&str>,
) -> ModalPickerField {
    let mut options: Vec<ModalPickerOption> = Vec::new();
    if include_none {
        options.push(ModalPickerOption {
            id: None,
            label: "(No fill)".into(),
            swatch: Color::srgba(0.0, 0.0, 0.0, 0.0),
        });
    }
    let mut sorted: Vec<&crate::world::floor_definitions::FloorTilesetDefinition> =
        floor_defs.iter().collect();
    sorted.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
    for def in sorted {
        options.push(ModalPickerOption {
            id: Some(def.id.clone()),
            label: def.name.clone(),
            swatch: def.debug_color(),
        });
    }
    let selected = default_id
        .and_then(|target| options.iter().position(|o| o.id.as_deref() == Some(target)))
        .unwrap_or(0);
    ModalPickerField {
        label: label.into(),
        options,
        selected,
    }
}

/// Build an object picker. Used for `Wall Type` in Generate Dungeon — there's
/// no "(None)" sentinel since the generator requires a concrete object.
fn build_object_picker(
    label: &str,
    object_defs: &OverworldObjectDefinitions,
    default_id: Option<&str>,
) -> ModalPickerField {
    let mut ids: Vec<&str> = object_defs.ids().collect();
    ids.sort();
    let options: Vec<ModalPickerOption> = ids
        .iter()
        .filter_map(|id| {
            object_defs.get(id).map(|def| ModalPickerOption {
                id: Some((*id).to_owned()),
                label: def.name.clone(),
                swatch: def.debug_color(),
            })
        })
        .collect();
    let selected = default_id
        .and_then(|target| options.iter().position(|o| o.id.as_deref() == Some(target)))
        .unwrap_or(0);
    ModalPickerField {
        label: label.into(),
        options,
        selected,
    }
}

pub fn open_generate_dungeon_dialog_impl(
    modal_state: &mut ModalState,
    floor_defs: &FloorTilesetDefinitions,
    object_defs: &OverworldObjectDefinitions,
) {
    let wall_picker = build_object_picker("Wall Type", object_defs, Some("wall_s"));
    let chamber_picker =
        build_floor_picker("Chamber Floor", floor_defs, false, Some("cobblestone"));
    let corridor_picker =
        build_floor_picker("Corridor Floor", floor_defs, false, Some("dirt_path"));
    *modal_state = ModalState {
        active: Some(ModalKind::GenerateDungeon),
        text_fields: vec![
            ModalTextField {
                label: "Map ID".into(),
                value: String::new(),
                placeholder: "my_dungeon".into(),
                numeric_only: false,
            },
            ModalTextField {
                label: "Width".into(),
                value: String::new(),
                placeholder: "64".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Height".into(),
                value: String::new(),
                placeholder: "48".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Target Rooms".into(),
                value: String::new(),
                placeholder: "8".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Room Padding".into(),
                value: String::new(),
                placeholder: "4".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Corridor Wander 0-100".into(),
                value: String::new(),
                placeholder: "55".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Branching 0-100".into(),
                value: String::new(),
                placeholder: "50".into(),
                numeric_only: true,
            },
            ModalTextField {
                label: "Seed (blank = random)".into(),
                value: String::new(),
                placeholder: "".into(),
                numeric_only: true,
            },
        ],
        picker_fields: vec![wall_picker, chamber_picker, corridor_picker],
        ..default()
    };
}

// ── Modal confirm processing ──────────────────────────────────────────────────

pub fn process_modal_confirm(
    mut modal_state: ResMut<ModalState>,
    editor_state: Res<EditorState>,
    definitions: Res<OverworldObjectDefinitions>,
) {
    if !modal_state.confirm_triggered {
        return;
    }
    modal_state.confirm_triggered = false;

    let Some(kind) = modal_state.active else {
        return;
    };
    match kind {
        ModalKind::FileOpen => {
            let Some(idx) = modal_state.selected_list_item else {
                modal_state.error_message = Some("Select a map first.".into());
                return;
            };
            let Some(authored_id) = modal_state.list_items.get(idx).cloned() else {
                return;
            };
            if editor_state.dirty && modal_state.error_message.is_none() {
                modal_state.error_message =
                    Some("Unsaved changes - click Open again to discard.".into());
                return;
            }
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::FileOpen { authored_id });
        }
        ModalKind::SaveAs => {
            let authored_id = modal_state
                .text_fields
                .first()
                .map(|f| f.value.trim().to_owned())
                .unwrap_or_default();
            if authored_id.is_empty() {
                modal_state.error_message = Some("Map ID cannot be empty.".into());
                return;
            }
            if !authored_id.chars().all(|c| c.is_alphanumeric() || c == '_') {
                modal_state.error_message =
                    Some("Map ID: letters, digits, underscores only.".into());
                return;
            }
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::SaveAs { authored_id });
        }
        ModalKind::NewMap => {
            let vals: Vec<String> = modal_state
                .text_fields
                .iter()
                .map(|f| f.value.trim().to_owned())
                .collect();
            let authored_id = vals.first().cloned().unwrap_or_default();
            if authored_id.is_empty()
                || !authored_id.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                modal_state.error_message = Some("Map ID must be non-empty alphanumeric.".into());
                return;
            }
            let width: i32 = match vals.get(1).and_then(|s| s.parse().ok()) {
                Some(v) if v > 0 && v <= 256 => v,
                _ => {
                    modal_state.error_message = Some("Width must be 1–256.".into());
                    return;
                }
            };
            let height: i32 = match vals.get(2).and_then(|s| s.parse().ok()) {
                Some(v) if v > 0 && v <= 256 => v,
                _ => {
                    modal_state.error_message = Some("Height must be 1–256.".into());
                    return;
                }
            };
            // Floor fill picker — empty string id = "(No fill)" → blank map.
            let fill_type = modal_state
                .picker_fields
                .first()
                .and_then(|p| p.options.get(p.selected))
                .and_then(|opt| opt.id.clone())
                .unwrap_or_default();
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::NewMap {
                authored_id,
                width,
                height,
                fill_type,
            });
        }
        ModalKind::GenerateDungeon => {
            let vals: Vec<String> = modal_state
                .text_fields
                .iter()
                .map(|f| f.value.trim().to_owned())
                .collect();
            let authored_id = vals.first().cloned().unwrap_or_default();
            if authored_id.is_empty()
                || !authored_id.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                modal_state.error_message = Some("Map ID must be non-empty alphanumeric.".into());
                return;
            }
            let width: i32 = match vals.get(1).map(|s| s.as_str()) {
                Some("") | None => 64,
                Some(s) => match s.parse::<i32>() {
                    Ok(v) if v > 0 && v <= 256 => v,
                    _ => {
                        modal_state.error_message = Some("Width must be 1–256.".into());
                        return;
                    }
                },
            };
            let height: i32 = match vals.get(2).map(|s| s.as_str()) {
                Some("") | None => 48,
                Some(s) => match s.parse::<i32>() {
                    Ok(v) if v > 0 && v <= 256 => v,
                    _ => {
                        modal_state.error_message = Some("Height must be 1–256.".into());
                        return;
                    }
                },
            };
            // Pickers (wall / chamber / corridor) are seeded with valid ids so
            // selections can't be unknown — fall back to the legacy default if
            // the picker is somehow empty.
            let wall_type =
                picker_value(&modal_state.picker_fields, 0).unwrap_or_else(|| "wall_s".into());
            let chamber_floor =
                picker_value(&modal_state.picker_fields, 1).unwrap_or_else(|| "cobblestone".into());
            let corridor_floor =
                picker_value(&modal_state.picker_fields, 2).unwrap_or_else(|| "dirt_path".into());
            let target_rooms: u32 = match vals.get(3).map(|s| s.as_str()) {
                Some("") | None => 8,
                Some(s) => match s.parse::<u32>() {
                    Ok(v) if (1..=200).contains(&v) => v,
                    _ => {
                        modal_state.error_message = Some("Target Rooms must be 1–200.".into());
                        return;
                    }
                },
            };
            let room_padding: i32 = match vals.get(4).map(|s| s.as_str()) {
                Some("") | None => 4,
                Some(s) => match s.parse::<i32>() {
                    Ok(v) if (0..=32).contains(&v) => v,
                    _ => {
                        modal_state.error_message = Some("Room Padding must be 0–32.".into());
                        return;
                    }
                },
            };
            let wander_pct: i32 = match vals.get(5).map(|s| s.as_str()) {
                Some("") | None => 55,
                Some(s) => match s.parse::<i32>() {
                    Ok(v) if (0..=100).contains(&v) => v,
                    _ => {
                        modal_state.error_message = Some("Corridor Wander must be 0–100.".into());
                        return;
                    }
                },
            };
            let branch_pct: i32 = match vals.get(6).map(|s| s.as_str()) {
                Some("") | None => 50,
                Some(s) => match s.parse::<i32>() {
                    Ok(v) if (0..=100).contains(&v) => v,
                    _ => {
                        modal_state.error_message = Some("Branching must be 0–100.".into());
                        return;
                    }
                },
            };
            let seed: Option<u64> = match vals.get(7).map(|s| s.as_str()) {
                Some("") | None => None,
                Some(s) => match s.parse::<u64>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        modal_state.error_message = Some("Seed must be a number.".into());
                        return;
                    }
                },
            };
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::GenerateDungeon {
                authored_id,
                width,
                height,
                wall_type,
                chamber_floor,
                corridor_floor,
                target_rooms,
                room_padding,
                corridor_wander: wander_pct as f32 / 100.0,
                branch_factor: branch_pct as f32 / 100.0,
                seed,
            });
        }
        ModalKind::PortalCreate => {
            let vals: Vec<String> = modal_state
                .text_fields
                .iter()
                .map(|f| f.value.trim().to_owned())
                .collect();
            let id = vals.first().cloned().unwrap_or_default();
            let dest_space_id = vals.get(1).cloned().unwrap_or_default();
            let dest_tile_x: i32 = vals.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let dest_tile_y: i32 = vals.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            if id.is_empty() {
                modal_state.error_message = Some("Portal ID required.".into());
                return;
            }
            if dest_space_id.is_empty() {
                modal_state.error_message = Some("Destination Space ID required.".into());
                return;
            }
            let Some(source_tile) = modal_state.portal_source_tile else {
                return;
            };
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::PortalCreate {
                source_tile,
                id,
                dest_space_id,
                dest_tile_x,
                dest_tile_y,
            });
        }
        ModalKind::SaveAsTemplate => {
            let name = modal_state
                .text_fields
                .first()
                .map(|f| f.value.trim().to_owned())
                .unwrap_or_default();
            if name.is_empty() {
                modal_state.error_message = Some("Template name cannot be empty.".into());
                return;
            }
            if !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                modal_state.error_message =
                    Some("Template name: letters, digits, '_' or '-' only.".into());
                return;
            }
            modal_state.active = None;
            modal_state.error_message = None;
            modal_state.confirmed = Some(ModalConfirmed::SaveAsTemplate { name });
        }
        ModalKind::SpawnGroupEdit { editing_index } => {
            let Some(draft) = modal_state.spawn_group_draft.clone() else {
                modal_state.active = None;
                return;
            };
            match build_spawn_group_from_draft(&draft, &definitions) {
                Ok(group) => {
                    modal_state.active = None;
                    modal_state.error_message = None;
                    modal_state.spawn_group_draft = None;
                    modal_state.confirmed_spawn_group = Some(ConfirmedSpawnGroup {
                        editing_index,
                        group,
                    });
                }
                Err(msg) => {
                    modal_state.error_message = Some(msg);
                }
            }
        }
        ModalKind::LightingKeyframeEdit { editing_index } => {
            let Some(draft) = modal_state.lighting_keyframe_draft.clone() else {
                modal_state.active = None;
                return;
            };
            match build_keyframe_from_draft(&draft) {
                Ok(keyframe) => {
                    modal_state.active = None;
                    modal_state.error_message = None;
                    modal_state.lighting_keyframe_draft = None;
                    modal_state.confirmed_lighting_keyframe = Some(ConfirmedLightingKeyframe {
                        editing_index,
                        keyframe,
                    });
                }
                Err(msg) => {
                    modal_state.error_message = Some(msg);
                }
            }
        }
    }
}

/// Drains `ModalState.confirmed_spawn_group` and applies the edit/create to
/// `EditorSpawnGroupBuffer`. Lives in its own system so `apply_modal_confirmed`
/// doesn't have to take another `ResMut` and blow past Bevy's per-system
/// parameter cap.
pub fn apply_spawn_group_confirmed(
    mut modal_state: ResMut<ModalState>,
    mut spawn_group_buffer: ResMut<EditorSpawnGroupBuffer>,
    mut undo_stack: ResMut<UndoStack>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(confirmed) = modal_state.confirmed_spawn_group.take() else {
        return;
    };
    let ConfirmedSpawnGroup {
        editing_index,
        group,
    } = confirmed;
    match editing_index {
        Some(idx) if idx < spawn_group_buffer.groups.len() => {
            let before = spawn_group_buffer.groups[idx].clone();
            spawn_group_buffer.groups[idx] = group;
            undo_stack.push_undo(UndoOp::EditSpawnGroup { index: idx, before });
        }
        _ => {
            let idx = spawn_group_buffer.groups.len();
            spawn_group_buffer.groups.push(group);
            undo_stack.push_undo(UndoOp::RemoveSpawnGroup { index: idx });
            spawn_group_buffer.selected = Some(idx);
        }
    }
    editor_state.dirty = true;
}

/// Drains `ModalState.confirmed_lighting_keyframe` and applies the edit/insert
/// into `EditorLightingBuffer.config.outdoor_curve`, then re-sorts by `time`.
pub fn apply_lighting_keyframe_confirmed(
    mut modal_state: ResMut<ModalState>,
    mut lighting_buffer: ResMut<EditorLightingBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(confirmed) = modal_state.confirmed_lighting_keyframe.take() else {
        return;
    };
    let ConfirmedLightingKeyframe {
        editing_index,
        keyframe,
    } = confirmed;
    match editing_index {
        Some(idx) if idx < lighting_buffer.config.outdoor_curve.len() => {
            lighting_buffer.config.outdoor_curve[idx] = keyframe;
        }
        _ => {
            lighting_buffer.config.outdoor_curve.push(keyframe);
        }
    }
    lighting_buffer.config.outdoor_curve.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    lighting_buffer.selected_keyframe = None;
    editor_state.dirty = true;
}

fn build_keyframe_from_draft(draft: &LightingKeyframeDraft) -> Result<AmbientKeyframe, String> {
    let time: f32 = draft
        .time
        .trim()
        .parse()
        .map_err(|_| "time must be a number".to_owned())?;
    if !time.is_finite() || !(0.0..=1.0).contains(&time) {
        return Err("time must be in [0.0, 1.0].".into());
    }
    let parse_u8 = |s: &str, label: &str| -> Result<u8, String> {
        let v: i32 = s
            .trim()
            .parse()
            .map_err(|_| format!("{label} must be an integer 0–255."))?;
        if !(0..=255).contains(&v) {
            return Err(format!("{label} must be in 0–255."));
        }
        Ok(v as u8)
    };
    let r = parse_u8(&draft.r, "R")?;
    let g = parse_u8(&draft.g, "G")?;
    let b = parse_u8(&draft.b, "B")?;
    let alpha: f32 = draft
        .alpha
        .trim()
        .parse()
        .map_err(|_| "alpha must be a number".to_owned())?;
    if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
        return Err("alpha must be in [0.0, 1.0].".into());
    }
    Ok(AmbientKeyframe {
        time,
        color: [r, g, b],
        alpha,
    })
}

/// Mirror `EditorLightingBuffer` into both the server-side `SpaceDefinitions`
/// and the client-side `ClientGameState.current_space.lighting` so the
/// darkness shader (`update_darkness_overlay`) picks up edits in real time.
///
/// Bypasses `PendingGameEvents` deliberately — the editor is single-user and
/// every keyframe drag would otherwise emit a full event round-trip.
pub fn sync_editor_lighting_to_world(
    lighting_buffer: Res<EditorLightingBuffer>,
    editor_context: Res<EditorContext>,
    mut space_definitions: ResMut<SpaceDefinitions>,
    mut space_manager: ResMut<crate::world::resources::SpaceManager>,
    mut client_state: ResMut<crate::game::resources::ClientGameState>,
) {
    if !lighting_buffer.is_changed() {
        return;
    }
    if let Some(def) = space_definitions
        .spaces
        .get_mut(&editor_context.authored_id)
    {
        def.lighting = lighting_buffer.config.clone();
    }
    if let Some(runtime) = space_manager.spaces.get_mut(&editor_context.space_id) {
        runtime.lighting = lighting_buffer.config.clone();
    }
    if let Some(current) = client_state.current_space.as_mut() {
        if current.space_id == editor_context.space_id {
            current.lighting = lighting_buffer.config.clone();
        }
    }
}

/// Ensure `ClientGameState.current_space` is populated when the editor opens
/// so the darkness overlay has a `lighting` config to read. The user can
/// reach the editor straight from the title screen, in which case no game
/// events have run and `current_space` is `None`.
pub fn init_editor_client_space(
    editor_context: Res<EditorContext>,
    space_definitions: Res<SpaceDefinitions>,
    space_manager: Res<SpaceManager>,
    world_config: Res<WorldConfig>,
    mut client_state: ResMut<crate::game::resources::ClientGameState>,
) {
    let needs_init = client_state
        .current_space
        .as_ref()
        .is_none_or(|s| s.space_id != editor_context.space_id);
    if !needs_init {
        return;
    }
    let lighting = space_definitions
        .get(&editor_context.authored_id)
        .map(|d| d.lighting.clone())
        .unwrap_or_default();
    let (width, height, fill_floor_type) = space_manager
        .get(editor_context.space_id)
        .map(|s| (s.width, s.height, s.fill_floor_type.clone()))
        .unwrap_or((
            world_config.map_width,
            world_config.map_height,
            world_config.fill_floor_type.clone(),
        ));
    client_state.current_space = Some(crate::game::resources::ClientSpaceState {
        space_id: editor_context.space_id,
        authored_id: editor_context.authored_id.clone(),
        width,
        height,
        fill_floor_type,
        lighting,
    });
}

/// Bridges that let the gameplay-side darkness overlay run inside the editor:
/// mirror the scrubber-driven `WorldClock.time_of_day` into
/// `client_state.world_time` (the shader reads the replicated copy), and the
/// editor camera into `client_state.player_position` (the shader uses it to
/// anchor the indoor-mask window — in editor mode the window follows the
/// authoring camera). Gated to `MapEditor` by the plugin registration so
/// gameplay's authoritative writes are untouched.
pub fn sync_editor_view_to_client(
    editor_context: Res<EditorContext>,
    editor_camera: Res<EditorCamera>,
    world_clock: Res<crate::world::lighting::WorldClock>,
    mut client_state: ResMut<crate::game::resources::ClientGameState>,
) {
    if world_clock.is_changed() && client_state.world_time != world_clock.time_of_day {
        client_state.world_time = world_clock.time_of_day;
    }
    if editor_camera.is_changed() || editor_context.is_changed() {
        let tile = crate::world::components::TilePosition::ground(
            editor_camera.center.x.round() as i32,
            editor_camera.center.y.round() as i32,
        );
        let pos = crate::world::components::SpacePosition::new(editor_context.space_id, tile);
        if client_state.player_position != Some(pos) {
            client_state.player_position = Some(pos);
            client_state.player_tile_position = Some(tile);
        }
    }
}

/// Validate and convert a `SpawnGroupDraft` into an authoritative
/// `SpawnGroupDef`. Mirrors `SpaceDefinition::validate_spawn_groups` so the
/// editor never produces YAML that would panic the loader.
fn build_spawn_group_from_draft(
    draft: &SpawnGroupDraft,
    definitions: &OverworldObjectDefinitions,
) -> Result<SpawnGroupDef, String> {
    let id = draft.id.trim().to_owned();
    if id.is_empty() {
        return Err("Spawn group id cannot be empty.".into());
    }
    if !id.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Spawn group id: letters, digits, underscores only.".into());
    }
    let template = draft.template.trim().to_owned();
    if template.is_empty() {
        return Err("Template cannot be empty.".into());
    }
    if definitions.get(&template).is_none() {
        return Err(format!("Unknown template '{template}'."));
    }
    let max_count: u32 = draft
        .max_count
        .trim()
        .parse()
        .map_err(|_| "max_count must be a positive integer.".to_owned())?;
    if max_count == 0 {
        return Err("max_count must be > 0.".into());
    }
    let respawn_mean_seconds: f32 = draft
        .respawn_mean_seconds
        .trim()
        .parse()
        .map_err(|_| "respawn_mean_seconds must be a positive number.".to_owned())?;
    if !respawn_mean_seconds.is_finite() || respawn_mean_seconds <= 0.0 {
        return Err("respawn_mean_seconds must be > 0.".into());
    }
    let area = match draft.area_kind {
        SpawnAreaKind::Bounds => {
            let rect = parse_rect(
                "area",
                &draft.area_min_x,
                &draft.area_min_y,
                &draft.area_max_x,
                &draft.area_max_y,
            )?;
            SpawnArea {
                bounds: Some(rect),
                tiles: None,
            }
        }
        SpawnAreaKind::Tiles => {
            if draft.area_tiles.is_empty() {
                return Err(
                    "Tiles list is empty (v1 cannot create new tile lists; switch to Bounds)."
                        .into(),
                );
            }
            SpawnArea {
                bounds: None,
                tiles: Some(
                    draft
                        .area_tiles
                        .iter()
                        .map(|t| TileCoordinate {
                            x: t.x,
                            y: t.y,
                            z: t.z,
                        })
                        .collect(),
                ),
            }
        }
    };
    let bhv_rect = parse_rect(
        "behavior bounds",
        &draft.bhv_min_x,
        &draft.bhv_min_y,
        &draft.bhv_max_x,
        &draft.bhv_max_y,
    )?;
    let behavior = match draft.behavior_kind {
        BehaviorKind::Roam => MapBehavior::Roam { bounds: bhv_rect },
        BehaviorKind::RoamAndChase => MapBehavior::RoamAndChase { bounds: bhv_rect },
    };
    Ok(SpawnGroupDef {
        id,
        template,
        max_count,
        respawn_mean_seconds,
        area,
        behavior,
    })
}

fn parse_rect(
    label: &str,
    min_x: &str,
    min_y: &str,
    max_x: &str,
    max_y: &str,
) -> Result<TileRectangle, String> {
    let mx: i32 = min_x
        .trim()
        .parse()
        .map_err(|_| format!("{label}: min_x must be an integer."))?;
    let my: i32 = min_y
        .trim()
        .parse()
        .map_err(|_| format!("{label}: min_y must be an integer."))?;
    let xx: i32 = max_x
        .trim()
        .parse()
        .map_err(|_| format!("{label}: max_x must be an integer."))?;
    let yy: i32 = max_y
        .trim()
        .parse()
        .map_err(|_| format!("{label}: max_y must be an integer."))?;
    if mx > xx || my > yy {
        return Err(format!("{label}: empty bounds (min > max)."));
    }
    Ok(TileRectangle {
        min_x: mx,
        min_y: my,
        max_x: xx,
        max_y: yy,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn apply_modal_confirmed(
    mut modal_state: ResMut<ModalState>,
    mut view: EditorViewState,
    mut space_manager: ResMut<SpaceManager>,
    mut floor_maps: ResMut<FloorMaps>,
    mut space_definitions: ResMut<SpaceDefinitions>,
    mut buffers: EditorMapBuffers,
    mut undo_stack: ResMut<UndoStack>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut templates_index: ResMut<EditorTemplatesIndex>,
    object_definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    objects_save: Query<
        (&OverworldObject, &SpaceResident, &TilePosition),
        (Without<SpawnGroupMember>, Without<Player>),
    >,
    mut reset_deps: EditorSpaceResetDeps,
    mut commands: Commands,
) {
    let editor_context = view.context.as_mut();
    let editor_state = view.state.as_mut();
    let editor_camera = view.camera.as_mut();
    let world_config = view.world_config.as_mut();
    let Some(confirmed) = modal_state.confirmed.take() else {
        return;
    };
    let portal_buffer = buffers.portals.as_mut();
    let spawn_group_buffer = buffers.spawn_groups.as_mut();
    let lighting_buffer = buffers.lighting.as_mut();
    let vendor_stash_buffer = buffers.vendor_stashes.as_mut();

    match confirmed {
        ModalConfirmed::FileOpen { authored_id } => {
            // Re-load from disk so the in-memory `SpaceDefinitions` matches the
            // file (and not, e.g., a stale entry from a prior session).
            if !space_definitions.load_single_from_disk(&authored_id) {
                warn!("Could not load map '{authored_id}' from disk");
                return;
            }
            let Some(def) = space_definitions.get(&authored_id).cloned() else {
                return;
            };

            let space_id = if let Some(id) = space_manager.persistent_space_id(&authored_id) {
                // Space already exists — wipe its contents and re-spawn from
                // the fresh definition so the editor view matches the YAML.
                reset_space_contents_from_def(
                    &mut commands,
                    id,
                    &def,
                    &object_definitions,
                    &object_registry,
                    &mut floor_maps,
                    &mut reset_deps.spawn_group_registry,
                    &reset_deps.residents,
                    &reset_deps.portal_markers,
                );
                id
            } else {
                instantiate_space(
                    &mut commands,
                    &mut space_manager,
                    &mut floor_maps,
                    &def,
                    &object_definitions,
                    &object_registry,
                    None,
                    def.permanence,
                )
            };

            editor_context.space_id = space_id;
            editor_context.authored_id = authored_id.clone();
            editor_context.map_width = def.width;
            editor_context.map_height = def.height;
            editor_context.fill_floor_type = def.fill_floor_type.clone();
            world_config.current_space_id = space_id;
            world_config.map_width = def.width;
            world_config.map_height = def.height;
            world_config.fill_floor_type = def.fill_floor_type.clone();
            editor_camera.center = Vec2::new(def.width as f32 * 0.5, def.height as f32 * 0.5);
            portal_buffer.portals = def.portals.clone();
            spawn_group_buffer.groups = def.spawn_groups.clone();
            spawn_group_buffer.selected = None;
            lighting_buffer.config = def.lighting.clone();
            lighting_buffer.selected_keyframe = None;
            vendor_stash_buffer.stashes = def.vendor_stashes.clone();
            vendor_stash_buffer.selected = None;
            vendor_stash_buffer.editing = None;
            vendor_stash_buffer.edit_text.clear();
            vendor_stash_buffer.pending_ware_pick = None;
            editor_state.dirty = false;
            editor_state.selected_type_id = None;
            editor_state.selected_object_id = None;
            editor_state.current_tool = EditorTool::Brush;
            prop_buffer.object_id = None;
            prop_buffer.entries.clear();
            prop_buffer.editing_index = None;
            undo_stack.clear();
        }
        ModalConfirmed::SaveAs { authored_id } => {
            editor_context.authored_id = authored_id.clone();
            serialize_and_save(
                &editor_context,
                portal_buffer,
                spawn_group_buffer,
                lighting_buffer,
                vendor_stash_buffer,
                &object_registry,
                &objects_save,
                &floor_maps,
            );
            space_definitions.load_single_from_disk(&authored_id);
            editor_state.dirty = false;
            info!("Saved map as '{authored_id}'");
        }
        ModalConfirmed::NewMap {
            authored_id,
            width,
            height,
            fill_type,
        } => {
            let new_space_id = space_manager.allocate_space_id();
            space_manager.insert_space(RuntimeSpace {
                id: new_space_id,
                authored_id: authored_id.clone(),
                width,
                height,
                fill_floor_type: fill_type.clone(),
                permanence: crate::world::map_layout::SpacePermanence::Persistent,
                instance_owner: None,
                lighting: crate::world::map_layout::SpaceLightingDef::default(),
            });
            let new_def = crate::world::map_layout::SpaceDefinition::new_empty(
                authored_id.clone(),
                width,
                height,
                fill_type.clone(),
            );
            floor_maps.insert(
                new_space_id,
                crate::world::components::TilePosition::GROUND_FLOOR,
                new_def.build_floor_map(crate::world::components::TilePosition::GROUND_FLOOR),
            );
            space_definitions.insert_or_replace(new_def);
            editor_context.space_id = new_space_id;
            editor_context.authored_id = authored_id.clone();
            editor_context.map_width = width;
            editor_context.map_height = height;
            editor_context.fill_floor_type = fill_type.clone();
            world_config.current_space_id = new_space_id;
            world_config.map_width = width;
            world_config.map_height = height;
            world_config.fill_floor_type = fill_type.clone();
            editor_camera.center = Vec2::new(width as f32 * 0.5, height as f32 * 0.5);
            portal_buffer.portals = vec![];
            spawn_group_buffer.groups.clear();
            spawn_group_buffer.selected = None;
            spawn_group_buffer.pending_new_spawn_group_template = None;
            lighting_buffer.config = crate::world::map_layout::SpaceLightingDef::default();
            lighting_buffer.selected_keyframe = None;
            vendor_stash_buffer.stashes.clear();
            vendor_stash_buffer.selected = None;
            vendor_stash_buffer.editing = None;
            vendor_stash_buffer.edit_text.clear();
            vendor_stash_buffer.pending_ware_pick = None;
            editor_state.dirty = true;
            editor_state.selected_type_id = None;
            editor_state.selected_object_id = None;
            editor_state.current_tool = EditorTool::Brush;
            prop_buffer.object_id = None;
            prop_buffer.entries.clear();
            prop_buffer.editing_index = None;
            undo_stack.clear();
        }
        ModalConfirmed::GenerateDungeon {
            authored_id,
            width,
            height,
            wall_type,
            chamber_floor,
            corridor_floor,
            target_rooms,
            room_padding,
            corridor_wander,
            branch_factor,
            seed,
        } => {
            // The Branching dial drives both extra room-to-room loops *and*
            // dead-end spurs off main corridors. Mapping it to both with
            // slightly different curves keeps a single user-facing knob while
            // hitting both branching mechanisms.
            let extra_corridor_ratio = (branch_factor * 0.8).clamp(0.0, 1.0);
            let params = crate::world::dungeon_gen::DungeonParams {
                width,
                height,
                wall_type_id: wall_type,
                chamber_floor,
                corridor_floor,
                // Empty = non-dungeon tiles render as black void.
                fill_floor_type: String::new(),
                target_rooms,
                min_room_size: 4,
                max_room_size: 7,
                room_padding,
                corridor_wander,
                extra_corridor_ratio,
                branch_factor,
                seed: seed.unwrap_or(0),
            };
            let mut def = crate::world::dungeon_gen::generate_dungeon(authored_id.clone(), params);

            // Allocate runtime IDs for the generated walls and register them
            // with the ObjectRegistry so future editor operations (brush,
            // properties panel) can find them.
            let start_id = object_registry.next_runtime_id();
            def.resolve_objects(start_id);
            for object in &def.resolved_objects {
                object_registry.register_existing(object.id, &object.type_id);
            }

            let space_id = instantiate_space(
                &mut commands,
                &mut space_manager,
                &mut floor_maps,
                &def,
                &object_definitions,
                &object_registry,
                None,
                def.permanence,
            );
            space_definitions.insert_or_replace(def.clone());

            editor_context.space_id = space_id;
            editor_context.authored_id = authored_id;
            editor_context.map_width = def.width;
            editor_context.map_height = def.height;
            editor_context.fill_floor_type = def.fill_floor_type.clone();
            world_config.current_space_id = space_id;
            world_config.map_width = def.width;
            world_config.map_height = def.height;
            world_config.fill_floor_type = def.fill_floor_type.clone();
            editor_camera.center = Vec2::new(def.width as f32 * 0.5, def.height as f32 * 0.5);
            portal_buffer.portals.clear();
            spawn_group_buffer.groups.clear();
            spawn_group_buffer.selected = None;
            spawn_group_buffer.pending_new_spawn_group_template = None;
            lighting_buffer.config = def.lighting.clone();
            lighting_buffer.selected_keyframe = None;
            vendor_stash_buffer.stashes.clear();
            vendor_stash_buffer.selected = None;
            vendor_stash_buffer.editing = None;
            vendor_stash_buffer.edit_text.clear();
            vendor_stash_buffer.pending_ware_pick = None;
            editor_state.dirty = true;
            editor_state.selected_type_id = None;
            editor_state.selected_object_id = None;
            editor_state.current_tool = EditorTool::Brush;
            editor_state.needs_visual_reattach = true;
            prop_buffer.object_id = None;
            prop_buffer.entries.clear();
            prop_buffer.editing_index = None;
            undo_stack.clear();
        }
        ModalConfirmed::PortalCreate {
            source_tile,
            id,
            dest_space_id,
            dest_tile_x,
            dest_tile_y,
        } => {
            let portal = PortalDefinition {
                id,
                source: TileCoordinate {
                    x: source_tile.x,
                    y: source_tile.y,
                    z: source_tile.z,
                },
                destination_space_id: dest_space_id,
                destination_tile: TileCoordinate {
                    x: dest_tile_x,
                    y: dest_tile_y,
                    z: 0,
                },
                destination_permanence: None,
            };
            let index = portal_buffer.portals.len();
            portal_buffer.portals.push(portal);
            undo_stack.push_undo(UndoOp::RemovePortal { index });
            editor_state.dirty = true;
        }
        ModalConfirmed::SaveAsTemplate { name } => {
            let Some(fragment) = modal_state.pending_template_fragment.take() else {
                warn!("SaveAsTemplate confirmed without a pending fragment");
                return;
            };
            match save_template(&name, &fragment) {
                Ok(()) => {
                    info!("Saved template '{name}'");
                    templates_index.loaded = false;
                }
                Err(e) => warn!("Failed to save template '{name}': {e}"),
            }
        }
    }
}

// ── Portal overlays ───────────────────────────────────────────────────────────

pub fn sync_portal_overlays(
    portal_buffer: Res<EditorPortalBuffer>,
    editor_context: Res<EditorContext>,
    markers: Query<Entity, With<crate::editor::resources::EditorPortalMarker>>,
    mut commands: Commands,
) {
    if !portal_buffer.is_changed() && !editor_context.is_changed() {
        return;
    }
    for entity in &markers {
        commands.entity(entity).despawn();
    }
    for (i, portal) in portal_buffer.portals.iter().enumerate() {
        let tile = portal.source.to_tile_position();
        commands.spawn((
            crate::editor::resources::EditorPortalMarker { portal_index: i },
            SpaceResident {
                space_id: editor_context.space_id,
            },
            tile,
            ViewPosition {
                space_id: editor_context.space_id,
                tile,
            },
            WorldVisual {
                z_index: 8.0,
                y_sort: false,
                sprite_height: 0.0,
                rotation_by_facing: false,
                block_size: 0,
                stack_order: 0,
                hide_when_inside_facing: None,
            },
            Sprite {
                color: Color::srgba(0.2, 0.6, 1.0, 0.55),
                custom_size: Some(Vec2::splat(48.0)),
                ..default()
            },
            Transform::default(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(x: i32, y: i32) -> TilePosition {
        TilePosition::ground(x, y)
    }

    #[test]
    fn bresenham_single_tile() {
        assert_eq!(bresenham_line_tiles(t(3, 4), t(3, 4)), vec![t(3, 4)]);
    }

    #[test]
    fn bresenham_horizontal_line() {
        assert_eq!(
            bresenham_line_tiles(t(0, 5), t(3, 5)),
            vec![t(0, 5), t(1, 5), t(2, 5), t(3, 5)]
        );
    }

    #[test]
    fn bresenham_45_degree_diagonal() {
        assert_eq!(
            bresenham_line_tiles(t(0, 0), t(3, 3)),
            vec![t(0, 0), t(1, 1), t(2, 2), t(3, 3)]
        );
    }

    #[test]
    fn bresenham_endpoints_inclusive_in_either_direction() {
        let forward = bresenham_line_tiles(t(0, 0), t(2, 4));
        let backward = bresenham_line_tiles(t(2, 4), t(0, 0));
        assert_eq!(forward.first(), Some(&t(0, 0)));
        assert_eq!(forward.last(), Some(&t(2, 4)));
        assert_eq!(backward.first(), Some(&t(2, 4)));
        assert_eq!(backward.last(), Some(&t(0, 0)));
        assert_eq!(forward.len(), backward.len());
    }
}
