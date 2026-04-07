pub mod components;
pub mod resources;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::ui::resources::{
    ChatLogState, ContextMenuState, CursorState, DragState, InventoryState, OpenContainerState,
    UseOnState,
};
use crate::ui::setup::spawn_hud;
use crate::ui::systems::{
    handle_context_menu_actions, handle_context_menu_opening, handle_movable_dragging,
    handle_use_on_targeting, manage_open_containers, setup_native_custom_cursor, sync_chat_log,
    sync_close_container_button, sync_container_slot_images, sync_context_menu_open_button,
    sync_context_menu_root, sync_context_menu_use_button, sync_context_menu_use_on_button,
    sync_drag_preview, sync_equipment_slot_images, sync_item_slot_button_visibility,
    sync_native_custom_cursor, sync_open_container_title, sync_vital_bars, toggle_cursor_mode,
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InventoryState::default())
            .insert_resource(ChatLogState::default())
            .insert_resource(ContextMenuState::default())
            .insert_resource(OpenContainerState::default())
            .insert_resource(DragState::default())
            .insert_resource(CursorState::default())
            .insert_resource(UseOnState::default())
            .add_systems(Startup, (spawn_hud, setup_native_custom_cursor))
            .add_systems(
                Update,
                (
                    toggle_cursor_mode,
                    sync_native_custom_cursor,
                    manage_open_containers,
                    sync_vital_bars,
                    sync_chat_log,
                    sync_context_menu_root,
                    sync_context_menu_open_button,
                    sync_context_menu_use_button,
                    sync_context_menu_use_on_button,
                    sync_open_container_title,
                ),
            )
            .add_systems(
                Update,
                (
                    sync_close_container_button,
                    sync_item_slot_button_visibility,
                    sync_container_slot_images,
                ),
            )
            .add_systems(Update, sync_equipment_slot_images)
            .add_systems(Update, handle_context_menu_actions)
            .add_systems(Update, handle_context_menu_opening)
            .add_systems(Update, handle_use_on_targeting)
            .add_systems(Update, handle_movable_dragging)
            .add_systems(Update, sync_drag_preview);
    }
}
