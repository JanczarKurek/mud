//! Glue between Bevy ECS and `QuestEngine`.
//!
//! `drain_quest_commands` runs each frame:
//!   1. Consumes `PendingQuestCommands` entries queued by the Yarn observer.
//!   2. For each, invokes the matching engine hook with a fresh
//!      `QuestApiEffects`, after installing the player's var store + inventory
//!      snapshot in the thread-local context.
//!   3. Replays the effects back into the ECS: item give/take via
//!      `GameCommand`, quest completion via `engine.end_quest`.
//!
//! `drain_quest_events` does the same for `PendingQuestEvents`, routing
//! through `engine.dispatch_event`.

use bevy::prelude::*;
use bevy_yarnspinner::events::ExecuteCommand;
use bevy_yarnspinner::prelude::YarnValue;

use crate::dialog::components::DialogSession;
use crate::dialog::resources::{CharacterVarStores, PlayerInventorySnapshots};
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::player::components::PlayerId;
use crate::quest::engine::QuestEngine;
use crate::quest::events::PendingQuestEvents;
use crate::quest::python::{with_full_call_context, QuestApiEffects};

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
    inventory_snapshots: Res<PlayerInventorySnapshots>,
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
        let inventory = inventory_snapshots
            .by_player
            .read()
            .unwrap()
            .get(&player_id)
            .cloned()
            .unwrap_or_default();

        let mut effects = QuestApiEffects::default();
        // Install the context so pymodule calls can read/write var store
        // + inventory + push to effects. The engine invokes hooks directly
        // without re-wrapping context, so all Python calls in this frame
        // see the same context.
        with_full_call_context(player_id, Some(var_store), inventory, &mut effects, || {
            match &request {
                QuestCommandRequest::Start { .. } => {
                    engine.start_quest(player_id, &quest_id);
                }
                QuestCommandRequest::Dispatch { name, args, .. } => {
                    engine.dispatch_command(player_id, &quest_id, name, args.clone());
                }
            }
        });

        apply_effects(&mut engine, player_id, effects, &mut pending_commands);
    }
}

pub fn drain_quest_events(
    mut engine: NonSendMut<QuestEngine>,
    mut pending_events: ResMut<PendingQuestEvents>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut var_stores: ResMut<CharacterVarStores>,
    inventory_snapshots: Res<PlayerInventorySnapshots>,
) {
    if pending_events.events.is_empty() {
        return;
    }
    let events = std::mem::take(&mut pending_events.events);

    for event in events {
        // Skip entirely if no quest subscribes to this kind — the firehose
        // short-circuit. Saves us building snapshots, var stores, effects
        // outboxes for events nobody watches.
        if !engine.subs_by_kind.contains_key(event.kind()) {
            continue;
        }

        // Event dispatch needs per-player context, but the subscription map
        // only tells us *which quests* care. Use event's hint fields (e.g.
        // `killer_player_id`) to pick the player context when possible;
        // otherwise we'd need to iterate all active characters (fine later).
        let candidate_player = match &event {
            crate::quest::events::QuestEvent::ObjectKilled {
                killer_player_id, ..
            } => *killer_player_id,
        };
        let Some(player_id) = candidate_player else {
            // Event didn't carry a player hint — skip for now. Real per-player
            // iteration lives inside engine.dispatch_event (which iterates its
            // own `active_states`); we just can't supply a shared inventory
            // context for all of them in one pass. Revisit when we add quests
            // that care about unattributed events.
            continue;
        };

        let var_store = var_stores.get_or_insert(player_id);
        let inventory = inventory_snapshots
            .by_player
            .read()
            .unwrap()
            .get(&player_id)
            .cloned()
            .unwrap_or_default();

        let mut effects = QuestApiEffects::default();
        with_full_call_context(player_id, Some(var_store), inventory, &mut effects, || {
            engine.dispatch_event_for_player(&event, player_id);
        });

        apply_effects(&mut engine, player_id, effects, &mut pending_commands);
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

fn apply_effects(
    engine: &mut QuestEngine,
    player_id: u64,
    effects: QuestApiEffects,
    pending_commands: &mut PendingGameCommands,
) {
    for line in effects.log_lines {
        info!("quest[{player_id}]: {line}");
    }
    for (type_id, count) in effects.give {
        pending_commands.push_for_player(
            PlayerId(player_id),
            GameCommand::GiveItem { type_id, count },
        );
    }
    for (type_id, count) in effects.take {
        pending_commands.push_for_player(
            PlayerId(player_id),
            GameCommand::TakeItem { type_id, count },
        );
    }
    for quest_id in effects.quest_complete {
        engine.end_quest(player_id, &quest_id);
    }
    for quest_id in effects.quest_fail {
        engine.end_quest(player_id, &quest_id);
    }
}
