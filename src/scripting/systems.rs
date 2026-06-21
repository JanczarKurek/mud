use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::prelude::*;
use bevy_terminal::{
    LineStyle, Terminal, TerminalCompletionRequest, TerminalFocus, TerminalSubmit,
};

use crate::game::resources::PendingGameCommands;
use crate::player::components::{Player, PlayerIdentity};
use crate::scripting::python::PythonConsoleHost;
use crate::scripting::resources::PythonConsoleState;
use crate::scripting_api::build::WorldSnapshotParams;
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

/// Take `TerminalSubmit` events from the console terminal, run them through
/// `PythonConsoleHost`, and push the resulting output lines back into the
/// `Terminal` component. Queued `GameCommand`s are dispatched through
/// `PendingGameCommands` using the same caller routing the old handler used.
pub fn handle_python_console_submissions(
    mut submissions: MessageReader<TerminalSubmit>,
    mut host: NonSendMut<PythonConsoleHost>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut terminals: Query<&mut Terminal, With<PythonConsoleTerminal>>,
    snapshot_params: WorldSnapshotParams,
    local_player_query: Query<&PlayerIdentity, With<Player>>,
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

        let caller = local_player_query.iter().next().map(|identity| identity.id);
        let snapshot = snapshot_params.build_for_player(caller);
        let output = host.execute(&code, snapshot);

        for (line, style) in output.lines {
            terminal.push(line, style);
        }
        for cmd in output.commands {
            match caller {
                Some(id) => pending_commands.push_for_player(id, cmd),
                None => pending_commands.push(cmd),
            }
        }
        for (target, cmd) in output.targeted_commands {
            pending_commands.push_for_player(target, cmd);
        }
    }
}

/// Resolve a Tab completion request against the persistent scope. A unique
/// match auto-fills the input via `Terminal::replace_input_token`; multiple
/// candidates list themselves as a system-styled output line.
pub fn handle_python_console_completion(
    mut requests: MessageReader<TerminalCompletionRequest>,
    host: NonSend<PythonConsoleHost>,
    mut terminals: Query<&mut Terminal, With<PythonConsoleTerminal>>,
) {
    for request in requests.read() {
        let Ok(mut terminal) = terminals.get_mut(request.terminal) else {
            continue;
        };
        // `complete_at` sees the full prefix so it can resolve dotted access
        // (`world.sp` → `spawn`); `token` is just the trailing identifier we
        // replace, so the `world.` part is left intact.
        let token = trailing_identifier(&request.text_before_cursor);
        let mut matches = host.complete_at(&request.text_before_cursor);
        matches.retain(|m| !m.starts_with('_'));
        match matches.as_slice() {
            [] => {}
            [single] => {
                let single = single.clone();
                terminal.replace_input_token(token.chars().count(), &single);
            }
            many => {
                terminal.push(
                    format!("[completions] {}", summarize(many)),
                    LineStyle::System,
                );
                if let Some(prefix) = common_prefix(many) {
                    if prefix.len() > token.len() {
                        terminal.replace_input_token(token.chars().count(), &prefix);
                    }
                }
            }
        }
    }
}

/// Render a candidate list for the `[completions]` line, capping long lists
/// (e.g. bare `world.` lists every member) so the terminal stays readable.
fn summarize(candidates: &[String]) -> String {
    const MAX: usize = 30;
    if candidates.len() <= MAX {
        candidates.join(" ")
    } else {
        format!(
            "{} … (+{} more)",
            candidates[..MAX].join(" "),
            candidates.len() - MAX
        )
    }
}

/// Restart-button click handler. Rebuilds the embedded interpreter scope
/// from scratch (same effect as running `world.reset()` from inside the
/// REPL) and prints a one-line confirmation into the terminal buffer.
pub fn handle_python_console_restart_button(
    interactions: Query<&Interaction, (With<PythonConsoleRestartButton>, Changed<Interaction>)>,
    mut host: NonSendMut<PythonConsoleHost>,
    mut terminals: Query<&mut Terminal, With<PythonConsoleTerminal>>,
) {
    let pressed = interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed));
    if !pressed {
        return;
    }
    host.reset_scope();
    for mut terminal in &mut terminals {
        terminal.push("[System] interpreter restarted.", LineStyle::System);
    }
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

/// Pull the last identifier-ish token off the buffer. Stops at the first
/// non-identifier char scanning right-to-left so `world.gi` returns `gi`.
fn trailing_identifier(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut split = bytes.len();
    for (i, b) in bytes.iter().enumerate().rev() {
        if b.is_ascii_alphanumeric() || *b == b'_' {
            split = i;
        } else {
            break;
        }
    }
    &input[split..]
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

fn common_prefix(strings: &[String]) -> Option<String> {
    let first = strings.first()?;
    let mut prefix_len = first.len();
    for s in &strings[1..] {
        prefix_len = prefix_len.min(s.len());
        for (i, (a, b)) in first.bytes().zip(s.bytes()).enumerate() {
            if i >= prefix_len {
                break;
            }
            if a != b {
                prefix_len = i;
                break;
            }
        }
    }
    Some(first[..prefix_len].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_identifier_finds_last_token() {
        assert_eq!(trailing_identifier("wor"), "wor");
        assert_eq!(trailing_identifier("world.gi"), "gi");
        assert_eq!(trailing_identifier("a + b.c"), "c");
        assert_eq!(trailing_identifier(""), "");
        assert_eq!(trailing_identifier("  "), "");
    }

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

    #[test]
    fn common_prefix_handles_empty_and_full_match() {
        let strings = vec!["world".into(), "worker".into(), "won".into()];
        assert_eq!(common_prefix(&strings).as_deref(), Some("wo"));
        let single = vec!["only".into()];
        assert_eq!(common_prefix(&single).as_deref(), Some("only"));
    }
}
