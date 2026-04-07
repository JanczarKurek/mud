pub mod components;
pub mod resources;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::ui::resources::{
    ChatLogState, ContextMenuState, DragState, InventoryState, OpenContainerState,
};
use crate::ui::setup::spawn_hud;
use crate::ui::systems::{
    handle_context_menu_actions, handle_context_menu_opening, handle_movable_dragging,
    manage_open_containers, sync_chat_log, sync_close_container_button, sync_container_slot_images,
    sync_context_menu_open_button, sync_context_menu_root, sync_context_menu_use_button,
    sync_drag_preview, sync_equipment_slot_images, sync_item_slot_button_visibility,
    sync_open_container_title, sync_vital_bars,
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InventoryState::default())
            .insert_resource(ChatLogState::default())
            .insert_resource(ContextMenuState::default())
            .insert_resource(OpenContainerState::default())
            .insert_resource(DragState::default())
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    manage_open_containers,
                    sync_vital_bars,
                    sync_chat_log,
                    sync_context_menu_root,
                    sync_context_menu_open_button,
                    sync_context_menu_use_button,
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
            .add_systems(Update, handle_movable_dragging)
            .add_systems(Update, sync_drag_preview);
    }
}
