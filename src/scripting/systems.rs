use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::game::resources::PendingGameCommands;
use crate::player::components::{Player, PlayerIdentity};
use crate::scripting::python::PythonConsoleHost;
use crate::scripting::resources::PythonConsoleState;
use crate::scripting_api::build::WorldSnapshotParams;
use crate::ui::components::{
    PythonConsoleInput, PythonConsoleOutput, PythonConsolePanel, PythonConsoleScrollbarThumb,
};

pub fn handle_python_console_input(
    mut keyboard_input_events: MessageReader<KeyboardInput>,
    mut mouse_wheel_events: MessageReader<MouseWheel>,
    mut console_state: ResMut<PythonConsoleState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut host: NonSendMut<PythonConsoleHost>,
    snapshot_params: WorldSnapshotParams,
    local_player_query: Query<&PlayerIdentity, With<Player>>,
) {
    if console_state.is_open {
        for event in mouse_wheel_events.read() {
            if event.y > 0.0 {
                console_state.scroll_up(event.y.ceil() as usize * 3);
            } else if event.y < 0.0 {
                console_state.scroll_down(event.y.abs().ceil() as usize * 3);
            }
        }
    }

    for event in keyboard_input_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        if event.key_code == KeyCode::Backquote {
            console_state.is_open = !console_state.is_open;
            console_state.history_index = None;
            console_state.scroll_offset = 0;
            continue;
        }

        if !console_state.is_open {
            continue;
        }

        match event.key_code {
            KeyCode::Escape => {
                console_state.is_open = false;
                console_state.history_index = None;
            }
            KeyCode::Enter => {
                let command = console_state.input.trim().to_owned();
                if command.is_empty() {
                    console_state.input.clear();
                    continue;
                }

                console_state.push_output(format!(">>> {command}"));
                console_state.history.push(command.clone());
                console_state.history_index = None;
                console_state.input.clear();

                let caller = local_player_query.iter().next().map(|identity| identity.id);
                let snapshot = snapshot_params.build_for_player(caller);

                let queued = host.execute(&mut console_state, &command, snapshot);
                for cmd in queued {
                    match caller {
                        Some(id) => pending_commands.push_for_player(id, cmd),
                        None => pending_commands.push(cmd),
                    }
                }
            }
            KeyCode::Backspace => {
                console_state.input.pop();
            }
            KeyCode::ArrowUp => {
                history_up(&mut console_state);
            }
            KeyCode::ArrowDown => {
                history_down(&mut console_state);
            }
            KeyCode::PageUp => {
                scroll_page_up(&mut console_state);
            }
            KeyCode::PageDown => {
                scroll_page_down(&mut console_state);
            }
            KeyCode::Tab => {}
            _ => {
                if event.repeat {
                    continue;
                }

                match &event.logical_key {
                    Key::PageUp => {
                        scroll_page_up(&mut console_state);
                    }
                    Key::PageDown => {
                        scroll_page_down(&mut console_state);
                    }
                    Key::Character(character) => {
                        console_state.input.push_str(character.as_str());
                    }
                    Key::Space => {
                        console_state.input.push(' ');
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn refresh_python_console_ui(
    console_state: Res<PythonConsoleState>,
    mut panel_query: Query<&mut Visibility, With<PythonConsolePanel>>,
    mut output_query: Query<&mut Text, (With<PythonConsoleOutput>, Without<PythonConsoleInput>)>,
    mut input_query: Query<&mut Text, (With<PythonConsoleInput>, Without<PythonConsoleOutput>)>,
    mut scrollbar_query: Query<
        (&mut Node, &mut Visibility),
        (
            With<PythonConsoleScrollbarThumb>,
            Without<PythonConsolePanel>,
        ),
    >,
) {
    let Ok(mut panel_visibility) = panel_query.single_mut() else {
        return;
    };
    let Ok(mut output_text) = output_query.single_mut() else {
        return;
    };
    let Ok(mut input_text) = input_query.single_mut() else {
        return;
    };
    let Ok((mut scrollbar_node, mut scrollbar_visibility)) = scrollbar_query.single_mut() else {
        return;
    };

    *panel_visibility = if console_state.is_open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    output_text.0 = console_state.rendered_output();
    input_text.0 = format!(">>> {}", console_state.input);

    let (thumb_fraction, progress) = console_state.scrollbar_metrics();
    *scrollbar_visibility = if thumb_fraction >= 1.0 {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    scrollbar_node.height = percent(thumb_fraction * 100.0);
    scrollbar_node.top = percent((1.0 - thumb_fraction) * progress * 100.0);
}

fn history_up(console_state: &mut PythonConsoleState) {
    if console_state.history.is_empty() {
        return;
    }

    let next_index = match console_state.history_index {
        Some(0) => 0,
        Some(index) => index - 1,
        None => console_state.history.len() - 1,
    };

    console_state.history_index = Some(next_index);
    console_state.input = console_state.history[next_index].clone();
}

fn history_down(console_state: &mut PythonConsoleState) {
    let Some(index) = console_state.history_index else {
        return;
    };

    let next_index = index + 1;
    if next_index >= console_state.history.len() {
        console_state.history_index = None;
        console_state.input.clear();
        return;
    }

    console_state.history_index = Some(next_index);
    console_state.input = console_state.history[next_index].clone();
}

fn scroll_page_up(console_state: &mut PythonConsoleState) {
    let lines = console_state.visible_output_lines.saturating_sub(2).max(1);
    console_state.scroll_up(lines);
}

fn scroll_page_down(console_state: &mut PythonConsoleState) {
    let lines = console_state.visible_output_lines.saturating_sub(2).max(1);
    console_state.scroll_down(lines);
}
