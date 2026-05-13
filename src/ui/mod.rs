pub mod components;
pub mod dialog;
pub mod item_details;
pub mod menu_bar;
pub mod minimap;
pub mod movable_window;
pub mod recipe_book;
pub mod resources;
pub mod setup;
pub mod sprite_state;
pub mod systems;
pub mod theme;
pub mod time_of_day_button;
pub mod trade;

use bevy::prelude::*;
use bevy_terminal::TerminalFocusId;

/// Stable focus IDs for the project's terminal-widget instances. Outer
/// systems flip `TerminalFocus::focused` to these when toggling the
/// console / chat input.
pub const PYTHON_CONSOLE_FOCUS_ID: TerminalFocusId = TerminalFocusId(1);
pub const CHAT_TERMINAL_FOCUS_ID: TerminalFocusId = TerminalFocusId(2);

use crate::app::state::ClientAppState;
use crate::ui::dialog::{
    auto_pin_dialog_transcript_scroll, handle_dialog_panel_clicks,
    handle_dialog_transcript_scrolling, sync_dialog_panel_continue_button,
    sync_dialog_panel_options, sync_dialog_panel_transcript, sync_dialog_window_lifecycle,
    DialogPanelRenderState,
};
use crate::ui::menu_bar::{apply_menu_actions, handle_menu_bar_clicks, sync_menu_dropdowns};
use crate::ui::minimap::{
    handle_minimap_keybinds, handle_minimap_scroll_wheel, handle_minimap_zoom_buttons,
    sync_full_map_window_visibility, sync_minimap_zoom_labels, update_minimap_images,
};
use crate::ui::resources::{
    ActiveDialogState, CharacterSheetState, ContextMenuState, CursorState, DockedPanelDragState,
    DockedPanelResizeState, DockedPanelState, DragState, FullMapWindowState, HudMinimapSettings,
    OpenMenuState, PendingMenuActions, SpellTargetingState, TakePartialState, TradePopupState,
    UseOnState,
};
use crate::ui::setup::spawn_hud;
use crate::ui::sprite_state::sync_object_state_visuals;
use crate::ui::systems::{
    apply_game_ui_events, consume_death_summary_events, consume_level_up_toasts,
    handle_attack_targeting, handle_character_sheet_button_click,
    handle_character_sheet_close_click, handle_class_picker_clicks, handle_context_menu_actions,
    handle_context_menu_opening, handle_death_summary_dismiss, handle_docked_panel_close_buttons,
    handle_docked_panel_dragging, handle_docked_panel_resizing, handle_docked_panel_scrolling,
    handle_movable_dragging, handle_spell_targeting, handle_take_partial_buttons,
    handle_trade_context_menu_actions, handle_use_on_targeting, manage_character_sheet_overlay,
    manage_class_picker, manage_open_containers, print_right_sidebar_layout_debug,
    setup_native_custom_cursor, sync_carry_weight_label, sync_chat_log, sync_container_slot_images,
    sync_context_menu_attack_button, sync_context_menu_interact_button,
    sync_context_menu_offer_to_trade_button, sync_context_menu_open_button, sync_context_menu_root,
    sync_context_menu_take_partial_button, sync_context_menu_talk_button,
    sync_context_menu_trade_button, sync_context_menu_use_button, sync_context_menu_use_on_button,
    sync_current_combat_target, sync_docked_panel_layout, sync_docked_panel_titles,
    sync_drag_preview, sync_equipment_slot_images, sync_item_slot_button_visibility,
    sync_item_tooltip, sync_magic_effects_label, sync_native_custom_cursor, sync_regen_buff_label,
    sync_take_partial_label, sync_vital_bars, sync_xp_bar, tick_level_up_toasts,
    toggle_cursor_mode, update_take_partial_popup_visibility,
};
use crate::ui::theme::UiThemePlugin;
use crate::ui::time_of_day_button::{
    handle_time_of_day_button_click, handle_time_of_day_popup_close_click,
    sync_time_of_day_window_lifecycle, update_time_of_day_indicator,
    update_time_of_day_popup_contents, TimeOfDayPopupState,
};
use crate::ui::trade::{
    handle_trade_panel_clicks, handle_trade_popup_close_click, sync_trade_panel_buttons,
    sync_trade_panel_partner_label, sync_trade_panel_rows, sync_trade_window_lifecycle,
    TradePanelRenderState,
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            UiThemePlugin,
            crate::ui::movable_window::MovableWindowPlugin,
            crate::ui::item_details::ItemDetailsPlugin,
        ))
        .insert_resource(ContextMenuState::default())
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
        .insert_resource(ActiveDialogState::default())
        .insert_resource(DialogPanelRenderState::default())
        .insert_resource(CharacterSheetState::default())
        .insert_resource(TradePanelRenderState::default())
        .insert_resource(TradePopupState::default())
        .insert_resource(TimeOfDayPopupState::default())
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
                sync_xp_bar,
                consume_level_up_toasts,
                tick_level_up_toasts,
                sync_regen_buff_label,
                sync_carry_weight_label,
                sync_chat_log,
                sync_context_menu_root,
                sync_context_menu_attack_button,
                sync_context_menu_open_button,
                sync_context_menu_interact_button,
                sync_context_menu_use_button,
                sync_context_menu_use_on_button,
                sync_context_menu_talk_button,
                sync_current_combat_target,
                sync_docked_panel_layout,
                sync_docked_panel_titles,
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            (
                sync_context_menu_trade_button,
                sync_context_menu_offer_to_trade_button,
                sync_magic_effects_label,
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
            (manage_class_picker, handle_class_picker_clicks)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            (consume_death_summary_events, handle_death_summary_dismiss)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            (
                handle_character_sheet_button_click,
                handle_character_sheet_close_click,
                manage_character_sheet_overlay,
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            sync_object_state_visuals.run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            (
                update_time_of_day_indicator,
                handle_time_of_day_button_click,
                handle_time_of_day_popup_close_click,
                sync_time_of_day_window_lifecycle,
                update_time_of_day_popup_contents,
            )
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
            handle_context_menu_actions
                .before(crate::game::CommandIntercept)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            handle_trade_context_menu_actions.run_if(in_state(ClientAppState::InGame)),
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
            (sync_drag_preview, sync_item_tooltip).run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            handle_minimap_keybinds
                .run_if(in_state(ClientAppState::InGame))
                .run_if(bevy_terminal::terminal_not_focused),
        )
        .add_systems(
            Update,
            (
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
        )
        .add_systems(
            Update,
            (
                sync_dialog_window_lifecycle,
                sync_dialog_panel_transcript.after(sync_dialog_window_lifecycle),
                sync_dialog_panel_continue_button.after(sync_dialog_window_lifecycle),
                sync_dialog_panel_options.after(sync_dialog_window_lifecycle),
                handle_dialog_transcript_scrolling.after(sync_dialog_panel_transcript),
                handle_dialog_panel_clicks
                    .after(sync_dialog_panel_options)
                    .after(apply_game_ui_events),
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            PostUpdate,
            auto_pin_dialog_transcript_scroll
                .after(bevy::ui::UiSystems::PostLayout)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            (
                sync_trade_window_lifecycle,
                sync_trade_panel_partner_label.after(sync_trade_window_lifecycle),
                sync_trade_panel_buttons.after(sync_trade_window_lifecycle),
                sync_trade_panel_rows.after(sync_trade_window_lifecycle),
                handle_trade_panel_clicks.after(sync_trade_panel_rows),
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            handle_trade_popup_close_click
                .after(sync_trade_window_lifecycle)
                .run_if(in_state(ClientAppState::InGame)),
        );
    }
}
