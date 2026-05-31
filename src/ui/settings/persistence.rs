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
use super::editor::{EditorAction, EditorKeybindings};
use super::gameplay::GameplaySettings;
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
    #[serde(default)]
    gameplay: GameplayFile,
    #[serde(default)]
    editor: EditorFile,
    #[serde(default)]
    servers: ServersFile,
}

#[derive(Serialize, Deserialize, Default)]
struct EditorFile {
    /// Same shape as `ControlsFile::bindings` — a list of (action, binding)
    /// pairs so missing entries fall back to their compiled defaults.
    #[serde(default)]
    bindings: Vec<EditorActionBinding>,
}

#[derive(Serialize, Deserialize)]
struct EditorActionBinding {
    action: EditorAction,
    binding: Bindings,
}

#[derive(Serialize, Deserialize)]
struct GameplayFile {
    #[serde(default)]
    auto_open_nearby_npcs_panel: bool,
}

impl Default for GameplayFile {
    fn default() -> Self {
        let g = GameplaySettings::default();
        Self {
            auto_open_nearby_npcs_panel: g.auto_open_nearby_npcs_panel,
        }
    }
}

/// Client-side server picker state on disk. The `saved` list is read-only at
/// runtime (hand-edit the file to add entries); `selected_addr` remembers the
/// last picked entry so the next launch defaults to it.
#[derive(Serialize, Deserialize)]
struct ServersFile {
    #[serde(default = "default_saved_servers")]
    saved: Vec<SavedServerEntry>,
    #[serde(default)]
    selected_addr: Option<String>,
}

impl Default for ServersFile {
    fn default() -> Self {
        Self {
            saved: default_saved_servers(),
            selected_addr: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SavedServerEntry {
    pub name: String,
    pub addr: String,
}

fn default_saved_servers() -> Vec<SavedServerEntry> {
    vec![SavedServerEntry {
        name: "Local".to_owned(),
        addr: "127.0.0.1:7000".to_owned(),
    }]
}

/// Resource holding the loaded saved-server list plus the last picked entry.
/// `dirty` is set by the title-screen picker so `persist_settings` flushes the
/// change on the next frame.
#[derive(Resource, Default, Debug)]
pub struct SavedServerList {
    pub saved: Vec<SavedServerEntry>,
    pub selected_addr: Option<String>,
    pub dirty: bool,
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
    mut editor_keys: ResMut<EditorKeybindings>,
    mut display: ResMut<DisplaySettings>,
    mut gameplay: ResMut<GameplaySettings>,
    mut servers: ResMut<SavedServerList>,
    mut loaded: ResMut<SettingsLoaded>,
) {
    if loaded.0 {
        return;
    }
    loaded.0 = true;

    // Always seed the saved-server list — if no file exists or it lacks a
    // `servers` block, this is the single source of truth for "Local".
    servers.saved = default_saved_servers();
    servers.selected_addr = None;
    servers.dirty = false;

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

    servers.saved = file.servers.saved;
    servers.selected_addr = file.servers.selected_addr;

    keybindings.apply_overrides(
        file.controls
            .bindings
            .into_iter()
            .map(|ab| (ab.action, ab.binding)),
        file.controls.movement,
    );
    keybindings.dirty = false;

    editor_keys.apply_overrides(
        file.editor
            .bindings
            .into_iter()
            .map(|ab| (ab.action, ab.binding)),
    );
    editor_keys.dirty = false;

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

    gameplay.auto_open_nearby_npcs_panel = file.gameplay.auto_open_nearby_npcs_panel;
    gameplay.dirty = false;
}

/// `Last`: write when `dirty`. Same shape as `persist_quickbar`.
pub fn persist_settings(
    runtime: Res<AppRuntime>,
    mut keybindings: ResMut<Keybindings>,
    mut editor_keys: ResMut<EditorKeybindings>,
    mut display: ResMut<DisplaySettings>,
    mut gameplay: ResMut<GameplaySettings>,
    mut servers: ResMut<SavedServerList>,
) {
    if !keybindings.dirty
        && !editor_keys.dirty
        && !display.dirty
        && !gameplay.dirty
        && !servers.dirty
    {
        return;
    }
    let Some(path) = client_settings_path(*runtime) else {
        keybindings.dirty = false;
        editor_keys.dirty = false;
        display.dirty = false;
        gameplay.dirty = false;
        servers.dirty = false;
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
        gameplay: GameplayFile {
            auto_open_nearby_npcs_panel: gameplay.auto_open_nearby_npcs_panel,
        },
        editor: EditorFile {
            bindings: editor_keys
                .entries()
                .into_iter()
                .map(|(action, binding)| EditorActionBinding { action, binding })
                .collect(),
        },
        servers: ServersFile {
            saved: servers.saved.clone(),
            selected_addr: servers.selected_addr.clone(),
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
    editor_keys.dirty = false;
    display.dirty = false;
    gameplay.dirty = false;
    servers.dirty = false;
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
            gameplay: GameplayFile::default(),
            editor: EditorFile::default(),
            servers: ServersFile {
                saved: default_saved_servers(),
                selected_addr: None,
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
    fn settings_file_without_servers_block_seeds_local() {
        // A file written before the servers block was introduced must
        // populate the `Local` seed on read.
        let json = r#"{"controls":{"bindings":[],"movement":null},"display":{}}"#;
        let parsed: SettingsFile = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.servers.saved.len(), 1);
        assert_eq!(parsed.servers.saved[0].name, "Local");
        assert_eq!(parsed.servers.saved[0].addr, "127.0.0.1:7000");
    }

    #[test]
    fn servers_block_round_trips() {
        let original = ServersFile {
            saved: vec![
                SavedServerEntry {
                    name: "Local".into(),
                    addr: "127.0.0.1:7000".into(),
                },
                SavedServerEntry {
                    name: "Prod".into(),
                    addr: "mud.example.com:7000".into(),
                },
            ],
            selected_addr: Some("mud.example.com:7000".into()),
        };
        let file = SettingsFile {
            controls: ControlsFile::default(),
            display: DisplayFile::default(),
            gameplay: GameplayFile::default(),
            editor: EditorFile::default(),
            servers: original,
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        let parsed: SettingsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.servers.saved.len(), 2);
        assert_eq!(parsed.servers.saved[1].addr, "mud.example.com:7000");
        assert_eq!(
            parsed.servers.selected_addr.as_deref(),
            Some("mud.example.com:7000")
        );
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
    fn settings_file_without_editor_block_keeps_defaults() {
        // Files written before the editor block was introduced must keep the
        // default editor bindings on load.
        let json = r#"{"controls":{"bindings":[],"movement":null},"display":{}}"#;
        let parsed: SettingsFile = serde_json::from_str(json).unwrap();
        assert!(parsed.editor.bindings.is_empty());

        let mut ek = EditorKeybindings::default();
        ek.apply_overrides(
            parsed
                .editor
                .bindings
                .into_iter()
                .map(|ab| (ab.action, ab.binding)),
        );
        // Default for ToolBrush is Digit1.
        assert_eq!(
            ek.bindings(EditorAction::ToolBrush).primary,
            Some(super::super::model::Binding::plain(KeyCode::Digit1))
        );
    }

    #[test]
    fn editor_bindings_round_trip() {
        let mut ek = EditorKeybindings::default();
        ek.rebind_action(
            EditorAction::Eyedropper,
            super::super::model::Binding::plain(KeyCode::KeyQ),
        );
        let file = SettingsFile {
            controls: ControlsFile::default(),
            display: DisplayFile::default(),
            gameplay: GameplayFile::default(),
            editor: EditorFile {
                bindings: ek
                    .entries()
                    .into_iter()
                    .map(|(action, binding)| EditorActionBinding { action, binding })
                    .collect(),
            },
            servers: ServersFile {
                saved: default_saved_servers(),
                selected_addr: None,
            },
        };
        let json = serde_json::to_string_pretty(&file).unwrap();
        let parsed: SettingsFile = serde_json::from_str(&json).unwrap();
        let mut restored = EditorKeybindings::default();
        restored.apply_overrides(
            parsed
                .editor
                .bindings
                .into_iter()
                .map(|ab| (ab.action, ab.binding)),
        );
        assert_eq!(
            restored.bindings(EditorAction::Eyedropper).primary,
            Some(super::super::model::Binding::plain(KeyCode::KeyQ))
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
