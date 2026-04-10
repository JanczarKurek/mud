pub mod components;
pub mod resources;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::ui::resources::{
    ChatLogState, ContextMenuState, CursorState, DockedPanelDragState, DockedPanelResizeState,
    DockedPanelState, DragState, InventoryState, SpellTargetingState, UseOnState,
};
use crate::ui::setup::spawn_hud;
use crate::ui::systems::{
    handle_context_menu_actions, handle_context_menu_opening, handle_docked_panel_close_buttons,
    handle_docked_panel_dragging, handle_docked_panel_resizing, handle_docked_panel_scrolling,
    handle_movable_dragging, handle_spell_targeting, handle_use_on_targeting,
    manage_open_containers, print_right_sidebar_layout_debug, setup_native_custom_cursor,
    sync_chat_log, sync_container_slot_images, sync_context_menu_attack_button,
    sync_context_menu_open_button, sync_context_menu_root, sync_context_menu_use_button,
    sync_context_menu_use_on_button, sync_current_combat_target, sync_docked_panel_layout,
    sync_docked_panel_titles, sync_drag_preview, sync_equipment_slot_images,
    sync_item_slot_button_visibility, sync_native_custom_cursor, sync_vital_bars,
    toggle_cursor_mode,
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InventoryState::default())
            .insert_resource(ChatLogState::default())
            .insert_resource(ContextMenuState::default())
            .insert_resource(DockedPanelState::default())
            .insert_resource(DockedPanelResizeState::default())
            .insert_resource(DockedPanelDragState::default())
            .insert_resource(DragState::default())
            .insert_resource(CursorState::default())
            .insert_resource(UseOnState::default())
            .insert_resource(SpellTargetingState::default())
            .add_systems(Startup, (spawn_hud, setup_native_custom_cursor))
            .add_systems(
                Update,
                (
                    toggle_cursor_mode,
                    manage_open_containers,
                    sync_vital_bars,
                    sync_chat_log,
                    sync_context_menu_root,
                    sync_context_menu_attack_button,
                    sync_context_menu_open_button,
                    sync_context_menu_use_button,
                    sync_context_menu_use_on_button,
                    sync_current_combat_target,
                    sync_docked_panel_layout,
                    sync_docked_panel_titles,
                ),
            )
            .add_systems(
                Update,
                (sync_item_slot_button_visibility, sync_container_slot_images),
            )
            .add_systems(Update, sync_equipment_slot_images)
            .add_systems(Update, handle_context_menu_actions)
            .add_systems(Update, handle_docked_panel_close_buttons)
            .add_systems(Update, handle_docked_panel_dragging)
            .add_systems(Update, handle_docked_panel_resizing)
            .add_systems(Update, handle_docked_panel_scrolling)
            .add_systems(Update, handle_context_menu_opening)
            .add_systems(Update, handle_use_on_targeting)
            .add_systems(Update, handle_spell_targeting)
            .add_systems(Update, handle_movable_dragging)
            .add_systems(Update, print_right_sidebar_layout_debug)
            .add_systems(
                Update,
                sync_native_custom_cursor
                    .after(toggle_cursor_mode)
                    .after(handle_context_menu_actions)
                    .after(handle_context_menu_opening)
                    .after(handle_use_on_targeting)
                    .after(handle_spell_targeting),
            )
            .add_systems(Update, sync_drag_preview);
    }
}
