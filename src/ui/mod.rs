pub mod components;
pub mod menu_bar;
pub mod minimap;
pub mod resources;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::ui::menu_bar::{apply_menu_actions, handle_menu_bar_clicks, sync_menu_dropdowns};
use crate::ui::minimap::{
    handle_minimap_keybinds, handle_minimap_scroll_wheel, handle_minimap_zoom_buttons,
    sync_full_map_window_visibility, sync_minimap_zoom_labels, update_minimap_images,
};
use crate::ui::resources::{
    ContextMenuState, CursorState, DockedPanelDragState, DockedPanelResizeState, DockedPanelState,
    DragState, FullMapWindowState, HudMinimapSettings, OpenMenuState, PendingMenuActions,
    SpellTargetingState, TakePartialState, UseOnState,
};
use crate::ui::setup::spawn_hud;
use crate::ui::systems::{
    apply_game_ui_events, handle_attack_targeting, handle_context_menu_actions,
    handle_context_menu_opening, handle_docked_panel_close_buttons, handle_docked_panel_dragging,
    handle_docked_panel_resizing, handle_docked_panel_scrolling, handle_movable_dragging,
    handle_spell_targeting, handle_take_partial_buttons, handle_use_on_targeting,
    manage_open_containers, print_right_sidebar_layout_debug, setup_native_custom_cursor,
    sync_chat_log, sync_container_slot_images, sync_context_menu_attack_button,
    sync_context_menu_open_button, sync_context_menu_root, sync_context_menu_take_partial_button,
    sync_context_menu_use_button, sync_context_menu_use_on_button, sync_current_combat_target,
    sync_docked_panel_layout, sync_docked_panel_titles, sync_drag_preview,
    sync_equipment_slot_images, sync_item_slot_button_visibility, sync_native_custom_cursor,
    sync_take_partial_label, sync_vital_bars, toggle_cursor_mode,
    update_take_partial_popup_visibility,
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ContextMenuState::default())
            .insert_resource(DockedPanelState::default())
            .insert_resource(DockedPanelResizeState::default())
            .insert_resource(DockedPanelDragState::default())
            .insert_resource(DragState::default())
            .insert_resource(CursorState::default())
            .insert_resource(UseOnState::default())
            .insert_resource(SpellTargetingState::default())
            .insert_resource(TakePartialState::default())
            .insert_resource(HudMinimapSettings::default())
            .insert_resource(FullMapWindowState::default())
            .insert_resource(OpenMenuState::default())
            .insert_resource(PendingMenuActions::default())
            .add_systems(
                OnEnter(ClientAppState::InGame),
                (spawn_hud, setup_native_custom_cursor),
            )
            .add_systems(
                Update,
                (
                    apply_game_ui_events,
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
                )
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                (sync_item_slot_button_visibility, sync_container_slot_images)
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    sync_context_menu_take_partial_button,
                    update_take_partial_popup_visibility,
                    sync_take_partial_label,
                    handle_take_partial_buttons,
                )
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                sync_equipment_slot_images.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_context_menu_actions.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_docked_panel_close_buttons.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_docked_panel_dragging.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_docked_panel_resizing.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_docked_panel_scrolling.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_context_menu_opening.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_use_on_targeting.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_spell_targeting.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_attack_targeting.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                handle_movable_dragging.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                print_right_sidebar_layout_debug.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                sync_native_custom_cursor
                    .after(toggle_cursor_mode)
                    .after(handle_context_menu_actions)
                    .after(handle_context_menu_opening)
                    .after(handle_use_on_targeting)
                    .after(handle_spell_targeting)
                    .after(handle_attack_targeting)
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                sync_drag_preview.run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    handle_minimap_keybinds,
                    handle_minimap_scroll_wheel,
                    handle_minimap_zoom_buttons,
                    sync_full_map_window_visibility,
                    sync_minimap_zoom_labels,
                    update_minimap_images,
                )
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    handle_menu_bar_clicks,
                    sync_menu_dropdowns.after(handle_menu_bar_clicks),
                    apply_menu_actions.after(handle_menu_bar_clicks),
                )
                    .run_if(in_state(ClientAppState::InGame)),
            );
    }
}
