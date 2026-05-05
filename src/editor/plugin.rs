use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::editor::floor_render::{
    cleanup_editor_floor_cells, editor_build_floor_render_cells,
    editor_sync_floor_render_transforms, EditorFloorRenderState,
};
use crate::editor::clipboard::{
    handle_clipboard_shortcuts, handle_editor_paste_click, render_paste_ghost,
};
use crate::editor::dialog_index::EditorDialogIndex;
use crate::editor::resources::{
    EditorCamera, EditorClipboard, EditorCursorMarker, EditorPasteGhostMarker,
    EditorPickRectResult, EditorPortalBuffer, EditorPropertyEditBuffer, EditorSpawnGroupBuffer,
    EditorState, ModalState, UndoStack,
};
use crate::editor::selection::{
    handle_editor_pick_rect_drag, handle_editor_select_drag, handle_editor_select_hotkey,
    render_selection,
};
use crate::editor::templates::EditorTemplatesIndex;
use crate::editor::systems::{
    apply_modal_confirmed, attach_editor_visuals, handle_editor_camera_pan, handle_editor_escape,
    handle_editor_floor_brush_drag, handle_editor_floor_brush_hotkey, handle_editor_keyboard_input,
    handle_editor_left_click, handle_editor_middle_drag_pan, handle_editor_right_click,
    handle_editor_save, handle_editor_zoom, init_editor_context, init_portal_buffer,
    open_file_dialog_shortcut, open_save_as_shortcut, process_modal_confirm, sync_portal_overlays,
    sync_tile_transforms_editor, update_editor_cursor_ghost,
};
use crate::editor::ui::modal::{
    apply_pick_rect_result_to_modal, handle_modal_buttons, handle_modal_keyboard_input,
    handle_modal_list_click, handle_spawn_group_area_kind_click,
    handle_spawn_group_behavior_kind_click, handle_spawn_group_field_click,
    handle_spawn_group_pick_rect_click, spawn_or_rebuild_modal, sync_modal_error_text,
};
use crate::editor::ui::palette::{
    handle_floor_palette_clicks, handle_palette_clicks, handle_palette_filter_click,
    handle_palette_scrolling, sync_floor_palette_selection, sync_palette_filter_text,
    sync_palette_selection,
};
use crate::editor::ui::properties::{
    apply_pick_rect_to_instance_behavior, handle_add_property_button,
    handle_behavior_nudge_buttons, handle_behavior_pick_bounds, handle_behavior_set_buttons,
    handle_dialog_select_buttons, handle_property_row_click, sync_properties_panel,
};
use crate::editor::ui::spawn_groups_panel::{
    handle_spawn_groups_panel_clicks, render_spawn_group_overlay, sync_spawn_groups_panel,
    sync_spawn_groups_panel_visibility,
};
use crate::editor::ui::templates_panel::{
    handle_templates_panel_clicks, sync_templates_panel, sync_templates_panel_visibility,
};
use crate::editor::ui::{
    cleanup_editor_hud, handle_new_map_button_click, handle_open_button_click,
    handle_portal_tool_button_click, handle_redo_button_click,
    handle_save_as_button_click, handle_save_as_template_button_click, handle_save_button_click,
    handle_select_tool_button_click, handle_spawn_groups_toggle_button_click,
    handle_templates_toggle_button_click, handle_undo_button_click, spawn_editor_hud,
    sync_editor_top_bar,
};
use crate::editor::undo::handle_undo_redo;

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
fn reset_editor_session_state(mut editor_state: ResMut<EditorState>) {
    editor_state.selection = None;
    editor_state.paste_state.active = false;
    editor_state.templates_panel_visible = false;
    editor_state.spawn_groups_panel_visible = false;
    editor_state.tool_before_pick = None;
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

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .init_resource::<EditorCamera>()
            .init_resource::<EditorPropertyEditBuffer>()
            .init_resource::<ModalState>()
            .init_resource::<UndoStack>()
            .init_resource::<EditorPortalBuffer>()
            .init_resource::<EditorSpawnGroupBuffer>()
            .init_resource::<EditorPickRectResult>()
            .init_resource::<EditorDialogIndex>()
            .init_resource::<EditorFloorRenderState>()
            .init_resource::<EditorClipboard>()
            .init_resource::<EditorTemplatesIndex>()
            .add_systems(
                OnEnter(ClientAppState::MapEditor),
                (
                    init_editor_context,
                    attach_editor_visuals.after(init_editor_context),
                    init_portal_buffer.after(init_editor_context),
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
                    handle_editor_paste_click.before(handle_editor_left_click),
                )
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
                    handle_spawn_group_field_click,
                    handle_spawn_group_area_kind_click,
                    handle_spawn_group_behavior_kind_click,
                    handle_spawn_group_pick_rect_click,
                    process_modal_confirm
                        .after(handle_modal_buttons)
                        .after(handle_modal_keyboard_input),
                    apply_modal_confirmed.after(process_modal_confirm),
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
                    handle_palette_clicks,
                    handle_palette_filter_click,
                    sync_floor_palette_selection,
                    handle_floor_palette_clicks,
                    handle_palette_scrolling,
                    sync_properties_panel,
                    handle_property_row_click,
                    handle_add_property_button,
                    handle_behavior_set_buttons,
                    handle_behavior_pick_bounds,
                    handle_behavior_nudge_buttons,
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
                    handle_portal_tool_button_click,
                    handle_undo_button_click,
                    handle_redo_button_click,
                    handle_select_tool_button_click,
                    handle_save_as_template_button_click,
                    handle_templates_toggle_button_click,
                    handle_spawn_groups_toggle_button_click,
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
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            );
    }
}
