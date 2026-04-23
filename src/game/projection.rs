//! Server ECS → client view-state projection.
//!
//! Three pieces live here:
//! - [`compute_events_for_peer`] diffs the authoritative ECS against a supplied
//!   baseline and returns a `Vec<GameEvent>`. This is the single serializer
//!   for both embedded and networked clients — per-peer on the server, against
//!   the local `ClientGameState` in embedded mode.
//! - [`collect_game_events_from_authority`] is the embedded/server-local system
//!   wrapper that feeds `compute_events_for_peer` with the current
//!   `ClientGameState` resource as the baseline and writes the result into
//!   `PendingGameEvents`.
//! - [`apply_game_events_to_client_state`] folds pending events back into
//!   `ClientGameState`, keeping presentation in lock-step with authority.
//!
//! See the "EmbeddedClient Invariant" in `CLAUDE.md`: these functions are the
//! single fold through which all server → client state flows, both in
//! networked and embedded modes.

use bevy::ecs::query::QuerySingleError;
use bevy::log::{debug, info};
use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::dialog::components::DialogNode;
use crate::game::resources::{
    ChatLogState, ClientGameState, ClientRemotePlayerState, ClientSpaceState, ClientVitalStats,
    ClientWorldObjectState, GameEvent, InventoryState, PendingGameEvents,
};
use crate::npc::components::Npc;
use crate::player::components::{DerivedStats, Player, PlayerId, PlayerIdentity, VitalStats};
use crate::world::components::{
    Container, Facing, Movable, OverworldObject, Quantity, SpaceId, SpacePosition, SpaceResident,
    TilePosition,
};
use crate::world::resources::SpaceManager;

pub type ProjectionPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerIdentity,
        &'static InventoryState,
        &'static ChatLogState,
        &'static SpaceResident,
        &'static TilePosition,
        &'static VitalStats,
        &'static DerivedStats,
        Option<&'static CombatTarget>,
        &'static OverworldObject,
        Option<&'static Facing>,
    ),
    With<Player>,
>;

pub type ProjectionObjectQuery<'w, 's> = Query<'w, 's, &'static OverworldObject>;

pub type ProjectionWorldObjectQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static SpaceResident,
        &'static TilePosition,
        &'static OverworldObject,
        Option<&'static VitalStats>,
        Has<Container>,
        Has<Npc>,
        Has<Movable>,
        Option<&'static Quantity>,
        Has<DialogNode>,
        Option<&'static Facing>,
    ),
    Without<Player>,
>;

pub type ProjectionContainerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Container,
        &'static OverworldObject,
        &'static SpaceResident,
    ),
    Without<Player>,
>;

/// Diffs the authoritative ECS against a per-peer baseline, returning a
/// `Vec<GameEvent>` that, when folded into `previous`, produces the peer's
/// next `ClientGameState`. Passing `&ClientGameState::default()` as `previous`
/// yields a full bootstrap sequence for a newly connected client.
pub fn compute_events_for_peer(
    local_player_id: PlayerId,
    previous: &ClientGameState,
    player_query: &ProjectionPlayerQuery,
    object_query: &ProjectionObjectQuery,
    world_object_query: &ProjectionWorldObjectQuery,
    container_query: &ProjectionContainerQuery,
    space_manager: &SpaceManager,
) -> Vec<GameEvent> {
    let mut events = Vec::new();

    let mut local_player_object_id: Option<u64> = None;
    let mut local_space_id: Option<SpaceId> = None;
    let mut seen_remote_player_ids: Vec<PlayerId> = Vec::new();

    for (
        identity,
        inventory,
        chat_log,
        space_resident,
        tile_position,
        vital_stats,
        derived_stats,
        combat_target,
        player_object,
        facing,
    ) in player_query.iter()
    {
        let projected_facing = facing.copied().unwrap_or_default().0;
        let projected_vitals = ClientVitalStats {
            health: vital_stats.health,
            max_health: vital_stats.max_health,
            mana: vital_stats.mana,
            max_mana: vital_stats.max_mana,
        };

        if identity.id == local_player_id {
            local_player_object_id = Some(player_object.object_id);
            local_space_id = Some(space_resident.space_id);

            if previous.local_player_id != Some(local_player_id)
                || previous.local_player_object_id != Some(player_object.object_id)
            {
                events.push(GameEvent::LocalPlayerIdentified {
                    player_id: local_player_id,
                    object_id: player_object.object_id,
                });
            }

            if previous.inventory != *inventory {
                events.push(GameEvent::InventoryChanged {
                    inventory: inventory.clone(),
                });
            }

            if previous.chat_log_lines != chat_log.lines {
                events.push(GameEvent::ChatLogChanged {
                    lines: chat_log.lines.clone(),
                });
            }

            let current_player_position =
                SpacePosition::new(space_resident.space_id, *tile_position);
            if previous.player_position != Some(current_player_position)
                || previous.player_facing != Some(projected_facing)
            {
                events.push(GameEvent::PlayerPositionChanged {
                    position: current_player_position,
                    tile_position: *tile_position,
                    facing: projected_facing,
                });
            }

            if previous.player_vitals != Some(projected_vitals) {
                events.push(GameEvent::PlayerVitalsChanged {
                    vitals: projected_vitals,
                });
            }

            if previous.player_storage_slots != derived_stats.storage_slots {
                events.push(GameEvent::PlayerStorageChanged {
                    storage_slots: derived_stats.storage_slots,
                });
            }

            let current_target_object_id = combat_target
                .and_then(|combat_target| object_query.get(combat_target.entity).ok())
                .map(|object| object.object_id);
            if previous.current_target_object_id != current_target_object_id {
                events.push(GameEvent::CombatTargetChanged {
                    target_object_id: current_target_object_id,
                });
            }
        } else {
            seen_remote_player_ids.push(identity.id);
            let position = SpacePosition::new(space_resident.space_id, *tile_position);
            let projected = ClientRemotePlayerState {
                player_id: identity.id,
                object_id: player_object.object_id,
                position,
                tile_position: *tile_position,
                vitals: projected_vitals,
                facing: projected_facing,
            };
            if previous.remote_players.get(&identity.id) != Some(&projected) {
                events.push(GameEvent::RemotePlayerUpserted { player: projected });
            }
        }
    }

    for previous_id in previous.remote_players.keys() {
        if !seen_remote_player_ids.contains(previous_id) {
            events.push(GameEvent::RemotePlayerRemoved {
                player_id: *previous_id,
            });
        }
    }

    let Some(local_space_id) = local_space_id else {
        return events;
    };
    let _ = local_player_object_id;

    if let Some(runtime_space) = space_manager.get(local_space_id) {
        let current_space = ClientSpaceState {
            space_id: runtime_space.id,
            authored_id: runtime_space.authored_id.clone(),
            width: runtime_space.width,
            height: runtime_space.height,
            fill_object_type: runtime_space.fill_object_type.clone(),
        };
        if previous.current_space.as_ref() != Some(&current_space) {
            events.push(GameEvent::CurrentSpaceChanged {
                space: current_space,
            });
        }
    }

    let mut current_container_ids = Vec::new();
    for (container, object, resident) in container_query.iter() {
        if resident.space_id != local_space_id {
            continue;
        }
        current_container_ids.push(object.object_id);
        let current_slots = &container.slots;
        if previous.container_slots.get(&object.object_id) != Some(current_slots) {
            events.push(GameEvent::ContainerChanged {
                object_id: object.object_id,
                slots: current_slots.clone(),
            });
        }
    }

    for stale_object_id in previous.container_slots.keys() {
        if !current_container_ids.contains(stale_object_id) {
            events.push(GameEvent::ContainerRemoved {
                object_id: *stale_object_id,
            });
        }
    }

    let mut current_world_object_ids = Vec::new();
    for (
        space_resident,
        tile_position,
        object,
        vitals,
        has_container,
        has_npc,
        has_movable,
        qty,
        has_dialog,
        facing,
    ) in world_object_query.iter()
    {
        if space_resident.space_id != local_space_id {
            continue;
        }
        current_world_object_ids.push(object.object_id);
        let projected_object = ClientWorldObjectState {
            object_id: object.object_id,
            definition_id: object.definition_id.clone(),
            position: SpacePosition::new(space_resident.space_id, *tile_position),
            tile_position: *tile_position,
            vitals: vitals.map(|vitals| ClientVitalStats {
                health: vitals.health,
                max_health: vitals.max_health,
                mana: vitals.mana,
                max_mana: vitals.max_mana,
            }),
            is_container: has_container,
            is_npc: has_npc,
            is_movable: has_movable,
            quantity: qty.map(|q| q.0).unwrap_or(1),
            has_dialog,
            facing: facing.copied().unwrap_or_default().0,
        };

        if previous.world_objects.get(&object.object_id) != Some(&projected_object) {
            events.push(GameEvent::WorldObjectUpserted {
                object: projected_object,
            });
        }
    }

    for stale_object_id in previous.world_objects.keys() {
        if !current_world_object_ids.contains(stale_object_id) {
            events.push(GameEvent::WorldObjectRemoved {
                object_id: *stale_object_id,
            });
        }
    }

    events
}

/// Embedded-mode wrapper: picks the local player's id from the single
/// authoritative player entity and calls [`compute_events_for_peer`] with the
/// current `ClientGameState` as baseline. Writes the result into
/// `PendingGameEvents` for `apply_game_events_to_client_state` to fold.
pub fn collect_game_events_from_authority(
    client_state: Res<ClientGameState>,
    space_manager: Res<SpaceManager>,
    player_query: ProjectionPlayerQuery,
    object_query: ProjectionObjectQuery,
    world_object_query: ProjectionWorldObjectQuery,
    container_query: ProjectionContainerQuery,
    mut pending_game_events: ResMut<PendingGameEvents>,
) {
    pending_game_events.events.clear();

    let local_player_id = match player_query.single() {
        Ok((identity, ..)) => identity.id,
        Err(QuerySingleError::NoEntities(_)) => {
            bevy::log::warn!("collect_game_events: no Player entity found");
            return;
        }
        Err(QuerySingleError::MultipleEntities(_)) => {
            let count = player_query.iter().count();
            bevy::log::warn!(
                "collect_game_events: {} Player entities found (expected 1 for embedded mode)",
                count
            );
            return;
        }
    };

    let events = compute_events_for_peer(
        local_player_id,
        &client_state,
        &player_query,
        &object_query,
        &world_object_query,
        &container_query,
        &space_manager,
    );

    pending_game_events.events.extend(events);
}

pub fn apply_game_events_to_client_state(
    mut client_state: ResMut<ClientGameState>,
    mut pending_game_events: ResMut<PendingGameEvents>,
) {
    let events = std::mem::take(&mut pending_game_events.events);
    for event in events {
        log_client_game_event(&client_state, &event);
        apply_event_to_state(&mut client_state, event);
    }
}

/// Folds a single `GameEvent` into a `ClientGameState` — used both by
/// `apply_game_events_to_client_state` (on the client) and the per-peer
/// baseline-advance on the server.
pub fn apply_event_to_state(state: &mut ClientGameState, event: GameEvent) {
    match event {
        GameEvent::LocalPlayerIdentified {
            player_id,
            object_id,
        } => {
            state.local_player_id = Some(player_id);
            state.local_player_object_id = Some(object_id);
        }
        GameEvent::InventoryChanged { inventory } => {
            state.inventory = inventory;
        }
        GameEvent::ChatLogChanged { lines } => {
            state.chat_log_lines = lines;
        }
        GameEvent::PlayerPositionChanged {
            position,
            tile_position,
            facing,
        } => {
            state.player_position = Some(position);
            state.player_tile_position = Some(tile_position);
            state.player_facing = Some(facing);
        }
        GameEvent::CurrentSpaceChanged { space } => {
            state.current_space = Some(space);
        }
        GameEvent::PlayerVitalsChanged { vitals } => {
            state.player_vitals = Some(vitals);
        }
        GameEvent::PlayerStorageChanged { storage_slots } => {
            state.player_storage_slots = storage_slots;
        }
        GameEvent::CombatTargetChanged { target_object_id } => {
            state.current_target_object_id = target_object_id;
        }
        GameEvent::ContainerChanged { object_id, slots } => {
            state.container_slots.insert(object_id, slots);
        }
        GameEvent::ContainerRemoved { object_id } => {
            state.container_slots.remove(&object_id);
        }
        GameEvent::WorldObjectUpserted { object } => {
            state.world_objects.insert(object.object_id, object);
        }
        GameEvent::WorldObjectRemoved { object_id } => {
            state.world_objects.remove(&object_id);
        }
        GameEvent::RemotePlayerUpserted { player } => {
            state.remote_players.insert(player.player_id, player);
        }
        GameEvent::RemotePlayerRemoved { player_id } => {
            state.remote_players.remove(&player_id);
        }
    }
}

fn log_client_game_event(client_state: &ClientGameState, event: &GameEvent) {
    match event {
        GameEvent::LocalPlayerIdentified {
            player_id,
            object_id,
        } => info!(
            "client local player identified: {:?} object {} (was {:?}/{:?})",
            player_id,
            object_id,
            client_state.local_player_id,
            client_state.local_player_object_id,
        ),
        GameEvent::InventoryChanged { inventory } => info!(
            "client inventory updated: {} backpack slots used, {} equipped slots occupied",
            inventory.backpack_slots.iter().flatten().count(),
            inventory
                .equipment_slots
                .iter()
                .filter_map(|(_, object_id)| *object_id)
                .count(),
        ),
        GameEvent::ChatLogChanged { lines } => {
            let previous_count = client_state.chat_log_lines.len();
            let new_count = lines.len();
            if new_count > previous_count {
                if let Some(last_line) = lines.last() {
                    info!("client chat log appended: {last_line}");
                }
            } else {
                debug!(
                    "client chat log replaced: {} -> {} lines",
                    previous_count, new_count
                );
            }
        }
        GameEvent::PlayerPositionChanged { position, .. } => info!(
            "client player position updated: {:?} -> space {} at ({}, {})",
            client_state.player_position,
            position.space_id.0,
            position.tile_position.x,
            position.tile_position.y
        ),
        GameEvent::CurrentSpaceChanged { space } => info!(
            "client current space updated: {:?} -> {} ({})",
            client_state
                .current_space
                .as_ref()
                .map(|current| current.space_id.0),
            space.space_id.0,
            space.authored_id
        ),
        GameEvent::PlayerVitalsChanged { vitals } => info!(
            "client player vitals updated: hp {:.1}/{:.1} -> {:.1}/{:.1}, mana {:.1}/{:.1} -> {:.1}/{:.1}",
            client_state.player_vitals.map(|current| current.health).unwrap_or_default(),
            client_state.player_vitals.map(|current| current.max_health).unwrap_or_default(),
            vitals.health,
            vitals.max_health,
            client_state.player_vitals.map(|current| current.mana).unwrap_or_default(),
            client_state.player_vitals.map(|current| current.max_mana).unwrap_or_default(),
            vitals.mana,
            vitals.max_mana
        ),
        GameEvent::PlayerStorageChanged { storage_slots } => info!(
            "client player storage updated: {} -> {}",
            client_state.player_storage_slots, storage_slots
        ),
        GameEvent::CombatTargetChanged { target_object_id } => info!(
            "client combat target updated: {:?} -> {:?}",
            client_state.current_target_object_id, target_object_id
        ),
        GameEvent::ContainerChanged { object_id, slots } => debug!(
            "client container {} updated: {} slots",
            object_id,
            slots.len()
        ),
        GameEvent::ContainerRemoved { object_id } => {
            debug!("client container {} removed from projection", object_id)
        }
        GameEvent::WorldObjectUpserted { object } => debug!(
            "client projected object upserted: {} ({}) at ({}, {})",
            object.object_id, object.definition_id, object.tile_position.x, object.tile_position.y
        ),
        GameEvent::WorldObjectRemoved { object_id } => {
            debug!("client projected object removed: {}", object_id)
        }
        GameEvent::RemotePlayerUpserted { player } => debug!(
            "client remote player upserted: {} object {} at ({}, {})",
            player.player_id.0, player.object_id, player.tile_position.x, player.tile_position.y
        ),
        GameEvent::RemotePlayerRemoved { player_id } => {
            debug!("client remote player removed: {}", player_id.0)
        }
    }
}
