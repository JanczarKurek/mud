use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};

use bevy::log::{error, info, warn};
use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::game::resources::{
    ClientGameState, ClientRemotePlayerState, ClientSpaceState, ClientVitalStats,
    ClientWorldObjectState, PendingGameCommands, PendingGameUiEvents,
};
use crate::network::protocol::{ClientMessage, ServerMessage};
use crate::network::resources::{
    ConnectionId, TcpClientConfig, TcpClientConnection, TcpServerConfig, TcpServerPeer,
    TcpServerState,
};
use crate::npc::components::Npc;
use crate::player::components::{
    ChatLog, DerivedStats, Inventory, Player, PlayerId, PlayerIdentity, VitalStats,
};
use crate::player::setup::spawn_player_authoritative_in_space;
use crate::world::components::{
    Collider, Container, Movable, OverworldObject, Quantity, SpacePosition, SpaceResident,
    TilePosition,
};
use crate::world::map_layout::SpaceDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::WorldConfig;

pub fn start_tcp_server(config: Res<TcpServerConfig>, mut server_state: ResMut<TcpServerState>) {
    if server_state.listener.is_some() {
        return;
    }

    let Ok(listener) = TcpListener::bind(&config.bind_addr) else {
        error!("failed to bind TCP server on {}", config.bind_addr);
        return;
    };
    if let Err(error) = listener.set_nonblocking(true) {
        error!("failed to set TCP listener nonblocking: {error}");
        return;
    }

    info!("TCP server listening on {}", config.bind_addr);
    server_state.listener = Some(listener);
}

pub fn accept_tcp_client_connections(
    mut server_state: ResMut<TcpServerState>,
    world_config: Res<WorldConfig>,
    authored_spaces: Res<SpaceDefinitions>,
    space_manager: Res<SpaceManager>,
    collider_query: Query<(&SpaceResident, &TilePosition), With<Collider>>,
    player_position_query: Query<(&SpaceResident, &TilePosition), With<Player>>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut commands: Commands,
) {
    let Some(listener) = server_state
        .listener
        .as_ref()
        .and_then(|listener| listener.try_clone().ok())
    else {
        return;
    };

    loop {
        match listener.accept() {
            Ok((stream, address)) => {
                if let Err(error) = stream.set_nonblocking(true) {
                    warn!("failed to set accepted stream nonblocking: {error}");
                    continue;
                }

                let Some((spawn_space_id, spawn_tile)) = find_spawn_location(
                    &world_config,
                    &authored_spaces,
                    &space_manager,
                    &collider_query,
                    &player_position_query,
                ) else {
                    warn!("rejecting TCP client from {address}: no free spawn tile");
                    continue;
                };

                let connection_id = ConnectionId(server_state.next_connection_id);
                server_state.next_connection_id += 1;
                let player_id = PlayerId(connection_id.0);
                let object_id = object_registry.allocate_runtime_id("player");
                let player_entity = spawn_player_authoritative_in_space(
                    &mut commands,
                    player_id,
                    object_id,
                    spawn_space_id,
                    spawn_tile,
                );

                info!("TCP client connected from {address}");
                server_state.peers.insert(
                    connection_id,
                    TcpServerPeer {
                        connection_id,
                        player_id,
                        player_entity,
                        stream,
                        read_buffer: Vec::new(),
                        last_snapshot: None,
                    },
                );
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => {
                warn!("TCP accept failed: {error}");
                break;
            }
        }
    }
}

pub fn poll_tcp_server_messages(
    mut server_state: ResMut<TcpServerState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut commands: Commands,
) {
    let connection_ids = server_state.peers.keys().copied().collect::<Vec<_>>();
    let mut disconnected_peers = Vec::new();

    for connection_id in connection_ids {
        let Some(peer) = server_state.peers.get_mut(&connection_id) else {
            continue;
        };

        let mut disconnected = false;
        while let Some(line) =
            read_next_line(&mut peer.stream, &mut peer.read_buffer, &mut disconnected)
        {
            match serde_json::from_str::<ClientMessage>(&line) {
                Ok(ClientMessage::Command(command)) => {
                    pending_commands.push_for_player(peer.player_id, command);
                }
                Err(error) => warn!("failed to parse client message: {error}"),
            }
        }

        if disconnected {
            disconnected_peers.push(connection_id);
        }
    }

    for connection_id in disconnected_peers {
        disconnect_peer(&mut server_state, connection_id, &mut commands);
    }
}

pub fn flush_server_messages(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    mut server_state: ResMut<TcpServerState>,
    player_query: Query<
        (
            Entity,
            &PlayerIdentity,
            &Inventory,
            &ChatLog,
            &SpaceResident,
            &TilePosition,
            &VitalStats,
            &DerivedStats,
            Option<&CombatTarget>,
            &OverworldObject,
        ),
        With<Player>,
    >,
    object_query: Query<&OverworldObject>,
    world_object_query: Query<
        (
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            Option<&VitalStats>,
            Has<Container>,
            Has<Npc>,
            Has<Movable>,
            Option<&Quantity>,
        ),
        Without<Player>,
    >,
    container_query: Query<(&Container, &OverworldObject, &SpaceResident), Without<Player>>,
    space_manager: Res<SpaceManager>,
    mut commands: Commands,
) {
    let peer_ui_events = std::mem::take(&mut pending_ui_events.peer_events);
    pending_ui_events.events.clear();

    let connection_ids = server_state.peers.keys().copied().collect::<Vec<_>>();
    let mut disconnected_peers = Vec::new();

    for connection_id in connection_ids {
        let Some(peer) = server_state.peers.get_mut(&connection_id) else {
            continue;
        };

        let snapshot = build_client_game_state(
            peer.player_id,
            &player_query,
            &object_query,
            &world_object_query,
            &container_query,
            &space_manager,
        );

        let mut disconnected = false;
        if peer.last_snapshot.as_ref() != Some(&snapshot) {
            if !write_message(
                &mut peer.stream,
                &ServerMessage::Snapshot(snapshot.clone()),
                &mut disconnected,
            ) {
                warn!("failed to send snapshot to TCP client");
            } else {
                peer.last_snapshot = Some(snapshot);
            }
        }

        if let Some(events) = peer_ui_events.get(&peer.player_id) {
            if !events.is_empty()
                && !write_message(
                    &mut peer.stream,
                    &ServerMessage::UiEvents(events.clone()),
                    &mut disconnected,
                )
            {
                warn!("failed to send UI events to TCP client");
            }
        }

        if disconnected {
            disconnected_peers.push(connection_id);
        }
    }

    for connection_id in disconnected_peers {
        disconnect_peer(&mut server_state, connection_id, &mut commands);
    }
}

pub fn flush_client_commands_to_server(
    config: Res<TcpClientConfig>,
    mut connection: ResMut<TcpClientConnection>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    ensure_tcp_client_connected(&config, &mut connection);

    let Some(stream) = &mut connection.stream else {
        pending_commands.commands.clear();
        return;
    };

    let commands = std::mem::take(&mut pending_commands.commands);
    let mut disconnected = false;
    for command in commands {
        if !write_message(
            stream,
            &ClientMessage::Command(command.command),
            &mut disconnected,
        ) {
            break;
        }
    }

    if disconnected {
        connection.stream = None;
        connection.read_buffer.clear();
    }
}

pub fn poll_tcp_client_messages(
    config: Res<TcpClientConfig>,
    mut connection: ResMut<TcpClientConnection>,
    mut client_state: ResMut<ClientGameState>,
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
) {
    ensure_tcp_client_connected(&config, &mut connection);

    let mut read_buffer = std::mem::take(&mut connection.read_buffer);
    let Some(stream) = &mut connection.stream else {
        connection.read_buffer = read_buffer;
        return;
    };

    let mut disconnected = false;
    while let Some(line) = read_next_line(stream, &mut read_buffer, &mut disconnected) {
        match serde_json::from_str::<ServerMessage>(&line) {
            Ok(ServerMessage::Snapshot(snapshot)) => {
                *client_state = snapshot;
            }
            Ok(ServerMessage::UiEvents(events)) => {
                pending_ui_events.events.extend(events);
            }
            Err(error) => warn!("failed to parse server message: {error}"),
        }
    }

    if disconnected {
        warn!("lost TCP connection to {}", config.server_addr);
        connection.stream = None;
        connection.read_buffer.clear();
    } else {
        connection.read_buffer = read_buffer;
    }
}

fn ensure_tcp_client_connected(config: &TcpClientConfig, connection: &mut TcpClientConnection) {
    if !config.active {
        return;
    }

    if connection.stream.is_some() {
        return;
    }

    let Ok(stream) = TcpStream::connect(&config.server_addr) else {
        return;
    };
    if let Err(error) = stream.set_nonblocking(true) {
        warn!("failed to set TCP client stream nonblocking: {error}");
        return;
    }

    info!("connected to TCP server at {}", config.server_addr);
    connection.stream = Some(stream);
}

fn disconnect_peer(
    server_state: &mut TcpServerState,
    connection_id: ConnectionId,
    commands: &mut Commands,
) {
    if let Some(peer) = server_state.peers.remove(&connection_id) {
        info!("TCP client disconnected");
        commands.entity(peer.player_entity).despawn();
    }
}

fn find_spawn_location(
    world_config: &WorldConfig,
    authored_spaces: &SpaceDefinitions,
    space_manager: &SpaceManager,
    collider_query: &Query<(&SpaceResident, &TilePosition), With<Collider>>,
    player_position_query: &Query<(&SpaceResident, &TilePosition), With<Player>>,
) -> Option<(crate::world::components::SpaceId, TilePosition)> {
    let bootstrap_space_id = space_manager
        .persistent_space_id(&authored_spaces.bootstrap_space_id)
        .unwrap_or(world_config.current_space_id);
    let (width, height) = space_manager
        .get(bootstrap_space_id)
        .map(|space| (space.width, space.height))
        .unwrap_or((world_config.map_width, world_config.map_height));
    let origin = TilePosition::new(width / 2, height / 2);

    for radius in 0..width.max(height) {
        for y in -radius..=radius {
            for x in -radius..=radius {
                if radius > 0 && x.abs() != radius && y.abs() != radius {
                    continue;
                }

                let candidate = TilePosition::new(origin.x + x, origin.y + y);
                if candidate.x < 0
                    || candidate.y < 0
                    || candidate.x >= width
                    || candidate.y >= height
                {
                    continue;
                }

                let blocked = collider_query.iter().any(|(resident, tile)| {
                    resident.space_id == bootstrap_space_id && *tile == candidate
                }) || player_position_query.iter().any(|(resident, tile)| {
                    resident.space_id == bootstrap_space_id && *tile == candidate
                });
                if !blocked {
                    return Some((bootstrap_space_id, candidate));
                }
            }
        }
    }

    None
}

fn build_client_game_state(
    local_player_id: PlayerId,
    player_query: &Query<
        (
            Entity,
            &PlayerIdentity,
            &Inventory,
            &ChatLog,
            &SpaceResident,
            &TilePosition,
            &VitalStats,
            &DerivedStats,
            Option<&CombatTarget>,
            &OverworldObject,
        ),
        With<Player>,
    >,
    object_query: &Query<&OverworldObject>,
    world_object_query: &Query<
        (
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            Option<&VitalStats>,
            Has<Container>,
            Has<Npc>,
            Has<Movable>,
            Option<&Quantity>,
        ),
        Without<Player>,
    >,
    container_query: &Query<(&Container, &OverworldObject, &SpaceResident), Without<Player>>,
    space_manager: &SpaceManager,
) -> ClientGameState {
    let mut state = ClientGameState {
        local_player_id: Some(local_player_id),
        ..default()
    };
    let mut local_space_id = None;

    for (
        _entity,
        identity,
        inventory,
        chat_log,
        space_resident,
        tile_position,
        vital_stats,
        derived_stats,
        combat_target,
        player_object,
    ) in player_query.iter()
    {
        let projected_vitals = ClientVitalStats {
            health: vital_stats.health,
            max_health: vital_stats.max_health,
            mana: vital_stats.mana,
            max_mana: vital_stats.max_mana,
        };

        if identity.id == local_player_id {
            state.inventory = inventory.clone();
            state.chat_log_lines = chat_log.lines.clone();
            state.player_position =
                Some(SpacePosition::new(space_resident.space_id, *tile_position));
            state.player_tile_position = Some(*tile_position);
            state.player_vitals = Some(projected_vitals);
            state.player_storage_slots = derived_stats.storage_slots;
            state.local_player_object_id = Some(player_object.object_id);
            local_space_id = Some(space_resident.space_id);
            state.current_target_object_id = combat_target
                .and_then(|combat_target| object_query.get(combat_target.entity).ok())
                .map(|object| object.object_id);
        } else {
            let position = SpacePosition::new(space_resident.space_id, *tile_position);
            state.remote_players.insert(
                identity.id,
                ClientRemotePlayerState {
                    player_id: identity.id,
                    object_id: player_object.object_id,
                    position,
                    tile_position: *tile_position,
                    vitals: projected_vitals,
                },
            );
        }
    }

    let Some(local_space_id) = local_space_id else {
        return state;
    };
    let Some(runtime_space) = space_manager.get(local_space_id) else {
        return state;
    };
    state.current_space = Some(ClientSpaceState {
        space_id: runtime_space.id,
        authored_id: runtime_space.authored_id.clone(),
        width: runtime_space.width,
        height: runtime_space.height,
        fill_object_type: runtime_space.fill_object_type.clone(),
    });

    state
        .remote_players
        .retain(|_, remote_player| remote_player.position.space_id == local_space_id);

    for (container, object, resident) in container_query.iter() {
        if resident.space_id != local_space_id {
            continue;
        }
        state
            .container_slots
            .insert(object.object_id, container.slots.clone());
    }

    for (space_resident, tile_position, object, vitals, has_container, has_npc, has_movable, qty) in
        world_object_query.iter()
    {
        if space_resident.space_id != local_space_id {
            continue;
        }
        state.world_objects.insert(
            object.object_id,
            ClientWorldObjectState {
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
            },
        );
    }

    state
}

fn read_next_line(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
    disconnected: &mut bool,
) -> Option<String> {
    let mut chunk = [0; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                *disconnected = true;
                break;
            }
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => {
                warn!("TCP read failed: {error}");
                *disconnected = true;
                break;
            }
        }
    }

    let newline_index = buffer.iter().position(|byte| *byte == b'\n')?;
    let line = buffer.drain(..=newline_index).collect::<Vec<_>>();
    let payload = &line[..line.len().saturating_sub(1)];
    String::from_utf8(payload.to_vec()).ok()
}

fn write_message(
    stream: &mut TcpStream,
    message: &impl serde::Serialize,
    disconnected: &mut bool,
) -> bool {
    let Ok(mut bytes) = serde_json::to_vec(message) else {
        return false;
    };
    bytes.push(b'\n');

    match stream.write_all(&bytes) {
        Ok(()) => true,
        Err(error) if error.kind() == ErrorKind::WouldBlock => false,
        Err(error) => {
            warn!("TCP write failed: {error}");
            *disconnected = true;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::SystemState;
    use bevy::prelude::*;

    use super::*;
    use crate::combat::components::{AttackProfile, CombatLeash};
    use crate::game::GameServerPlugin;
    use crate::magic::MagicPlugin;
    use crate::npc::NpcPlugin;
    use crate::player::components::{
        BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, Player, PlayerId,
        PlayerIdentity, VitalStats,
    };
    use crate::player::PlayerServerPlugin;
    use crate::world::components::{Collider, OverworldObject};
    use crate::world::object_registry::ObjectRegistry;
    use crate::world::{WorldConfig, WorldServerPlugin};

    fn setup_server_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins((
            GameServerPlugin,
            WorldServerPlugin,
            NpcPlugin,
            PlayerServerPlugin,
            MagicPlugin,
        ));
        app.update();
        app
    }

    fn spawn_player(app: &mut App, player_id: u64, x: i32, y: i32) -> Entity {
        let base_stats = BaseStats::default();
        let derived_stats = DerivedStats::from_base(&base_stats);
        let max_health = derived_stats.max_health as f32;
        let max_mana = derived_stats.max_mana as f32;
        let current_space_id = app.world().resource::<WorldConfig>().current_space_id;
        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("player");
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity {
                    id: PlayerId(player_id),
                },
                Inventory::default(),
                ChatLog::default(),
                base_stats,
                derived_stats,
                VitalStats::full(max_health, max_mana),
                MovementCooldown::default(),
                AttackProfile::melee(),
                CombatLeash {
                    max_distance_tiles: 6,
                },
                Collider,
                OverworldObject {
                    object_id,
                    definition_id: "player".to_owned(),
                },
                crate::world::components::SpaceResident {
                    space_id: current_space_id,
                },
                crate::world::components::TilePosition::new(x, y),
            ))
            .id()
    }

    fn spawn_container(app: &mut App, type_id: &str, x: i32, y: i32) -> u64 {
        let current_space_id = app.world().resource::<WorldConfig>().current_space_id;
        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id(type_id);
        let definition = app
            .world()
            .resource::<crate::world::object_definitions::OverworldObjectDefinitions>()
            .get(type_id)
            .unwrap()
            .clone();
        let mut entity = app.world_mut().spawn((
            OverworldObject {
                object_id,
                definition_id: type_id.to_owned(),
            },
            crate::world::components::SpaceResident {
                space_id: current_space_id,
            },
            crate::world::components::TilePosition::new(x, y),
        ));
        if definition.colliding {
            entity.insert(Collider);
        }
        if let Some(capacity) = definition.container_capacity {
            entity.insert(crate::world::components::Container {
                slots: vec![None; capacity],
            });
        }
        if definition.movable {
            entity.insert(crate::world::components::Movable);
        }
        object_id
    }

    #[test]
    fn spawn_tile_skips_blocked_center_and_existing_players() {
        let mut app = setup_server_app();
        let world_config = {
            let config = app.world().resource::<WorldConfig>();
            WorldConfig {
                current_space_id: config.current_space_id,
                map_width: config.map_width,
                map_height: config.map_height,
                tile_size: config.tile_size,
                fill_object_type: config.fill_object_type.clone(),
            }
        };
        let center = crate::world::components::TilePosition::new(
            world_config.map_width / 2,
            world_config.map_height / 2,
        );

        spawn_player(&mut app, 1, center.x, center.y);
        spawn_container(&mut app, "wall", center.x + 1, center.y);

        type SpawnState<'w, 's> = SystemState<(
            Query<
                'w,
                's,
                (
                    &'static crate::world::components::SpaceResident,
                    &'static crate::world::components::TilePosition,
                ),
                With<crate::world::components::Collider>,
            >,
            Query<
                'w,
                's,
                (
                    &'static crate::world::components::SpaceResident,
                    &'static crate::world::components::TilePosition,
                ),
                With<Player>,
            >,
            Res<'w, SpaceDefinitions>,
            Res<'w, crate::world::resources::SpaceManager>,
        )>;
        let mut state: SpawnState = SystemState::new(app.world_mut());
        let (collider_query, player_query, authored_spaces, space_manager) =
            state.get(app.world_mut());
        let (_, spawn_tile) = find_spawn_location(
            &world_config,
            &authored_spaces,
            &space_manager,
            &collider_query,
            &player_query,
        )
        .unwrap();

        assert_ne!(spawn_tile, center);
        assert_ne!(
            spawn_tile,
            crate::world::components::TilePosition::new(center.x + 1, center.y)
        );
    }

    #[test]
    fn snapshot_builder_separates_local_and_remote_players() {
        let mut app = setup_server_app();
        spawn_player(&mut app, 1, 10, 10);
        spawn_player(&mut app, 2, 12, 10);
        let barrel_id = spawn_container(&mut app, "barrel", 11, 10);

        type SnapshotState<'w, 's> = SystemState<(
            Query<
                'w,
                's,
                (
                    Entity,
                    &'static crate::player::components::PlayerIdentity,
                    &'static crate::player::components::Inventory,
                    &'static crate::player::components::ChatLog,
                    &'static crate::world::components::SpaceResident,
                    &'static crate::world::components::TilePosition,
                    &'static crate::player::components::VitalStats,
                    &'static crate::player::components::DerivedStats,
                    Option<&'static crate::combat::components::CombatTarget>,
                    &'static crate::world::components::OverworldObject,
                ),
                With<Player>,
            >,
            Query<'w, 's, &'static crate::world::components::OverworldObject>,
            Query<
                'w,
                's,
                (
                    &'static crate::world::components::SpaceResident,
                    &'static crate::world::components::TilePosition,
                    &'static crate::world::components::OverworldObject,
                    Option<&'static crate::player::components::VitalStats>,
                    Has<crate::world::components::Container>,
                    Has<crate::npc::components::Npc>,
                    Has<crate::world::components::Movable>,
                    Option<&'static crate::world::components::Quantity>,
                ),
                Without<Player>,
            >,
            Query<
                'w,
                's,
                (
                    &'static crate::world::components::Container,
                    &'static crate::world::components::OverworldObject,
                    &'static crate::world::components::SpaceResident,
                ),
                Without<Player>,
            >,
            Res<'w, crate::world::resources::SpaceManager>,
        )>;
        let mut state: SnapshotState = SystemState::new(app.world_mut());
        let (player_query, object_query, world_object_query, container_query, space_manager) =
            state.get(app.world_mut());

        let snapshot = build_client_game_state(
            PlayerId(1),
            &player_query,
            &object_query,
            &world_object_query,
            &container_query,
            &space_manager,
        );

        assert_eq!(snapshot.local_player_id, Some(PlayerId(1)));
        assert_eq!(
            snapshot.player_tile_position,
            Some(crate::world::components::TilePosition::new(10, 10))
        );
        assert_eq!(snapshot.remote_players.len(), 1);
        assert_eq!(
            snapshot
                .remote_players
                .get(&PlayerId(2))
                .unwrap()
                .tile_position,
            crate::world::components::TilePosition::new(12, 10)
        );
        assert!(snapshot.container_slots.contains_key(&barrel_id));
        assert!(snapshot
            .world_objects
            .values()
            .any(|object| object.vitals.is_none()));
    }
}
