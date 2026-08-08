use std::fs;
use std::io::Write as _;
use std::path::Path;

use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::prelude::*;
use bevy_terminal::{LineStyle, Terminal, TerminalFocus, TerminalSubmit};

use crate::app::paths::python_history_path;
use crate::app::plugin::AppRuntime;
use crate::game::commands::GameCommand;
use crate::game::resources::ClientPendingCommands;
use crate::scripting::resources::{PythonConsoleState, PythonHistoryPersist};
use crate::ui::components::{
    PythonConsoleMaximizeButton, PythonConsolePanel, PythonConsoleRestartButton,
    PythonConsoleTerminal,
};
use crate::ui::PYTHON_CONSOLE_FOCUS_ID;

/// Toggle the Python console on backtick. Lives outside the
/// `terminal_not_focused` run-condition so the same keystroke can both
/// open *and* close the console. Escape closes when focused.
pub fn toggle_python_console(
    mut key_events: MessageReader<KeyboardInput>,
    keybindings: Res<crate::ui::settings::Keybindings>,
    mut console_state: ResMut<PythonConsoleState>,
    mut focus: ResMut<TerminalFocus>,
    mut panel_query: Query<&mut Node, With<PythonConsolePanel>>,
) {
    let toggle_key = keybindings.console_toggle_key();
    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        let want_toggle = Some(event.key_code) == toggle_key;
        let want_close = matches!(event.key_code, KeyCode::Escape) && console_state.is_open;
        if !want_toggle && !want_close {
            continue;
        }
        if want_toggle {
            console_state.is_open = !console_state.is_open;
        } else {
            console_state.is_open = false;
        }
        // Update focus and visibility together so neither lags by a frame.
        focus.focused = if console_state.is_open {
            Some(PYTHON_CONSOLE_FOCUS_ID)
        } else {
            None
        };
        // Record the toggle key so `terminal_input` (same PreUpdate, ordered
        // after this system) drops it instead of inserting a backtick into
        // the freshly-focused buffer. Only needed on open — closing the
        // console flips focus to None, in which case `terminal_input` early-
        // returns and drains by itself.
        if console_state.is_open {
            focus.absorbed_key = Some(event.key_code);
        }
        for mut node in &mut panel_query {
            node.display = if console_state.is_open {
                Display::Flex
            } else {
                Display::None
            };
        }
        break;
    }
}

/// Take `TerminalSubmit` events from the console terminal and ship each line
/// to the server-side REPL as `GameCommand::AdminExec`. The reply arrives as
/// `GameUiEvent::ReplOutput` and is rendered by `apply_game_ui_events`;
/// execution is gated server-side on the account's admin flag.
pub fn handle_python_console_submissions(
    mut submissions: MessageReader<TerminalSubmit>,
    mut outbox: ResMut<ClientPendingCommands>,
    mut terminals: Query<&mut Terminal, With<PythonConsoleTerminal>>,
) {
    for submission in submissions.read() {
        let Ok(mut terminal) = terminals.get_mut(submission.terminal) else {
            continue;
        };
        // Echo the input as a prompt line so users see history mid-stream.
        terminal.push(format!(">>> {}", submission.text), LineStyle::Prompt);

        // IPython-style `expr?` rewrites to `help(expr)` before compiling
        // (`?` is not valid Python, so it must be expanded here).
        let code =
            expand_help_shortcut(&submission.text).unwrap_or_else(|| submission.text.clone());

        outbox.push(GameCommand::AdminExec { code });
    }
}

/// Cap on how many history entries we keep on disk and seed into the terminal,
/// so the file (and the in-memory `Vec`) stay bounded across sessions.
const MAX_HISTORY: usize = 1000;

/// Seed the console terminal's in-memory command history from disk the first
/// time we see its entity (re-seeds if the HUD is rebuilt and spawns a new
/// terminal). Mirrors the load-once idiom of `load_quickbar_on_login`.
pub fn load_python_console_history(
    runtime: Res<AppRuntime>,
    mut persist: ResMut<PythonHistoryPersist>,
    mut terminals: Query<(Entity, &mut Terminal), With<PythonConsoleTerminal>>,
) {
    let Some((entity, mut terminal)) = terminals.iter_mut().next() else {
        return;
    };
    if persist.loaded_into == Some(entity) {
        return;
    }
    let Some(path) = python_history_path(*runtime) else {
        persist.loaded_into = Some(entity);
        return;
    };

    let mut lines = read_history_lines(&path);
    // Bound the on-disk file so it can't grow without limit across sessions.
    if lines.len() > MAX_HISTORY {
        let start = lines.len() - MAX_HISTORY;
        lines.drain(..start);
        rewrite_history_file(&path, &lines);
    }
    persist.last_written = lines.last().cloned();
    terminal.input.history = lines;
    persist.loaded_into = Some(entity);
}

/// Append each submitted console command to the on-disk history file. Reads
/// `TerminalSubmit` with its own cursor, independent of
/// `handle_python_console_submissions`, and ignores submits from other
/// terminals (e.g. chat). Consecutive duplicates collapse to one line.
pub fn persist_python_console_history(
    mut submissions: MessageReader<TerminalSubmit>,
    runtime: Res<AppRuntime>,
    mut persist: ResMut<PythonHistoryPersist>,
    consoles: Query<Entity, With<PythonConsoleTerminal>>,
) {
    let Some(path) = python_history_path(*runtime) else {
        return;
    };
    for submission in submissions.read() {
        if !consoles.contains(submission.terminal) {
            continue;
        }
        if persist.last_written.as_deref() == Some(submission.text.as_str()) {
            continue;
        }
        append_history_line(&path, &submission.text);
        persist.last_written = Some(submission.text.clone());
    }
}

/// Read the history file as one command per line, dropping blanks. Missing or
/// unreadable file yields an empty history.
fn read_history_lines(path: &Path) -> Vec<String> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.to_owned())
        .collect()
}

/// Append a single command line, creating the parent directory if needed.
fn append_history_line(path: &Path, line: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            if let Err(err) = writeln!(file, "{line}") {
                warn!("failed to append python history {}: {err}", path.display());
            }
        }
        Err(err) => warn!("failed to open python history {}: {err}", path.display()),
    }
}

/// Overwrite the history file with `lines` (used to trim an oversized file).
fn rewrite_history_file(path: &Path, lines: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut body = lines.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    if let Err(err) = fs::write(path, body) {
        warn!("failed to rewrite python history {}: {err}", path.display());
    }
}

/// Restart-button click handler. Asks the server-side REPL to rebuild its
/// interpreter scope (`AdminReplReset`); the confirmation line arrives as a
/// `ReplOutput` UI event.
pub fn handle_python_console_restart_button(
    interactions: Query<&Interaction, (With<PythonConsoleRestartButton>, Changed<Interaction>)>,
    mut outbox: ResMut<ClientPendingCommands>,
) {
    let pressed = interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed));
    if !pressed {
        return;
    }
    outbox.push(GameCommand::AdminReplReset);
}

/// Maximize/Restore-button click handler. Flips `PythonConsoleState::maximized`;
/// `sync_bottom_panels_visibility` reads the flag and grows the chat/console
/// area to cover most of the screen.
pub fn handle_python_console_maximize_button(
    interactions: Query<&Interaction, (With<PythonConsoleMaximizeButton>, Changed<Interaction>)>,
    mut console_state: ResMut<PythonConsoleState>,
) {
    let pressed = interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed));
    if pressed {
        console_state.maximized = !console_state.maximized;
    }
}

/// Keep the Maximize/Restore button's label in step with the console state.
/// Runs only when the state actually changes.
pub fn sync_python_console_maximize_label(
    console_state: Res<PythonConsoleState>,
    buttons: Query<&Children, With<PythonConsoleMaximizeButton>>,
    mut texts: Query<&mut Text>,
) {
    if !console_state.is_changed() {
        return;
    }
    let label = if console_state.maximized {
        "Restore"
    } else {
        "Maximize"
    };
    for children in &buttons {
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                if text.0 != label {
                    text.0 = label.to_owned();
                }
            }
        }
    }
}

/// IPython-style `?` help shortcut. `world.spawn?` → `help(world.spawn)`,
/// `?world.spawn` → the same, and a bare `?` → `help()`. Only triggers when
/// the `?` bracket the statement (leading and/or trailing) so a `?` buried in
/// a string or expression (`print("a?b")`) is left to run as-is. Returns the
/// rewritten code, or `None` to run the input unchanged.
fn expand_help_shortcut(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !trimmed.contains('?') {
        return None;
    }
    let target = trimmed.trim_matches('?').trim();
    // A surviving interior `?` means it wasn't a bracketing help marker.
    if target.contains('?') {
        return None;
    }
    if target.is_empty() {
        Some("help()".to_owned())
    } else {
        Some(format!("help({target})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_shortcut_rewrites_trailing_and_leading_question() {
        assert_eq!(
            expand_help_shortcut("world.spawn?").as_deref(),
            Some("help(world.spawn)")
        );
        assert_eq!(
            expand_help_shortcut("?world.spawn").as_deref(),
            Some("help(world.spawn)")
        );
        // `??` (IPython source view) collapses to plain help here.
        assert_eq!(
            expand_help_shortcut("world.player()??").as_deref(),
            Some("help(world.player())")
        );
        assert_eq!(expand_help_shortcut("?").as_deref(), Some("help()"));
    }

    #[test]
    fn help_shortcut_ignores_plain_and_interior_question() {
        assert_eq!(expand_help_shortcut("world.spawn"), None);
        assert_eq!(expand_help_shortcut("  "), None);
        // `?` inside a string/expression is not a help marker.
        assert_eq!(expand_help_shortcut("print(\"a?b\")"), None);
    }
}
