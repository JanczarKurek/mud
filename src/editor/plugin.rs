use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::editor::resources::{
    EditorCamera, EditorPortalBuffer, EditorPropertyEditBuffer, EditorState, ModalState, UndoStack,
};
use crate::editor::systems::{
    apply_modal_confirmed, attach_editor_visuals, handle_editor_camera_pan, handle_editor_escape,
    handle_editor_floor_brush_hotkey, handle_editor_keyboard_input, handle_editor_left_click,
    handle_editor_right_click, handle_editor_save, handle_editor_zoom, init_editor_context,
    init_portal_buffer, open_file_dialog_shortcut, open_save_as_shortcut, process_modal_confirm,
    sync_portal_overlays, sync_tile_transforms_editor,
};
use crate::editor::ui::modal::{
    handle_modal_buttons, handle_modal_keyboard_input, handle_modal_list_click,
    spawn_or_rebuild_modal, sync_modal_error_text,
};
use crate::editor::ui::palette::{
    handle_palette_clicks, handle_palette_filter_click, sync_palette_filter_text,
    sync_palette_selection,
};
use crate::editor::ui::properties::{
    handle_add_property_button, handle_property_row_click, sync_properties_panel,
};
use crate::editor::ui::{
    cleanup_editor_hud, handle_new_map_button_click, handle_open_button_click,
    handle_portal_tool_button_click, handle_redo_button_click, handle_save_as_button_click,
    handle_save_button_click, handle_undo_button_click, spawn_editor_hud, sync_editor_top_bar,
};
use crate::editor::undo::handle_undo_redo;

pub struct EditorPlugin;

fn no_modal(s: Res<ModalState>) -> bool {
    s.active.is_none()
}
fn has_modal(s: Res<ModalState>) -> bool {
    s.active.is_some()
}

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .init_resource::<EditorCamera>()
            .init_resource::<EditorPropertyEditBuffer>()
            .init_resource::<ModalState>()
            .init_resource::<UndoStack>()
            .init_resource::<EditorPortalBuffer>()
            .add_systems(
                OnEnter(ClientAppState::MapEditor),
                (
                    init_editor_context,
                    attach_editor_visuals.after(init_editor_context),
                    init_portal_buffer.after(init_editor_context),
                    spawn_editor_hud.after(init_editor_context),
                ),
            )
            .add_systems(OnExit(ClientAppState::MapEditor), cleanup_editor_hud)
            // Camera / render
            .add_systems(
                Update,
                (
                    handle_editor_camera_pan,
                    handle_editor_zoom,
                    sync_tile_transforms_editor,
                    attach_editor_visuals,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            )
            // World interaction (no modal)
            .add_systems(
                Update,
                (
                    handle_editor_left_click,
                    handle_editor_right_click,
                    handle_editor_escape,
                    handle_editor_keyboard_input,
                    handle_editor_floor_brush_hotkey,
                    handle_undo_redo,
                )
                    .run_if(in_state(ClientAppState::MapEditor))
                    .run_if(no_modal),
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
                    process_modal_confirm
                        .after(handle_modal_buttons)
                        .after(handle_modal_keyboard_input),
                    apply_modal_confirmed.after(process_modal_confirm),
                    spawn_or_rebuild_modal,
                    sync_modal_error_text,
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
                    sync_properties_panel,
                    handle_property_row_click,
                    handle_add_property_button,
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
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            );
    }
}
