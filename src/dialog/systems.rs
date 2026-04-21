//! Server-side dialog systems: translate `TalkToNpc` / `DialogAdvance` /
//! `DialogChoose` / `DialogEnd` commands into Yarn `DialogueRunner` calls, and
//! translate Yarn presentation events into `GameUiEvent`s for clients.

use bevy::prelude::*;
use bevy_yarnspinner::events::{DialogueCompleted, PresentLine, PresentOptions};
use bevy_yarnspinner::prelude::*;

use crate::dialog::components::{DialogNode, DialogSession};
use crate::dialog::resources::{DialogSessionHandle, DialogSessionRegistry, PendingDialogOptions};
use crate::dialog::yarn_bindings;
use crate::game::commands::GameCommand;
use crate::game::resources::{GameUiEvent, PendingGameCommands, PendingGameUiEvents};
use crate::player::components::{Player, PlayerIdentity};
use crate::world::components::OverworldObject;

/// Drains dialog-flavored `GameCommand`s from `PendingGameCommands` and
/// operates on Yarn runners accordingly. Scheduled ahead of
/// `process_game_commands` so non-dialog systems never see these variants.
///
/// The closed-over player match uses `Option<PlayerId>` because embedded mode
/// does not fill `player_id` on locally queued commands. Falling back to the
/// single `Player` entity is safe in embedded / single-player contexts — TCP
/// pathways always set `player_id` explicitly.
#[allow(clippy::too_many_arguments)]
pub fn process_dialog_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut sessions: ResMut<DialogSessionRegistry>,
    mut pending_options: ResMut<PendingDialogOptions>,
    project: Option<Res<YarnProject>>,
    mut commands: Commands,
    player_query: Query<(Entity, &PlayerIdentity), With<Player>>,
    npc_query: Query<(&OverworldObject, &DialogNode)>,
    mut runners: Query<(&mut DialogueRunner, &DialogSession)>,
) {
    let original_len = pending_commands.commands.len();
    let drained: Vec<_> = pending_commands
        .commands
        .drain(..)
        .map(|queued| {
            let is_dialog = matches!(
                queued.command,
                GameCommand::TalkToNpc { .. }
                    | GameCommand::DialogAdvance { .. }
                    | GameCommand::DialogChoose { .. }
                    | GameCommand::DialogEnd { .. }
            );
            (is_dialog, queued)
        })
        .collect();

    let mut remaining = Vec::with_capacity(original_len);
    for (is_dialog, queued) in drained {
        if !is_dialog {
            remaining.push(queued);
            continue;
        }

        let Some(acting_player_id) = queued
            .player_id
            .or_else(|| player_query.iter().next().map(|(_, identity)| identity.id))
        else {
            continue;
        };

        match queued.command {
            GameCommand::TalkToNpc { npc_object_id } => {
                let Some(project) = project.as_deref() else {
                    bevy::log::warn!(
                        "TalkToNpc ignored: YarnProject not ready yet (still compiling)"
                    );
                    continue;
                };
                let Some(node_name) = npc_query.iter().find_map(|(object, node)| {
                    (object.object_id == npc_object_id).then(|| node.0.clone())
                }) else {
                    bevy::log::warn!(
                        "TalkToNpc ignored: NPC {npc_object_id} missing or has no DialogNode"
                    );
                    continue;
                };

                let mut runner = project.create_dialogue_runner(&mut commands);
                yarn_bindings::install(&mut runner, &mut commands);
                runner.start_node(&node_name);

                let session_id = sessions.allocate();
                let runner_entity = commands
                    .spawn((
                        runner,
                        DialogSession {
                            session_id,
                            player_id: acting_player_id.0,
                            npc_object_id,
                        },
                    ))
                    .id();
                sessions.by_id.insert(
                    session_id,
                    DialogSessionHandle {
                        runner_entity,
                        player_id: acting_player_id.0,
                        npc_object_id,
                    },
                );

                bevy::log::info!(
                    "dialog session {session_id} started (player={} npc={} node={})",
                    acting_player_id.0,
                    npc_object_id,
                    node_name
                );
            }
            GameCommand::DialogAdvance { session_id } => {
                let Some(handle) = sessions.by_id.get(&session_id).copied() else {
                    continue;
                };
                if handle.player_id != acting_player_id.0 {
                    continue;
                }
                if let Ok((mut runner, _)) = runners.get_mut(handle.runner_entity) {
                    runner.continue_in_next_update();
                }
            }
            GameCommand::DialogChoose {
                session_id,
                option_idx,
            } => {
                let Some(handle) = sessions.by_id.get(&session_id).copied() else {
                    continue;
                };
                if handle.player_id != acting_player_id.0 {
                    continue;
                }
                let Some(options) = pending_options.by_session.remove(&session_id) else {
                    bevy::log::warn!("DialogChoose: no pending options for session {session_id}");
                    continue;
                };
                let Some(option_id) = options.get(option_idx).copied() else {
                    bevy::log::warn!(
                        "DialogChoose: option_idx {option_idx} out of range (have {})",
                        options.len()
                    );
                    continue;
                };
                if let Ok((mut runner, _)) = runners.get_mut(handle.runner_entity) {
                    runner.select_option(option_id).ok();
                }
            }
            GameCommand::DialogEnd { session_id } => {
                let Some(handle) = sessions.by_id.get(&session_id).copied() else {
                    continue;
                };
                if handle.player_id != acting_player_id.0 {
                    continue;
                }
                if let Ok((mut runner, _)) = runners.get_mut(handle.runner_entity) {
                    runner.stop();
                }
                // Entity teardown + UI close happen in `handle_dialogue_completed`
                // when Yarn's own DialogueCompleted fires (triggered by stop()).
                let _ = &mut ui_events; // keep dependency expressed
            }
            _ => unreachable!("non-dialog command in dialog drain"),
        }
    }

    pending_commands.commands = remaining;
}

/// Observer: every Yarn `PresentLine` → `GameUiEvent::DialogLine` for the
/// owning player. Yarn 0.8 triggers presentation events on the runner entity
/// (they're `EntityEvent`s), so we read them via Bevy 0.18's `On<T>` observer
/// parameter rather than `MessageReader`.
pub fn forward_present_line(
    event: On<PresentLine>,
    sessions: Query<&DialogSession>,
    mut ui_events: ResMut<PendingGameUiEvents>,
) {
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    let speaker = event.line.character_name().map(str::to_owned);
    let text = event.line.text_without_character_name();
    ui_events.push(
        crate::player::components::PlayerId(session.player_id),
        GameUiEvent::DialogLine {
            session_id: session.session_id,
            speaker,
            text,
        },
    );
}

pub fn forward_present_options(
    event: On<PresentOptions>,
    sessions: Query<&DialogSession>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut pending_options: ResMut<PendingDialogOptions>,
) {
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    let mut texts = Vec::with_capacity(event.options.len());
    let mut ids = Vec::with_capacity(event.options.len());
    for option in &event.options {
        texts.push(option.line.text_without_character_name());
        ids.push(option.id);
    }
    pending_options.by_session.insert(session.session_id, ids);
    ui_events.push(
        crate::player::components::PlayerId(session.player_id),
        GameUiEvent::DialogOptions {
            session_id: session.session_id,
            options: texts,
        },
    );
}

pub fn forward_dialogue_completed(
    event: On<DialogueCompleted>,
    sessions: Query<&DialogSession>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut registry: ResMut<DialogSessionRegistry>,
    mut pending_options: ResMut<PendingDialogOptions>,
    mut commands: Commands,
) {
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    ui_events.push(
        crate::player::components::PlayerId(session.player_id),
        GameUiEvent::DialogClose {
            session_id: session.session_id,
        },
    );
    registry.by_id.remove(&session.session_id);
    pending_options.by_session.remove(&session.session_id);
    commands.entity(event.entity).despawn();
    bevy::log::info!("dialog session {} closed", session.session_id);
}
