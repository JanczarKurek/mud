//! Load/save the global client settings file. Mirrors the proven
//! `ui::quickbar` pattern (serde_json pretty, `fs::create_dir_all`,
//! `dirty`-gated write) but is client-wide rather than per-character, so it
//! loads once at `Startup` instead of on login — keybindings must be live on
//! the title screen too.

use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::paths::client_settings_path;
use crate::app::plugin::AppRuntime;

use super::display::{DisplaySettings, WindowModeSetting};
use super::model::{Action, Bindings, Keybindings, MovementBindings};

/// On-disk schema. `#[serde(default)]` everywhere so older/newer files with
/// missing fields still parse, and unknown actions simply fall back to the
/// in-memory default (we merge *over* `Keybindings::default()`).
#[derive(Serialize, Deserialize, Default)]
struct SettingsFile {
    #[serde(default)]
    controls: ControlsFile,
    #[serde(default)]
    display: DisplayFile,
}

/// `#[serde(default = …)]` on each field handles a *partial* `display` block
/// (some keys missing). The hand-written `Default` handles the *whole block*
/// missing — `#[derive(Default)]` would ignore the serde attrs and yield
/// `ui_scale: 0.0`, which collapses the entire UI. Keep both in sync with
/// `DisplaySettings::default()`.
#[derive(Serialize, Deserialize)]
struct DisplayFile {
    #[serde(default)]
    window_mode: WindowModeSetting,
    #[serde(default = "default_true")]
    vsync: bool,
    #[serde(default = "default_ui_scale")]
    ui_scale: f32,
}

impl Default for DisplayFile {
    fn default() -> Self {
        let d = DisplaySettings::default();
        Self {
            window_mode: d.window_mode,
            vsync: d.vsync,
            ui_scale: d.ui_scale,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_ui_scale() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize, Default)]
struct ControlsFile {
    /// A list (not a map) because `Action` is an enum and JSON map keys must
    /// be strings — a `Vec` of `{action, binding}` keeps the file readable.
    #[serde(default)]
    bindings: Vec<ActionBinding>,
    #[serde(default)]
    movement: Option<MovementBindings>,
}

#[derive(Serialize, Deserialize)]
struct ActionBinding {
    action: Action,
    binding: Bindings,
}

/// Once-only load guard (mirrors `QuickbarLoadedFor`'s intent).
#[derive(Resource, Default)]
pub struct SettingsLoaded(pub bool);

/// `Startup`: read the settings file (if any) and merge it over the default
/// keybindings. Silent no-op for `HeadlessServer` (no path) or a missing /
/// corrupt file (defaults stand).
pub fn load_settings(
    runtime: Res<AppRuntime>,
    mut keybindings: ResMut<Keybindings>,
    mut display: ResMut<DisplaySettings>,
    mut loaded: ResMut<SettingsLoaded>,
) {
    if loaded.0 {
        return;
    }
    loaded.0 = true;

    let Some(path) = client_settings_path(*runtime) else {
        return;
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(file) = serde_json::from_str::<SettingsFile>(&raw) else {
        warn!(
            "settings file {} is corrupt; using defaults",
            path.display()
        );
        return;
    };

    keybindings.apply_overrides(
        file.controls
            .bindings
            .into_iter()
            .map(|ab| (ab.action, ab.binding)),
        file.controls.movement,
    );
    keybindings.dirty = false;

    // Mutating through `ResMut` flags the resource changed, so
    // `apply_display_settings` picks the persisted values up on the first
    // frame even though `dirty` stays false (nothing to re-write yet).
    display.window_mode = file.display.window_mode;
    display.vsync = file.display.vsync;
    // A zero / NaN / wildly-off scale collapses the whole UI — never trust
    // the file blindly (it may be hand-edited or from an older buggy build).
    display.ui_scale = if file.display.ui_scale.is_finite() {
        file.display.ui_scale.clamp(0.5, 3.0)
    } else {
        1.0
    };
    display.dirty = false;
}

/// `Last`: write when `dirty`. Same shape as `persist_quickbar`.
pub fn persist_settings(
    runtime: Res<AppRuntime>,
    mut keybindings: ResMut<Keybindings>,
    mut display: ResMut<DisplaySettings>,
) {
    if !keybindings.dirty && !display.dirty {
        return;
    }
    let Some(path) = client_settings_path(*runtime) else {
        keybindings.dirty = false;
        display.dirty = false;
        return;
    };

    let file = SettingsFile {
        controls: ControlsFile {
            bindings: keybindings
                .entries()
                .into_iter()
                .map(|(action, binding)| ActionBinding { action, binding })
                .collect(),
            movement: Some(keybindings.movement.clone()),
        },
        display: DisplayFile {
            window_mode: display.window_mode,
            vsync: display.vsync,
            ui_scale: display.ui_scale,
        },
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(err) = fs::write(&path, json) {
                warn!("failed to write settings file {}: {err}", path.display());
            }
        }
        Err(err) => warn!("failed to serialize settings: {err}"),
    }
    keybindings.dirty = false;
    display.dirty = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::KeyCode;

    use super::super::model::Binding;

    #[test]
    fn round_trips_through_json() {
        let mut kb = Keybindings::default();
        kb.rebind_action(Action::SetHome, Binding::plain(KeyCode::KeyZ));

        let file = SettingsFile {
            controls: ControlsFile {
                bindings: kb
                    .entries()
                    .into_iter()
                    .map(|(action, binding)| ActionBinding { action, binding })
                    .collect(),
                movement: Some(kb.movement.clone()),
            },
            display: DisplayFile::default(),
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        let parsed: SettingsFile = serde_json::from_str(&json).unwrap();

        let mut restored = Keybindings::default();
        restored.apply_overrides(
            parsed
                .controls
                .bindings
                .into_iter()
                .map(|ab| (ab.action, ab.binding)),
            parsed.controls.movement,
        );
        assert_eq!(
            restored.bindings(Action::SetHome).primary,
            Some(Binding::plain(KeyCode::KeyZ))
        );
    }

    #[test]
    fn settings_file_without_display_block_is_sane() {
        // The exact shape the pre-Display Settings commit wrote: a
        // controls-only file. Regression for the `ui_scale: 0.0` UI collapse.
        let json = r#"{"controls":{"bindings":[],"movement":null}}"#;
        let parsed: SettingsFile = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.display.ui_scale, 1.0);
        assert!(parsed.display.vsync);
        assert_eq!(parsed.display.window_mode, WindowModeSetting::Windowed);
    }

    #[test]
    fn partial_display_block_fills_missing_fields() {
        // Only window_mode present → vsync/ui_scale fall back to sane serde
        // defaults, not 0.
        let json = r#"{"display":{"window_mode":"BorderlessFullscreen"}}"#;
        let parsed: SettingsFile = serde_json::from_str(json).unwrap();
        assert_eq!(
            parsed.display.window_mode,
            WindowModeSetting::BorderlessFullscreen
        );
        assert!(parsed.display.vsync);
        assert_eq!(parsed.display.ui_scale, 1.0);
    }

    #[test]
    fn missing_action_in_file_keeps_default() {
        // A file that only specifies SetHome must not wipe other actions.
        let json = r#"{"controls":{"bindings":[{"action":"SetHome","binding":{"primary":{"key":"Z","mods":{"ctrl":false,"shift":false,"alt":false}},"secondary":null}}]}}"#;
        let parsed: SettingsFile = serde_json::from_str(json).unwrap();
        let mut kb = Keybindings::default();
        kb.apply_overrides(
            parsed
                .controls
                .bindings
                .into_iter()
                .map(|ab| (ab.action, ab.binding)),
            parsed.controls.movement,
        );
        assert_eq!(
            kb.bindings(Action::SetHome).primary,
            Some(Binding::plain(KeyCode::KeyZ))
        );
        assert_eq!(
            kb.bindings(Action::ToggleFullMap).primary,
            Some(Binding::plain(KeyCode::KeyM))
        );
    }
}
