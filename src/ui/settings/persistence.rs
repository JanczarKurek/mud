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

use super::model::{Action, Bindings, Keybindings, MovementBindings};

/// On-disk schema. `#[serde(default)]` everywhere so older/newer files with
/// missing fields still parse, and unknown actions simply fall back to the
/// in-memory default (we merge *over* `Keybindings::default()`).
#[derive(Serialize, Deserialize, Default)]
struct SettingsFile {
    #[serde(default)]
    controls: ControlsFile,
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
}

/// `Last`: write when `dirty`. Same shape as `persist_quickbar`.
pub fn persist_settings(runtime: Res<AppRuntime>, mut keybindings: ResMut<Keybindings>) {
    if !keybindings.dirty {
        return;
    }
    let Some(path) = client_settings_path(*runtime) else {
        keybindings.dirty = false;
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
