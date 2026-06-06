use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::editor::building::{
    handle_editor_building_door_swap_click, handle_editor_building_draw_drag,
};
use crate::editor::clipboard::{
    handle_clipboard_shortcuts, handle_editor_delete_key, handle_editor_paste_click,
    handle_paste_transform_hotkeys, render_paste_ghost,
};
use crate::editor::dialog_index::EditorDialogIndex;
use crate::editor::fill::{handle_editor_flood_fill, handle_editor_rect_fill};
use crate::editor::floor_render::{
    cleanup_editor_floor_cells, editor_build_floor_render_cells, editor_recompute_floor_mask_map,
    editor_sync_floor_render_transforms, EditorFloorRenderState,
};
use crate::editor::floors_editor::{
    editor_recompute_visible_floors, sync_editor_hover_tile, EditorHoverTile,
};
use crate::editor::hotkeys::{
    handle_editor_alt_recent_hotkeys, handle_editor_brush_radius_hotkeys, handle_editor_eyedropper,
    handle_editor_fill_mode_hotkey, handle_editor_floor_switch_hotkey,
    handle_editor_tool_number_hotkeys,
};
use crate::editor::resources::{
    EditorCamera, EditorClipboard, EditorCursorMarker, EditorLightingBuffer,
    EditorPasteGhostMarker, EditorPickRectResult, EditorPortalBuffer, EditorPropertyEditBuffer,
    EditorSpawnGroupBuffer, EditorState, EditorVendorStashBuffer, ModalState, UndoStack,
};
use crate::editor::selection::{
    handle_editor_pick_rect_drag, handle_editor_select_drag, handle_editor_select_hotkey,
    render_selection,
};
use crate::editor::status_bar::sync_status_bar;
use crate::editor::systems::{
    apply_lighting_keyframe_confirmed, apply_modal_confirmed, attach_editor_visuals,
    handle_editor_camera_pan, handle_editor_escape, handle_editor_floor_brush_drag,
    handle_editor_floor_brush_hotkey, handle_editor_keyboard_input, handle_editor_left_click,
    handle_editor_middle_drag_pan, handle_editor_right_click, handle_editor_save,
    handle_editor_zoom, init_editor_client_space, init_editor_context, init_portal_buffer,
    open_file_dialog_shortcut, open_save_as_shortcut, process_modal_confirm,
    reset_space_to_authored, sync_editor_lighting_to_world, sync_editor_view_to_client,
    sync_portal_overlays, sync_tile_transforms_editor, update_editor_cursor_ghost,
};
use crate::editor::templates::EditorTemplatesIndex;
use crate::editor::ui::building_panel::{
    handle_building_panel_clicks, sync_building_panel, sync_building_panel_visibility,
};
use crate::editor::ui::color_picker::EditorColorPickerAssets;
use crate::editor::ui::lighting_panel::{
    handle_lighting_panel_clicks, handle_lighting_scrubber_drag, sync_lighting_panel,
    sync_lighting_panel_visibility, sync_lighting_scrubber_visual,
};
use crate::editor::ui::mobs_panel::{
    apply_pick_rect_for_new_spawn_group, handle_mobs_panel_group_clicks,
    handle_mobs_panel_place_clicks, sync_mobs_panel, sync_mobs_panel_visibility,
};
use crate::editor::ui::modal::{
    apply_pick_rect_result_to_modal, handle_color_picker_hue_drag, handle_color_picker_sv_drag,
    handle_lighting_keyframe_field_click, handle_modal_buttons, handle_modal_keyboard_input,
    handle_modal_list_click, handle_modal_picker_click, handle_modal_text_field_click,
    handle_spawn_group_area_kind_click, handle_spawn_group_behavior_kind_click,
    handle_spawn_group_field_click, handle_spawn_group_pick_rect_click, spawn_or_rebuild_modal,
    sync_modal_error_text,
};
use crate::editor::ui::palette::{
    handle_floor_flavor_toggle_clicks, handle_floor_palette_clicks, handle_palette_clicks,
    handle_palette_filter_click, handle_palette_scrolling, sync_floor_flavor_toggle,
    sync_floor_palette_selection, sync_palette_filter_text, sync_palette_selection,
    sync_recent_strip,
};
use crate::editor::ui::properties::{
    apply_pick_rect_to_instance_behavior, handle_add_property_button, handle_behavior_pick_bounds,
    handle_behavior_set_buttons, handle_dialog_select_buttons, handle_property_row_click,
    sync_properties_panel,
};
use crate::editor::ui::spawn_groups_panel::{
    handle_spawn_groups_panel_clicks, render_spawn_group_overlay, sync_spawn_groups_panel,
    sync_spawn_groups_panel_visibility,
};
use crate::editor::ui::templates_panel::{
    handle_templates_panel_clicks, sync_templates_panel, sync_templates_panel_visibility,
};
use crate::editor::ui::vendor_stashes_panel::{
    handle_vendor_stash_keyboard_input, handle_vendor_stash_palette_pick,
    handle_vendor_stashes_panel_clicks, sync_vendor_stashes_panel,
    sync_vendor_stashes_panel_visibility,
};
use crate::editor::ui::{
    cleanup_editor_hud, handle_building_tool_button_click, handle_exit_button_click,
    handle_generate_dungeon_button_click, handle_lighting_toggle_button_click,
    handle_mobs_toggle_button_click, handle_new_map_button_click, handle_open_button_click,
    handle_portal_tool_button_click, handle_redo_button_click, handle_save_as_button_click,
    handle_save_as_template_button_click, handle_save_button_click,
    handle_select_tool_button_click, handle_spawn_groups_toggle_button_click,
    handle_templates_toggle_button_click, handle_undo_button_click,
    handle_vendor_stashes_toggle_button_click, spawn_editor_hud, sync_editor_top_bar,
};
use crate::editor::undo::handle_undo_redo;
use crate::world::animation::AnimatedSprite;
use crate::world::components::{
    ClientProjectedWorldObject, ClientRemotePlayerVisual, OverworldObject, WorldVisual,
};
use crate::world::resources::{ClientRemotePlayerProjectionState, ClientWorldProjectionState};

pub struct EditorPlugin;

fn no_modal(s: Res<ModalState>) -> bool {
    s.active.is_none()
}
fn has_modal(s: Res<ModalState>) -> bool {
    s.active.is_some()
}

/// Drop selection / paste-mode / templates-panel visibility when leaving the
/// editor so a future re-entry on a different map doesn't carry over stale
/// per-map state. (Clipboard contents and the templates index are kept —
/// they're cross-session by design.)
fn reset_editor_session_state(
    mut editor_state: ResMut<EditorState>,
    mut vendor_stash_buffer: ResMut<EditorVendorStashBuffer>,
) {
    editor_state.selection = None;
    editor_state.paste_state.active = false;
    editor_state.templates_panel_visible = false;
    editor_state.spawn_groups_panel_visible = false;
    editor_state.mobs_panel_visible = false;
    editor_state.lighting_panel_visible = false;
    editor_state.vendor_stashes_panel_visible = false;
    editor_state.tool_before_pick = None;
    editor_state.current_editing_floor = 0;
    editor_state.brush_radius = 1;
    editor_state.fill_mode = crate::editor::resources::FillMode::Single;
    vendor_stash_buffer.editing = None;
    vendor_stash_buffer.edit_text.clear();
    vendor_stash_buffer.pending_ware_pick = None;
}

/// Re-scan `assets/dialogs/` when the editor opens so the dropdown list is
/// fresh after external file changes.
fn refresh_dialog_index_on_enter(mut index: ResMut<EditorDialogIndex>) {
    index.refresh();
}

/// Despawn any cursor / paste ghost sprites left over from the last editor
/// frame. Without this they linger as orphans in the world after exiting,
/// since the systems that own them stop running on `OnExit`.
fn cleanup_editor_ghost_markers(
    mut commands: Commands,
    cursor_ghosts: Query<Entity, With<EditorCursorMarker>>,
    paste_ghosts: Query<Entity, With<EditorPasteGhostMarker>>,
) {
    for entity in cursor_ghosts.iter().chain(paste_ghosts.iter()) {
        commands.entity(entity).despawn();
    }
}

/// Despawn presentation-only projected entities on entering the editor.
/// `sync_client_world_projection` / `sync_remote_player_projection` are
/// gated to `InGame`, so without this they remain as frozen "shadows" of the
/// world the player just left.
fn despawn_client_projections_on_editor_enter(
    mut commands: Commands,
    projected_objects: Query<Entity, With<ClientProjectedWorldObject>>,
    remote_players: Query<Entity, With<ClientRemotePlayerVisual>>,
    mut world_projection: ResMut<ClientWorldProjectionState>,
    mut remote_projection: ResMut<ClientRemotePlayerProjectionState>,
) {
    for entity in projected_objects.iter().chain(remote_players.iter()) {
        commands.entity(entity).despawn();
    }
    world_projection.entities.clear();
    remote_projection.entities.clear();
}

/// Strip presentation components that `attach_editor_visuals` attached to
/// authoritative `OverworldObject` entities. Uses `remove::<>` rather than
/// despawn — the authoritative data (`OverworldObject`, `SpaceResident`,
/// `TilePosition`, `Collider`, …) must survive into gameplay.
fn strip_editor_visuals_on_exit(
    mut commands: Commands,
    query: Query<Entity, With<OverworldObject>>,
) {
    for entity in &query {
        commands.entity(entity).remove::<(
            Sprite,
            Transform,
            GlobalTransform,
            WorldVisual,
            AnimatedSprite,
            bevy::sprite::Anchor,
        )>();
    }
}

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        // Building-preset asset type is editor-only (presets aren't read by
        // the runtime game — once a building is stamped, the world just sees
        // plain wall + floor + door entities). Load + validate at plugin
        // construction so a typo in a preset YAML fails fast. Validation
        // borrows the already-inserted object/floor resources by reference;
        // EditorPlugin is registered after WorldClientPlugin in
        // `src/app/plugin.rs` so both are present here.
        let presets = crate::world::building_presets::BuildingPresets::load_from_disk();
        {
            let world = app.world();
            let object_defs = world
                .get_resource::<crate::world::object_definitions::OverworldObjectDefinitions>();
            let floor_defs =
                world.get_resource::<crate::world::floor_definitions::FloorTilesetDefinitions>();
            match (object_defs, floor_defs) {
                (Some(objs), Some(floors)) => presets.validate_against(objs, floors),
                _ => warn!(
                    "BuildingPresets: skipped validation — object or floor definitions \
                     not yet inserted"
                ),
            }
        }
        app.insert_resource(presets);

        app.init_resource::<EditorState>()
            .init_resource::<EditorCamera>()
            .init_resource::<EditorPropertyEditBuffer>()
            .init_resource::<ModalState>()
            .init_resource::<UndoStack>()
            .init_resource::<EditorPortalBuffer>()
            .init_resource::<EditorSpawnGroupBuffer>()
            .init_resource::<EditorLightingBuffer>()
            .init_resource::<EditorVendorStashBuffer>()
            .init_resource::<EditorPickRectResult>()
            .init_resource::<EditorDialogIndex>()
            .init_resource::<EditorFloorRenderState>()
            .init_resource::<EditorHoverTile>()
            .init_resource::<EditorClipboard>()
            .init_resource::<EditorTemplatesIndex>()
            .init_resource::<EditorColorPickerAssets>()
            .add_systems(
                OnEnter(ClientAppState::MapEditor),
                (
                    init_editor_context,
                    despawn_client_projections_on_editor_enter,
                    reset_space_to_authored.after(init_editor_context),
                    // attach_editor_visuals only catches entities that existed
                    // *before* `reset_space_to_authored`'s despawns flush; the
                    // Update-schedule copy of the same system attaches visuals
                    // to the re-spawned entities once the command queue drains.
                    attach_editor_visuals.after(init_editor_context),
                    init_portal_buffer.after(init_editor_context),
                    init_editor_client_space.after(init_editor_context),
                    refresh_dialog_index_on_enter.after(init_editor_context),
                    spawn_editor_hud.after(init_editor_context),
                ),
            )
            .add_systems(
                OnExit(ClientAppState::MapEditor),
                (
                    cleanup_editor_hud,
                    cleanup_editor_floor_cells,
                    cleanup_editor_ghost_markers,
                    reset_editor_session_state,
                    // Discard unsaved edits on exit so they never leak into the
                    // runtime world snapshot — Ctrl+S (which refreshes
                    // `SpaceDefinitions`) is the only way edits persist. Mirrors
                    // the reset that runs on editor *entry*. Ordered before the
                    // visual strip so it despawns the edited objects before the
                    // strip queries them (avoids remove-on-despawning entities).
                    reset_space_to_authored.before(strip_editor_visuals_on_exit),
                    strip_editor_visuals_on_exit,
                ),
            )
            // Camera / render. `.chain()` keeps both transform syncs strictly
            // after the camera-pan write so they observe identical post-pan
            // values within a frame; otherwise object and floor transforms can
            // drift relative to each other during continuous panning.
            // The whole chain also runs `.after(CommandIntercept)` so the
            // floor-cell builder sees the same-frame mutations from
            // `process_floor_commands` (and the tiles-it-touched dirty queue
            // it populates), avoiding a one-frame paint lag during drags.
            .add_systems(
                Update,
                (
                    handle_editor_camera_pan,
                    handle_editor_middle_drag_pan.run_if(no_modal),
                    handle_editor_zoom,
                    attach_editor_visuals,
                    sync_editor_hover_tile,
                    editor_recompute_visible_floors,
                    editor_recompute_floor_mask_map,
                    editor_build_floor_render_cells,
                    editor_sync_floor_render_transforms,
                    sync_tile_transforms_editor,
                    update_editor_cursor_ghost.run_if(no_modal),
                )
                    .chain()
                    .after(crate::game::CommandIntercept)
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // World interaction (no modal)
            .add_systems(
                Update,
                (
                    handle_editor_left_click,
                    handle_editor_right_click,
                    handle_editor_floor_brush_drag,
                    handle_editor_escape,
                    handle_editor_keyboard_input,
                    handle_editor_floor_brush_hotkey,
                    handle_undo_redo,
                )
                    .run_if(in_state(ClientAppState::MapEditor))
                    .run_if(no_modal),
            )
            // Phase A/B/C hotkey + fill systems. Number-row, brush radius,
            // eyedropper, PgUp/PgDn, fill-mode toggle, recent-alt-N. These
            // are tiny systems with clear gates and don't need to fight for
            // ordering with the bigger handlers, so they sit on their own
            // tuple.
            .add_systems(
                Update,
                (
                    handle_editor_tool_number_hotkeys,
                    handle_editor_brush_radius_hotkeys,
                    handle_editor_fill_mode_hotkey,
                    handle_editor_eyedropper,
                    handle_editor_floor_switch_hotkey,
                    handle_editor_alt_recent_hotkeys,
                    // Rect / flood fill bind to LMB; running them BEFORE the
                    // standard left-click handler so that handler sees its
                    // existing single-tile path when fill_mode is Single.
                    handle_editor_rect_fill.before(handle_editor_left_click),
                    handle_editor_flood_fill.before(handle_editor_left_click),
                )
                    .run_if(in_state(ClientAppState::MapEditor))
                    .run_if(no_modal),
            )
            // Status bar always runs (even with a modal open) so cursor
            // coordinates stay live.
            .add_systems(
                Update,
                sync_status_bar.run_if(in_state(ClientAppState::MapEditor)),
            )
            // Selection / clipboard input (split out so the previous tuple
            // stays under Bevy's `IntoSystemConfigs` arity limit). The paste
            // commit must run *before* the brush click handler so its early-
            // out gate (`paste_state.active`) sees the same state.
            .add_systems(
                Update,
                (
                    handle_editor_select_hotkey,
                    handle_editor_select_drag,
                    handle_editor_pick_rect_drag,
                    handle_clipboard_shortcuts,
                    handle_editor_delete_key,
                    handle_editor_paste_click.before(handle_editor_left_click),
                    handle_paste_transform_hotkeys,
                )
                    .run_if(in_state(ClientAppState::MapEditor))
                    .run_if(no_modal),
            )
            // Building tool input. Door-swap runs BEFORE the generic
            // left-click handler so the wall→door swap happens cleanly
            // without the brush flow also picking up the same click.
            .add_systems(
                Update,
                handle_editor_building_door_swap_click
                    .before(handle_editor_left_click)
                    .run_if(in_state(ClientAppState::MapEditor))
                    .run_if(no_modal),
            )
            .add_systems(
                Update,
                handle_editor_building_draw_drag
                    .run_if(in_state(ClientAppState::MapEditor))
                    .run_if(no_modal),
            )
            // Selection / paste rendering (always; user wants to see the rect
            // even while a modal is open or when they switch tools).
            .add_systems(
                Update,
                (
                    render_selection,
                    render_paste_ghost,
                    render_spawn_group_overlay,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // Save / dialog shortcuts
            .add_systems(
                Update,
                (
                    handle_editor_save,
                    open_file_dialog_shortcut,
                    open_save_as_shortcut,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // Modal systems
            .add_systems(
                Update,
                (
                    handle_modal_keyboard_input.run_if(has_modal),
                    handle_modal_buttons,
                    handle_modal_list_click,
                    handle_modal_picker_click,
                    handle_modal_text_field_click,
                    handle_spawn_group_field_click,
                    handle_spawn_group_area_kind_click,
                    handle_spawn_group_behavior_kind_click,
                    handle_spawn_group_pick_rect_click,
                    handle_lighting_keyframe_field_click,
                    handle_color_picker_sv_drag,
                    handle_color_picker_hue_drag,
                    process_modal_confirm
                        .after(handle_modal_buttons)
                        .after(handle_modal_keyboard_input),
                    apply_modal_confirmed.after(process_modal_confirm),
                    apply_lighting_keyframe_confirmed.after(process_modal_confirm),
                    spawn_or_rebuild_modal,
                    sync_modal_error_text,
                    apply_pick_rect_result_to_modal,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // Portal overlays + UI
            .add_systems(
                Update,
                (
                    sync_portal_overlays,
                    sync_editor_top_bar,
                    sync_palette_selection,
                    sync_palette_filter_text,
                    sync_recent_strip,
                    handle_palette_clicks,
                    handle_palette_filter_click,
                    sync_floor_palette_selection,
                    handle_floor_palette_clicks,
                    sync_floor_flavor_toggle,
                    handle_floor_flavor_toggle_clicks,
                    handle_palette_scrolling,
                    sync_properties_panel,
                    handle_property_row_click,
                    handle_add_property_button,
                    handle_behavior_set_buttons,
                    handle_behavior_pick_bounds,
                    handle_dialog_select_buttons,
                    apply_pick_rect_to_instance_behavior,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // Toolbar button handlers
            .add_systems(
                Update,
                (
                    handle_save_button_click,
                    handle_open_button_click,
                    handle_save_as_button_click,
                    handle_new_map_button_click,
                    handle_generate_dungeon_button_click,
                    handle_portal_tool_button_click,
                    handle_undo_button_click,
                    handle_redo_button_click,
                    handle_select_tool_button_click,
                    handle_save_as_template_button_click,
                    handle_templates_toggle_button_click,
                    handle_spawn_groups_toggle_button_click,
                    handle_mobs_toggle_button_click,
                    handle_lighting_toggle_button_click,
                    handle_vendor_stashes_toggle_button_click,
                    handle_building_tool_button_click,
                    handle_exit_button_click,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // Building panel sync + clicks.
            .add_systems(
                Update,
                (
                    sync_building_panel_visibility,
                    sync_building_panel,
                    handle_building_panel_clicks,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // Templates panel sync + clicks (always run, even with modal).
            .add_systems(
                Update,
                (
                    sync_templates_panel_visibility,
                    sync_templates_panel,
                    handle_templates_panel_clicks,
                    sync_spawn_groups_panel_visibility,
                    sync_spawn_groups_panel,
                    handle_spawn_groups_panel_clicks,
                    sync_mobs_panel_visibility,
                    sync_mobs_panel,
                    handle_mobs_panel_place_clicks,
                    handle_mobs_panel_group_clicks,
                    apply_pick_rect_for_new_spawn_group,
                    sync_lighting_panel_visibility,
                    sync_lighting_panel,
                    sync_lighting_scrubber_visual,
                    handle_lighting_panel_clicks,
                    handle_lighting_scrubber_drag,
                    sync_editor_lighting_to_world,
                    sync_editor_view_to_client,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // Vendor stashes panel — split into its own add_systems call to
            // stay under Bevy's per-tuple system arity limit.
            .add_systems(
                Update,
                (
                    sync_vendor_stashes_panel_visibility,
                    sync_vendor_stashes_panel,
                    handle_vendor_stashes_panel_clicks,
                    // `handle_palette_clicks` checks `pending_ware_pick` and
                    // bails when armed; this pick handler then consumes the
                    // click and clears the arm. Order matters: if pick ran
                    // first it would clear the flag before palette_clicks
                    // could see it, and the brush would arm.
                    handle_vendor_stash_palette_pick
                        .after(crate::editor::ui::palette::handle_palette_clicks),
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            .add_systems(
                Update,
                handle_vendor_stash_keyboard_input
                    .run_if(in_state(ClientAppState::MapEditor))
                    .run_if(no_modal),
            );
    }
}
