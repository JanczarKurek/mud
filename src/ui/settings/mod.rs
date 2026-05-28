//! Client-side settings: a reusable overlay modal whose first section is
//! configurable controls. See `model.rs` for the keybinding taxonomy and the
//! approved plan for the design rationale.

pub mod display;
pub mod gameplay;
pub mod keycode_serde;
pub mod model;
pub mod persistence;
pub mod ui;

use bevy::prelude::*;

pub use display::DisplaySettings;
pub use gameplay::GameplaySettings;
pub use model::{Action, Keybindings};
pub use ui::SettingsUiState;

pub use persistence::{SavedServerEntry, SavedServerList};

use display::apply_display_settings;
use persistence::{load_settings, persist_settings, SettingsLoaded};
use ui::{
    capture_keybind, handle_binding_row_clicks, handle_gameplay_option_row_clicks,
    handle_option_row_clicks, handle_settings_close_button, handle_settings_reset_button,
    handle_settings_scroll, handle_tab_clicks, is_capturing, is_open, spawn_settings_overlay,
    swallow_input_while_settings_open, sync_binding_row_labels, sync_gameplay_option_row_labels,
    sync_option_row_labels, sync_section_visibility, sync_settings_overlay_visibility,
    SettingsCaptureSet,
};

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Keybindings>()
            .init_resource::<DisplaySettings>()
            .init_resource::<GameplaySettings>()
            .init_resource::<SettingsUiState>()
            .init_resource::<SettingsLoaded>()
            .init_resource::<SavedServerList>()
            .add_systems(Startup, (load_settings, spawn_settings_overlay))
            .add_systems(
                PreUpdate,
                (
                    capture_keybind
                        .in_set(SettingsCaptureSet)
                        .run_if(is_capturing),
                    swallow_input_while_settings_open
                        .after(SettingsCaptureSet)
                        .run_if(is_open),
                )
                    .before(bevy_terminal::terminal_input)
                    .before(crate::ui::chat_input::toggle_chat_focus)
                    .before(crate::scripting::systems::toggle_python_console),
            )
            .add_systems(
                Update,
                (
                    sync_settings_overlay_visibility,
                    sync_binding_row_labels,
                    handle_binding_row_clicks,
                    handle_settings_close_button,
                    handle_settings_reset_button,
                    handle_settings_scroll,
                    handle_tab_clicks,
                    sync_section_visibility,
                    sync_option_row_labels,
                    handle_option_row_clicks,
                    sync_gameplay_option_row_labels,
                    handle_gameplay_option_row_clicks,
                    apply_display_settings,
                ),
            )
            .add_systems(Last, persist_settings);
    }
}
