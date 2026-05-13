use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::prelude::*;
use bevy_terminal::{Terminal, TerminalFocus, TerminalSubmit};

use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::player::components::{Player, PlayerIdentity};
use crate::ui::components::ChatTerminal;
use crate::ui::CHAT_TERMINAL_FOCUS_ID;

/// Focus / unfocus the chat input terminal on `T` (open) and `Escape`
/// (close). Mirrors `toggle_python_console`: runs in `PreUpdate` *before*
/// `bevy_terminal::terminal_input`, and records the toggle key in
/// `TerminalFocus::absorbed_key` so the same `T` press doesn't also insert a
/// `t` into the freshly-focused input row.
pub fn toggle_chat_focus(
    mut key_events: MessageReader<KeyboardInput>,
    mut focus: ResMut<TerminalFocus>,
    mut chat_terminal: Query<&mut Terminal, With<ChatTerminal>>,
) {
    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::KeyT if focus.focused.is_none() => {
                focus.focused = Some(CHAT_TERMINAL_FOCUS_ID);
                focus.absorbed_key = Some(KeyCode::KeyT);
            }
            KeyCode::Escape if focus.focused == Some(CHAT_TERMINAL_FOCUS_ID) => {
                focus.focused = None;
                if let Ok(mut terminal) = chat_terminal.single_mut() {
                    terminal.set_input(String::new());
                }
            }
            _ => {}
        }
    }
}

/// Take `TerminalSubmit` events from the chat terminal and forward the text
/// to the server as `GameCommand::Say`. Focus stays on the chat input so the
/// player can fire follow-up messages without re-toggling — `Escape`
/// (handled by `toggle_chat_focus`) is the explicit exit. Submissions from
/// other terminals are ignored.
pub fn handle_chat_submissions(
    mut submissions: MessageReader<TerminalSubmit>,
    chat_terminal: Query<Entity, With<ChatTerminal>>,
    local_player: Query<&PlayerIdentity, With<Player>>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    let chat_entity = chat_terminal.single().ok();
    for submission in submissions.read() {
        if Some(submission.terminal) != chat_entity {
            continue;
        }
        let text = submission.text.clone();
        let caller = local_player.iter().next().map(|identity| identity.id);
        let command = GameCommand::Say { text };
        match caller {
            Some(id) => pending_commands.push_for_player(id, command),
            None => pending_commands.push(command),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::Messages;

    use crate::player::components::{ChatLog, PlayerId};
    use crate::world::components::{SpaceId, SpaceResident, TilePosition};

    fn build_test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<TerminalFocus>();
        app.add_message::<TerminalSubmit>();
        app.insert_resource(PendingGameCommands::default());
        app.add_systems(Update, handle_chat_submissions);
        app
    }

    #[test]
    fn submission_from_chat_terminal_pushes_say_command() {
        let mut app = build_test_app();
        let chat_entity = app.world_mut().spawn(ChatTerminal).id();
        app.world_mut().spawn((
            Player,
            PlayerIdentity::with_display_name(PlayerId(7), "alice".to_owned()),
            ChatLog::default(),
            SpaceResident {
                space_id: SpaceId(0),
            },
            TilePosition::ground(0, 0),
        ));

        app.world_mut()
            .resource_mut::<Messages<TerminalSubmit>>()
            .write(TerminalSubmit {
                terminal: chat_entity,
                text: "hello".to_owned(),
            });

        app.update();

        let pending = app.world().resource::<PendingGameCommands>();
        assert_eq!(pending.commands.len(), 1, "exactly one Say queued");
        let queued = &pending.commands[0];
        assert_eq!(queued.player_id, Some(PlayerId(7)));
        match &queued.command {
            GameCommand::Say { text } => assert_eq!(text, "hello"),
            other => panic!("expected Say, got {other:?}"),
        }
    }

    #[test]
    fn submission_from_other_terminal_is_ignored() {
        let mut app = build_test_app();
        let _chat = app.world_mut().spawn(ChatTerminal).id();
        let unrelated = app.world_mut().spawn_empty().id();

        app.world_mut()
            .resource_mut::<Messages<TerminalSubmit>>()
            .write(TerminalSubmit {
                terminal: unrelated,
                text: "ignored".to_owned(),
            });

        app.update();

        assert!(app
            .world()
            .resource::<PendingGameCommands>()
            .commands
            .is_empty());
    }
}
