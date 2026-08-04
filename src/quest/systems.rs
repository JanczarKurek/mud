//! Glue between Bevy ECS and `QuestEngine`.
//!
//! `drain_quest_commands` runs each frame:
//!   1. Consumes `PendingQuestCommands` entries queued by the Yarn observer.
//!   2. For each, builds a `QuestApiContext` (snapshot + var store + caller
//!      identity), installs it via `scripting_api::install_ctx`, and invokes
//!      the matching engine hook.
//!   3. Drains the queued effects (`GameCommand`s, completed/failed quest
//!      ids, log lines) back into the world.
//!
//! `drain_quest_events` does the same for `PendingQuestEvents`.

use std::sync::Arc;

use bevy::prelude::*;
use bevy_yarnspinner::events::ExecuteCommand;
use bevy_yarnspinner::prelude::YarnValue;

use crate::crafting::CharacterStash;
use crate::dialog::components::DialogSession;
use crate::dialog::resources::CharacterVarStores;
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::log::{LogOwner, LogState, QUESTS_SECTION};
use crate::player::components::{Player, PlayerId, PlayerIdentity};
use crate::quest::engine::QuestEngine;
use crate::quest::events::PendingQuestEvents;
use crate::quest::journal::QuestJournalRegistry;
use crate::quest::python::{QuestApiContext, QuestApiOutbox};
use crate::scripting_api::build::WorldSnapshotParams;
use crate::scripting_api::{install_ctx, ApiContext};

/// One queued request to invoke a quest hook. Yarn observers push these; the
/// draining system is the only place Python actually runs.
pub enum QuestCommandRequest {
    Start {
        player_id: u64,
        quest_id: String,
    },
    Dispatch {
        player_id: u64,
        quest_id: String,
        name: String,
        args: Vec<String>,
    },
}

#[derive(Resource, Default)]
pub struct PendingQuestCommands {
    pub entries: Vec<QuestCommandRequest>,
}

pub fn drain_quest_commands(
    mut engine: NonSendMut<QuestEngine>,
    mut pending_quests: ResMut<PendingQuestCommands>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut var_stores: ResMut<CharacterVarStores>,
    journal_registry: Res<QuestJournalRegistry>,
    players: Query<(&PlayerIdentity, &CharacterStash), With<Player>>,
    snapshot_params: WorldSnapshotParams,
) {
    if pending_quests.entries.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut pending_quests.entries);

    for request in requests {
        let (player_id, quest_id) = match &request {
            QuestCommandRequest::Start {
                player_id,
                quest_id,
            }
            | QuestCommandRequest::Dispatch {
                player_id,
                quest_id,
                ..
            } => (*player_id, quest_id.clone()),
        };

        let var_store = var_stores.get_or_insert(player_id);
        let snapshot = snapshot_params.build_for_player(Some(PlayerId(player_id)));
        let context = Arc::new(QuestApiContext::new(snapshot, player_id, Some(var_store)));
        let trait_ctx: Arc<dyn ApiContext> = context.clone();

        let started = install_ctx(trait_ctx, || match &request {
            QuestCommandRequest::Start { .. } => engine.start_quest(player_id, &quest_id),
            QuestCommandRequest::Dispatch { name, args, .. } => {
                engine.dispatch_command(player_id, &quest_id, name, args.clone());
                false
            }
        });

        // Auto-journal: a freshly-started script quest gets a stub entry so it
        // is always visible in the Log panel. Pushed before the outbox so a
        // `world.log_write` in `on_start` overwrites the stub. Declarative
        // journal files own their quest id — skip those.
        if started && !journal_registry.has_quest(&quest_id) {
            pending_commands.push_for_player(
                PlayerId(player_id),
                GameCommand::UpsertLogEntry {
                    section: QUESTS_SECTION.to_owned(),
                    subsection: quest_id.clone(),
                    title: engine.display_title(&quest_id),
                    body: "Accepted.".to_owned(),
                    owner: LogOwner::Engine,
                },
            );
        }

        apply_outbox(
            &mut engine,
            player_id,
            context.take_outbox(),
            &mut pending_commands,
            &journal_registry,
            &players,
        );
    }
}

pub fn drain_quest_events(
    mut engine: NonSendMut<QuestEngine>,
    mut pending_events: ResMut<PendingQuestEvents>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut var_stores: ResMut<CharacterVarStores>,
    journal_registry: Res<QuestJournalRegistry>,
    players: Query<(&PlayerIdentity, &CharacterStash), With<Player>>,
    snapshot_params: WorldSnapshotParams,
) {
    if pending_events.events.is_empty() {
        return;
    }
    let events = std::mem::take(&mut pending_events.events);

    for event in events {
        // Skip entirely if no quest subscribes to this kind — the firehose
        // short-circuit. Saves us building snapshots, var stores, contexts
        // for events nobody watches.
        if !engine.subs_by_kind.contains_key(event.kind()) {
            continue;
        }

        let candidate_player = match &event {
            crate::quest::events::QuestEvent::ObjectKilled {
                killer_player_id, ..
            } => *killer_player_id,
        };
        let Some(player_id) = candidate_player else {
            // Event didn't carry a player hint — skip for now. Real per-player
            // iteration lives inside engine.dispatch_event_for_player; we just
            // can't supply a shared context for all of them in one pass.
            // Revisit when we add quests that care about unattributed events.
            continue;
        };

        let var_store = var_stores.get_or_insert(player_id);
        let snapshot = snapshot_params.build_for_player(Some(PlayerId(player_id)));
        let context = Arc::new(QuestApiContext::new(snapshot, player_id, Some(var_store)));
        let trait_ctx: Arc<dyn ApiContext> = context.clone();

        install_ctx(trait_ctx, || {
            engine.dispatch_event_for_player(&event, player_id);
        });

        apply_outbox(
            &mut engine,
            player_id,
            context.take_outbox(),
            &mut pending_commands,
            &journal_registry,
            &players,
        );
    }
}

/// Observer: translates `<<start_quest>>` / `<<complete_quest>>` /
/// `<<quest_command>>` into `PendingQuestCommands` entries. Runs in the Yarn
/// dispatch observer chain; the heavy Python invocation happens later in
/// `drain_quest_commands` where we have mutable access to everything.
pub fn handle_yarn_quest_commands(
    event: On<ExecuteCommand>,
    sessions: Query<&DialogSession>,
    mut pending_quests: ResMut<PendingQuestCommands>,
) {
    let name = event.command.name.as_str();
    if !matches!(name, "start_quest" | "complete_quest" | "quest_command") {
        return;
    }
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    let params = &event.command.parameters;
    let str_args: Vec<String> = params
        .iter()
        .map(|param| match param {
            YarnValue::String(s) => s.clone(),
            YarnValue::Number(n) => n.to_string(),
            YarnValue::Boolean(b) => b.to_string(),
        })
        .collect();

    let request = match name {
        "start_quest" => {
            let Some(quest_id) = str_args.first().cloned() else {
                warn!("<<start_quest>> requires a quest id");
                return;
            };
            QuestCommandRequest::Start {
                player_id: session.player_id,
                quest_id,
            }
        }
        "complete_quest" => {
            let Some(quest_id) = str_args.first().cloned() else {
                warn!("<<complete_quest>> requires a quest id");
                return;
            };
            QuestCommandRequest::Dispatch {
                player_id: session.player_id,
                quest_id,
                name: "complete".to_owned(),
                args: Vec::new(),
            }
        }
        "quest_command" => {
            if str_args.len() < 2 {
                warn!("<<quest_command>> requires (quest_id, command_name, ...)");
                return;
            }
            let mut iter = str_args.into_iter();
            let quest_id = iter.next().unwrap();
            let command_name = iter.next().unwrap();
            let rest: Vec<String> = iter.collect();
            QuestCommandRequest::Dispatch {
                player_id: session.player_id,
                quest_id,
                name: command_name,
                args: rest,
            }
        }
        _ => unreachable!(),
    };

    pending_quests.entries.push(request);
}

/// Restore each freshly-spawned player's quest `state` dicts from their
/// `CharacterStash`. Runs after `spawn_player_from_dump` so the stash
/// component is already present.
pub fn restore_quest_state_on_player_added(
    mut engine: NonSendMut<QuestEngine>,
    players: Query<(&PlayerIdentity, &CharacterStash), Added<Player>>,
) {
    for (identity, stash) in &players {
        engine.restore_for_player(identity.id.0, &stash.entries);
    }
}

/// Copy each player's quest snapshots into their `CharacterStash` so the
/// per-character autosave path writes them out. Runs in the `Last`
/// schedule, before `persist_disconnected_players` and
/// `autosave_all_players` in `src/accounts/autosave.rs`.
pub fn mirror_quest_state_to_stash(
    engine: Option<NonSendMut<QuestEngine>>,
    mut players: Query<(&PlayerIdentity, &mut CharacterStash), With<Player>>,
) {
    let Some(engine) = engine else {
        return;
    };
    for (identity, mut stash) in &mut players {
        engine.flush_to_stash(identity.id.0, &mut stash.entries);
    }
}

fn apply_outbox(
    engine: &mut QuestEngine,
    player_id: u64,
    outbox: QuestApiOutbox,
    pending_commands: &mut PendingGameCommands,
    journal_registry: &QuestJournalRegistry,
    players: &Query<(&PlayerIdentity, &CharacterStash), With<Player>>,
) {
    for line in outbox.log_lines {
        info!("quest[{player_id}]: {line}");
    }
    for command in outbox.commands {
        pending_commands.push_for_player(PlayerId(player_id), command);
    }
    for (quest_ids, verdict) in [
        (&outbox.quest_complete, "Completed."),
        (&outbox.quest_fail, "Failed."),
    ] {
        for quest_id in quest_ids {
            if !journal_registry.has_quest(quest_id) {
                push_quest_end_entry(
                    engine,
                    player_id,
                    quest_id,
                    verdict,
                    pending_commands,
                    players,
                );
            }
            engine.end_quest(player_id, quest_id);
        }
    }
}

/// Auto-journal completion marker for a script quest without a declarative
/// journal file: append the verdict to the entry's existing body (or start a
/// fresh entry if none exists — e.g. a quest completed the same frame a
/// script `log_write` is still queued).
fn push_quest_end_entry(
    engine: &QuestEngine,
    player_id: u64,
    quest_id: &str,
    verdict: &str,
    pending_commands: &mut PendingGameCommands,
    players: &Query<(&PlayerIdentity, &CharacterStash), With<Player>>,
) {
    let existing_body = players
        .iter()
        .find(|(identity, _)| identity.id.0 == player_id)
        .map(|(_, stash)| LogState::from_stash(stash))
        .and_then(|log| log.entry(QUESTS_SECTION, quest_id).map(|e| e.body.clone()));
    let body = match existing_body {
        Some(body) if !body.is_empty() => format!("{body}\n\n— {verdict}"),
        _ => verdict.to_owned(),
    };
    pending_commands.push_for_player(
        PlayerId(player_id),
        GameCommand::UpsertLogEntry {
            section: QUESTS_SECTION.to_owned(),
            subsection: quest_id.to_owned(),
            title: engine.display_title(quest_id),
            body,
            owner: LogOwner::Engine,
        },
    );
}
