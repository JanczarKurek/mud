pub mod backpack_panel;
pub mod book_panel;
pub mod character_sheet;
pub mod chat_input;
pub mod components;
pub mod container_panel;
pub mod dialog;
pub mod equipment_panel;
pub mod item_details;
pub mod log_panel;
pub mod menu_bar;
pub mod minimap;
pub mod minimap_panel;
pub mod mountable_panel;
pub mod movable_window;
pub mod nearby_npcs_panel;
pub mod persistence;
pub mod quickbar;
pub mod recipe_book;
pub mod resources;
pub mod retro_bar;
pub mod settings;
pub mod setup;
pub mod skills_panel;
pub mod sprite_state;
pub mod status_panel;
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
/// Focus id for the per-character log panel's multi-line note editor.
pub const LOG_NOTES_FOCUS_ID: TerminalFocusId = TerminalFocusId(3);
/// Focus id for the log panel's click-to-edit title input.
pub const LOG_TITLE_FOCUS_ID: TerminalFocusId = TerminalFocusId(4);
/// Focus id for the book panel's title field (single-line; Enter submits).
pub const BOOK_TITLE_FOCUS_ID: TerminalFocusId = TerminalFocusId(5);
/// Focus id for the book panel's body field (multi-line; Ctrl+Enter submits).
pub const BOOK_BODY_FOCUS_ID: TerminalFocusId = TerminalFocusId(6);

use crate::app::state::ClientAppState;
use crate::ui::backpack_panel::BackpackPanel;
use crate::ui::chat_input::{handle_chat_click_focus, handle_chat_submissions, toggle_chat_focus};
use crate::ui::container_panel::ContainerPanel;
use crate::ui::dialog::{
    auto_pin_dialog_transcript_scroll, handle_dialog_panel_clicks,
    handle_dialog_transcript_scrolling, sync_dialog_panel_continue_button,
    sync_dialog_panel_options, sync_dialog_panel_transcript, sync_dialog_window_lifecycle,
    DialogPanelRenderState,
};
use crate::ui::equipment_panel::EquipmentPanel;
use crate::ui::menu_bar::{
    apply_menu_actions, handle_menu_bar_clicks, sync_menu_dropdowns, sync_menu_toggle_labels,
    update_coordinate_readout,
};
use crate::ui::minimap::{
    handle_floating_minimap_pan, handle_minimap_keybinds, handle_minimap_scroll_wheel,
    handle_minimap_zoom_buttons, reset_floating_minimap_pan_when_mounted, sync_minimap_zoom_labels,
    update_minimap_images,
};
use crate::ui::minimap_panel::MinimapPanel;
use crate::ui::mountable_panel::{MountablePanelLifecycleSet, MountablePanelPlugin};
use crate::ui::nearby_npcs_panel::NearbyNpcsPanel;
use crate::ui::persistence::{load_ui_state_on_login, persist_ui_state, UiStateLoadedFor};
use crate::ui::quickbar::{
    handle_bottom_panel_hide_button, handle_bottom_panel_hide_key, handle_quickbar_clicks,
    handle_quickbar_keybinds, load_quickbar_on_login, persist_quickbar,
    sync_bottom_panels_visibility, sync_quickbar_visuals, unhide_on_console_open,
    QuickbarLoadedFor,
};
use crate::ui::resources::{
    ActiveDialogState, BottomPanelVisibility, ContextMenuState, CursorState, DockedPanelDragState,
    DockedPanelResizeState, DockedPanelState, DragState, FloatingMinimapPan, FloatingMinimapZoom,
    HoveredTile, HudMinimapSettings, OpenMenuState, PendingMenuActions, Quickbar, ShowCoordinates,
    SpellTargetingState, TakePartialState, TradePopupState, UseOnState,
};
use crate::ui::setup::spawn_hud;
use crate::ui::sprite_state::sync_object_state_visuals;
use crate::ui::status_panel::StatusPanel;
use crate::ui::systems::{
    apply_game_ui_events, close_context_menu_on_lmb, consume_death_summary_events,
    consume_level_up_toasts, handle_attack_targeting, handle_context_menu_actions,
    handle_context_menu_lock_actions, handle_context_menu_opening, handle_context_menu_read_action,
    handle_death_summary_dismiss, handle_docked_panel_close_buttons, handle_docked_panel_dragging,
    handle_docked_panel_resizing, handle_docked_panel_scrolling, handle_movable_dragging,
    handle_nearby_npc_row_clicks, handle_spell_targeting, handle_take_partial_buttons,
    handle_trade_context_menu_actions, handle_use_on_targeting, manage_open_containers,
    print_right_sidebar_layout_debug, setup_native_custom_cursor, sync_carry_weight_label,
    sync_chat_log, sync_container_slot_images, sync_context_menu_attack_button,
    sync_context_menu_force_lock_button, sync_context_menu_hide_button,
    sync_context_menu_interact_button, sync_context_menu_offer_to_trade_button,
    sync_context_menu_open_button, sync_context_menu_pick_lock_button,
    sync_context_menu_read_button, sync_context_menu_root, sync_context_menu_take_partial_button,
    sync_context_menu_talk_button, sync_context_menu_trade_button, sync_context_menu_use_button,
    sync_context_menu_use_key_button, sync_context_menu_use_on_button, sync_docked_panel_layout,
    sync_docked_panel_titles, sync_drag_preview, sync_equipment_slot_images,
    sync_item_slot_button_visibility, sync_item_tooltip, sync_magic_effects_label,
    sync_native_custom_cursor, sync_nearby_npcs_panel, sync_regen_buff_label,
    sync_take_partial_label, sync_vital_bars, sync_xp_bar, tick_level_up_toasts,
    toggle_cursor_mode, update_hovered_tile, update_take_partial_popup_visibility,
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
            MountablePanelPlugin::<StatusPanel>::default(),
            MountablePanelPlugin::<EquipmentPanel>::default(),
            MountablePanelPlugin::<BackpackPanel>::default(),
            MountablePanelPlugin::<NearbyNpcsPanel>::default(),
            MountablePanelPlugin::<MinimapPanel>::default(),
            MountablePanelPlugin::<ContainerPanel>::default(),
            crate::ui::settings::SettingsPlugin,
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
        .insert_resource(FloatingMinimapZoom::default())
        .insert_resource(FloatingMinimapPan::default())
        .insert_resource(OpenMenuState::default())
        .insert_resource(PendingMenuActions::default())
        .insert_resource(ShowCoordinates::default())
        .insert_resource(HoveredTile::default())
        .insert_resource(ActiveDialogState::default())
        .insert_resource(DialogPanelRenderState::default())
        .insert_resource(crate::ui::book_panel::BookPanelState::default())
        .insert_resource(crate::ui::book_panel::BookPanelRenderState::default())
        .insert_resource(TradePanelRenderState::default())
        .insert_resource(TradePopupState::default())
        .insert_resource(TimeOfDayPopupState::default())
        .insert_resource(Quickbar::default())
        .insert_resource(QuickbarLoadedFor::default())
        .insert_resource(UiStateLoadedFor::default())
        .insert_resource(BottomPanelVisibility::default())
        .add_systems(
            OnEnter(ClientAppState::InGame),
            (spawn_hud, setup_native_custom_cursor),
        )
        .add_systems(OnExit(ClientAppState::InGame), teardown_hud)
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
                sync_nearby_npcs_panel,
                sync_docked_panel_layout.after(MountablePanelLifecycleSet),
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
            (consume_death_summary_events, handle_death_summary_dismiss)
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
            handle_nearby_npc_row_clicks
                .before(crate::game::CommandIntercept)
                .before(handle_use_on_targeting)
                .before(handle_spell_targeting)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            handle_context_menu_lock_actions
                .before(crate::game::CommandIntercept)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            (
                sync_context_menu_pick_lock_button,
                sync_context_menu_force_lock_button,
                sync_context_menu_use_key_button,
                sync_context_menu_hide_button,
                sync_context_menu_read_button,
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            handle_context_menu_read_action
                .before(crate::game::CommandIntercept)
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            handle_trade_context_menu_actions.run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            close_context_menu_on_lmb
                .after(handle_context_menu_actions)
                .after(handle_context_menu_lock_actions)
                .after(handle_trade_context_menu_actions)
                .after(handle_context_menu_read_action)
                .run_if(in_state(ClientAppState::InGame)),
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
                handle_floating_minimap_pan,
                reset_floating_minimap_pan_when_mounted,
                sync_minimap_zoom_labels,
                // Run before the lifecycle's despawn so the dot-spawn
                // commands queue before the canvas despawn — the
                // recursive despawn then cleans up dots via Children
                // rather than leaving them orphaned on screen.
                update_minimap_images.before(MountablePanelLifecycleSet),
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            (
                handle_menu_bar_clicks,
                sync_menu_dropdowns.after(handle_menu_bar_clicks),
                apply_menu_actions.after(handle_menu_bar_clicks),
                sync_menu_toggle_labels.after(apply_menu_actions),
                update_hovered_tile,
                update_coordinate_readout.after(update_hovered_tile),
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
            Update,
            (
                crate::ui::book_panel::handle_book_panel_clicks
                    .before(crate::game::CommandIntercept),
                crate::ui::book_panel::sync_book_window_lifecycle
                    .after(crate::ui::book_panel::handle_book_panel_clicks),
                crate::ui::book_panel::sync_book_panel_body
                    .after(crate::ui::book_panel::sync_book_window_lifecycle),
                crate::ui::book_panel::install_book_editors
                    .after(crate::ui::book_panel::sync_book_panel_body)
                    .before(bevy_terminal::text_edit_sync),
                crate::ui::book_panel::handle_book_editor_focus_click,
                crate::ui::book_panel::release_book_focus_when_idle
                    .after(crate::ui::book_panel::sync_book_panel_body),
                crate::ui::book_panel::consume_book_text_edit_submits
                    .before(crate::game::CommandIntercept),
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            bevy::prelude::PreUpdate,
            crate::ui::book_panel::clear_book_editor_focus_on_escape
                .before(bevy_terminal::text_edit_input)
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
        )
        .add_systems(
            Update,
            (
                load_quickbar_on_login,
                load_ui_state_on_login,
                sync_quickbar_visuals,
                handle_quickbar_keybinds
                    .before(crate::game::CommandIntercept)
                    .run_if(bevy_terminal::terminal_not_focused),
                handle_quickbar_clicks.before(handle_context_menu_opening),
                persist_quickbar,
                persist_ui_state.after(load_ui_state_on_login),
                handle_bottom_panel_hide_button,
                handle_bottom_panel_hide_key.run_if(bevy_terminal::terminal_not_focused),
                unhide_on_console_open,
                sync_bottom_panels_visibility
                    .after(handle_bottom_panel_hide_button)
                    .after(handle_bottom_panel_hide_key)
                    .after(unhide_on_console_open),
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            PreUpdate,
            (
                toggle_chat_focus.before(bevy_terminal::terminal_input),
                handle_chat_click_focus,
            )
                .run_if(in_state(ClientAppState::InGame)),
        )
        .add_systems(
            Update,
            handle_chat_submissions
                .before(crate::game::CommandIntercept)
                .run_if(in_state(ClientAppState::InGame)),
        );

        crate::ui::character_sheet::register(app);
    }
}

/// Despawn every entity tagged with `HudRoot` and reset HUD-owned UI state
/// so a future `OnEnter(InGame)` rebuilds the HUD from a clean slate.
#[allow(clippy::too_many_arguments)]
fn teardown_hud(
    mut commands: Commands,
    hud_roots: Query<Entity, With<crate::ui::components::HudRoot>>,
    mut docked: ResMut<DockedPanelState>,
    mut floating_zoom: ResMut<FloatingMinimapZoom>,
    mut floating_pan: ResMut<FloatingMinimapPan>,
    mut open_menu: ResMut<OpenMenuState>,
    mut pending_actions: ResMut<PendingMenuActions>,
    mut active_dialog: ResMut<ActiveDialogState>,
    mut trade_popup: ResMut<TradePopupState>,
    mut quickbar: ResMut<Quickbar>,
    mut quickbar_loaded: ResMut<QuickbarLoadedFor>,
    mut ui_state_loaded: ResMut<UiStateLoadedFor>,
    mut bottom_visibility: ResMut<BottomPanelVisibility>,
) {
    for entity in &hud_roots {
        commands.entity(entity).despawn();
    }
    *docked = DockedPanelState::default();
    *floating_zoom = FloatingMinimapZoom::default();
    *floating_pan = FloatingMinimapPan::default();
    open_menu.open_id = None;
    pending_actions.actions.clear();
    *active_dialog = ActiveDialogState::default();
    *trade_popup = TradePopupState::default();
    *quickbar = Quickbar::default();
    *quickbar_loaded = QuickbarLoadedFor::default();
    *ui_state_loaded = UiStateLoadedFor::default();
    *bottom_visibility = BottomPanelVisibility::default();
}
