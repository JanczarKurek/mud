use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::editor::resources::{EditorCamera, EditorPropertyEditBuffer, EditorState};
use crate::editor::systems::{
    attach_editor_visuals, handle_editor_camera_pan, handle_editor_escape,
    handle_editor_keyboard_input, handle_editor_left_click, handle_editor_right_click,
    handle_editor_save, init_editor_context, sync_tile_transforms_editor,
};
use crate::editor::ui::{
    cleanup_editor_hud, handle_save_button_click, spawn_editor_hud, sync_editor_top_bar,
};
use crate::editor::ui::palette::{handle_palette_clicks, sync_palette_selection};
use crate::editor::ui::properties::{
    handle_add_property_button, handle_property_row_click, sync_properties_panel,
};

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorState>()
            .init_resource::<EditorCamera>()
            .init_resource::<EditorPropertyEditBuffer>()
            .add_systems(
                OnEnter(ClientAppState::MapEditor),
                (
                    init_editor_context,
                    attach_editor_visuals.after(init_editor_context),
                    spawn_editor_hud.after(init_editor_context),
                ),
            )
            .add_systems(OnExit(ClientAppState::MapEditor), cleanup_editor_hud)
            .add_systems(
                Update,
                (
                    handle_editor_camera_pan,
                    sync_tile_transforms_editor,
                    handle_editor_left_click,
                    handle_editor_right_click,
                    handle_editor_escape,
                    handle_editor_keyboard_input,
                    handle_editor_save,
                    // UI sync
                    sync_editor_top_bar,
                    sync_palette_selection,
                    handle_palette_clicks,
                    sync_properties_panel,
                    handle_property_row_click,
                    handle_add_property_button,
                    handle_save_button_click,
                )
                    .run_if(in_state(ClientAppState::MapEditor)),
            );
    }
}
