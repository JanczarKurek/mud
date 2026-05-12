//! Server-side dialog systems: translate `TalkToNpc` / `DialogAdvance` /
//! `DialogChoose` / `DialogEnd` commands into Yarn `DialogueRunner` calls, and
//! translate Yarn presentation events into `GameUiEvent`s for clients.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_yarnspinner::events::{DialogueCompleted, ExecuteCommand, PresentLine, PresentOptions};
use bevy_yarnspinner::prelude::*;

use crate::crafting::CharacterStash;
use crate::dialog::components::{DialogNode, DialogSession};
use crate::dialog::resources::{
    CharacterVarStores, DialogSessionHandle, DialogSessionRegistry, PendingDialogOptions,
    PlayerInventorySnapshots, PlayerStashSnapshots,
};
use crate::dialog::yarn_bindings;
use crate::game::commands::GameCommand;
use crate::game::resources::{
    GameUiEvent, InventoryState, PendingGameCommands, PendingGameUiEvents,
};
use crate::player::components::{Player, PlayerId as PlayerIdType, PlayerIdentity};
use crate::world::components::OverworldObject;
use crate::world::object_registry::ObjectRegistry;

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
    mut var_stores: ResMut<CharacterVarStores>,
    inventory_snapshots: Res<PlayerInventorySnapshots>,
    stash_snapshots: Res<PlayerStashSnapshots>,
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

                let storage = var_stores.get_or_insert(acting_player_id.0);
                let mut runner = project
                    .build_dialogue_runner(&mut commands)
                    .with_variable_storage(Box::new(storage))
                    .build();
                yarn_bindings::install(
                    &mut runner,
                    &mut commands,
                    &inventory_snapshots,
                    &stash_snapshots,
                    acting_player_id.0,
                );
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
                    bevy::log::warn!(
                        "DialogAdvance: no session {session_id} in registry (have {:?})",
                        sessions.by_id.keys().copied().collect::<Vec<_>>()
                    );
                    continue;
                };
                if handle.player_id != acting_player_id.0 {
                    bevy::log::warn!(
                        "DialogAdvance: session {session_id} player_id={} != acting={}",
                        handle.player_id,
                        acting_player_id.0
                    );
                    continue;
                }
                match runners.get_mut(handle.runner_entity) {
                    Ok((mut runner, _)) => {
                        runner.continue_in_next_update();
                        bevy::log::info!(
                            "DialogAdvance: continued runner for session {session_id}"
                        );
                    }
                    Err(e) => {
                        bevy::log::warn!(
                            "DialogAdvance: runner entity for session {session_id} not found: {e:?}"
                        );
                    }
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
                    bevy::log::warn!(
                        "DialogEnd: no session {session_id} in registry (have {:?})",
                        sessions.by_id.keys().copied().collect::<Vec<_>>()
                    );
                    continue;
                };
                if handle.player_id != acting_player_id.0 {
                    bevy::log::warn!(
                        "DialogEnd: session {session_id} player_id={} != acting={}",
                        handle.player_id,
                        acting_player_id.0
                    );
                    continue;
                }
                match runners.get_mut(handle.runner_entity) {
                    Ok((mut runner, _)) => {
                        runner.stop();
                        bevy::log::info!("DialogEnd: stopped runner for session {session_id}");
                    }
                    Err(e) => {
                        bevy::log::warn!(
                            "DialogEnd: runner entity for session {session_id} not found: {e:?}"
                        );
                    }
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

/// Refreshes the shared `PlayerInventorySnapshots` each frame. Yarn `has_item`
/// closures are pure (no ECS world access), so they read from this snapshot;
/// keeping it one-frame-stale is fine — dialogs advance slower than the Bevy
/// loop.
pub fn refresh_inventory_snapshots(
    snapshots: Res<PlayerInventorySnapshots>,
    registry: Res<ObjectRegistry>,
    players: Query<(&PlayerIdentity, &InventoryState), With<Player>>,
) {
    let mut snapshot_write = snapshots
        .by_player
        .write()
        .expect("snapshot RwLock poisoned");
    snapshot_write.clear();
    for (identity, inventory) in &players {
        let mut totals: HashMap<String, u32> = HashMap::new();
        for stack in inventory.backpack_slots.iter().flatten() {
            *totals.entry(stack.type_id.clone()).or_default() += stack.quantity;
        }
        snapshot_write.insert(identity.id.0, totals);
    }
    let _ = registry;
}

/// Refreshes `PlayerStashSnapshots` so Yarn `stash_*` library functions can
/// read each player's current stash without holding ECS handles. Runs in
/// `PreUpdate` (like the inventory refresh) so a `<<stash_set>>` directive
/// earlier this frame is visible in the *next* frame's reads — but
/// `stash_has` queries inside the same dialog branch will see last frame's
/// state, mirroring how `has_item` behaves after `<<give_item>>`.
pub fn refresh_stash_snapshots(
    snapshots: Res<PlayerStashSnapshots>,
    players: Query<(&PlayerIdentity, &CharacterStash), With<Player>>,
) {
    let mut snapshot_write = snapshots
        .by_player
        .write()
        .expect("stash snapshot RwLock poisoned");
    snapshot_write.clear();
    for (identity, stash) in &players {
        snapshot_write.insert(identity.id.0, stash.entries.clone());
    }
}

/// Observer: translates Yarn `<<give_recipe "id">>` into a
/// `GameCommand::LearnRecipe` queued for the acting player. Mirrors
/// `handle_yarn_item_commands` (single-string-arg form). The recipe
/// `id` must match a file in `assets/recipes/` (no `.yaml`).
pub fn handle_yarn_recipe_commands(
    event: On<ExecuteCommand>,
    sessions: Query<&DialogSession>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    if event.command.name != "give_recipe" {
        return;
    }
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    let Some(YarnValue::String(recipe_id)) = event.command.parameters.first().cloned() else {
        bevy::log::warn!("yarn <<give_recipe>>: first arg must be a string");
        return;
    };
    pending_commands.push_for_player(
        PlayerIdType(session.player_id),
        GameCommand::LearnRecipe { recipe_id },
    );
}

/// Observer: translates Yarn `<<stash_set "key" value>>` into a
/// `GameCommand::StashMutate` queued for the acting player. Accepts:
///   * `<<stash_set "key" "string">>` — string value
///   * `<<stash_set "key" 7>>` — numeric value (Yarn often passes numbers
///     as `String` tokens unless wrapped in `{...}` — both are accepted)
///   * `<<stash_set "key" true>>` / `false` — boolean value
///   * `<<stash_set "key">>` — delete the key (one-arg form)
pub fn handle_yarn_stash_commands(
    event: On<ExecuteCommand>,
    sessions: Query<&DialogSession>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    let name = event.command.name.as_str();
    if name != "stash_set" {
        return;
    }
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    let params = &event.command.parameters;
    let key = match params.first() {
        Some(YarnValue::String(s)) => s.clone(),
        Some(other) => {
            bevy::log::warn!("yarn <<stash_set>>: first arg must be a string, got {other:?}");
            return;
        }
        None => {
            bevy::log::warn!("yarn <<stash_set>> requires at least a key argument");
            return;
        }
    };
    let value: Option<serde_json::Value> = match params.get(1) {
        None => None,
        Some(YarnValue::String(s)) => {
            // Accept Number-as-String — Yarn ships numeric literals as
            // String tokens unless `{...}`-wrapped. Parse-fall-through so
            // `<<stash_set "k" "hello">>` still stores `"hello"`.
            if let Ok(n) = s.parse::<i64>() {
                Some(serde_json::Value::from(n))
            } else if let Ok(f) = s.parse::<f64>() {
                Some(
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null),
                )
            } else if let Ok(b) = s.parse::<bool>() {
                Some(serde_json::Value::Bool(b))
            } else {
                Some(serde_json::Value::String(s.clone()))
            }
        }
        Some(YarnValue::Number(n)) => Some(
            serde_json::Number::from_f64(*n as f64)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
        ),
        Some(YarnValue::Boolean(b)) => Some(serde_json::Value::Bool(*b)),
    };
    pending_commands.push_for_player(
        PlayerIdType(session.player_id),
        GameCommand::StashMutate { key, value },
    );
}

/// Observer: translates Yarn `<<give_item>>` / `<<take_item>>` into authoritative
/// `GameCommand`s. Registering this as an `ExecuteCommand` observer (rather than
/// per-runner `add_command`) keeps the source runner entity available so we can
/// resolve the acting player via its `DialogSession` component — system-backed
/// commands only see `In<T>`, not the source entity.
pub fn handle_yarn_item_commands(
    event: On<ExecuteCommand>,
    sessions: Query<&DialogSession>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    let name = event.command.name.as_str();
    if name != "give_item" && name != "take_item" {
        return;
    }
    let Ok(session) = sessions.get(event.entity) else {
        return;
    };
    // Yarn's command grammar passes argument tokens as-is — `<<give_item "apple" 3>>`
    // arrives as [String("apple"), String("3")] unless the author wraps numbers
    // in `{...}` to force expression evaluation. Accept both so dialog writers
    // don't have to know that detail.
    let params = &event.command.parameters;
    let (type_id, count) = match params.as_slice() {
        [type_val, count_val] => {
            let type_id = match type_val {
                YarnValue::String(s) => s.clone(),
                other => {
                    bevy::log::warn!("yarn <<{name}>>: first arg must be a string, got {other:?}");
                    return;
                }
            };
            let count: u32 = match count_val {
                YarnValue::Number(n) => n.max(0.0) as u32,
                YarnValue::String(s) => match s.parse::<u32>() {
                    Ok(c) => c,
                    Err(err) => {
                        bevy::log::warn!("yarn <<{name}>>: second arg {s:?} not a u32: {err}");
                        return;
                    }
                },
                other => {
                    bevy::log::warn!("yarn <<{name}>>: second arg must be a count, got {other:?}");
                    return;
                }
            };
            (type_id, count)
        }
        _ => {
            bevy::log::warn!(
                "yarn <<{name}>> expects 2 arguments (type_id, count) — got {} ({:?})",
                params.len(),
                params
            );
            return;
        }
    };
    let command = match name {
        "give_item" => GameCommand::GiveItem { type_id, count },
        "take_item" => GameCommand::TakeItem { type_id, count },
        _ => unreachable!(),
    };
    pending_commands.push_for_player(PlayerIdType(session.player_id), command);
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
