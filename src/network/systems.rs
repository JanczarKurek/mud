use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bevy::log::{error, info, warn};
use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::assets::AssetResolver;
use crate::game::resources::{ClientGameState, PendingGameCommands, PendingGameUiEvents};
use crate::network::asset_sync::{build_server_manifest, hash_bytes};
use crate::network::protocol::{ClientMessage, ServerMessage};
use crate::network::resources::{
    AssetSyncState, ConnectionId, ServerAssetManifest, TcpClientConfig, TcpClientConnection,
    TcpServerConfig, TcpServerPeer, TcpServerState,
};
use crate::player::components::{Player, PlayerId};
use crate::player::setup::spawn_player_authoritative_in_space;
use crate::world::components::{Collider, SpaceResident, TilePosition};
use crate::world::map_layout::SpaceDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::WorldConfig;

pub fn build_and_store_manifest(mut commands: Commands) {
    let manifest = build_server_manifest();
    info!("asset manifest built: {} files", manifest.len());
    commands.insert_resource(ServerAssetManifest(manifest));
}

pub fn send_asset_manifest_to_new_peers(
    manifest: Res<ServerAssetManifest>,
    mut server_state: ResMut<TcpServerState>,
) {
    for peer in server_state.peers.values_mut() {
        if peer.manifest_sent {
            continue;
        }
        info!(
            "sending asset manifest ({} files) to peer {}",
            manifest.0.len(),
            peer.connection_id.0
        );
        let mut disconnected = false;
        write_message(
            &mut peer.stream,
            &ServerMessage::AssetManifest(manifest.0.clone()),
            &mut disconnected,
        );
        peer.manifest_sent = true;
    }
}

pub fn poll_tcp_asset_sync_messages(
    config: Res<TcpClientConfig>,
    mut connection: ResMut<TcpClientConnection>,
    resolver: Res<AssetResolver>,
    mut sync_state: ResMut<AssetSyncState>,
    mut next_state: ResMut<NextState<ClientAppState>>,
) {
    ensure_tcp_client_connected(&config, &mut connection);

    let mut read_buffer = std::mem::take(&mut connection.read_buffer);
    let Some(stream) = &mut connection.stream else {
        connection.read_buffer = read_buffer;
        return;
    };

    let mut disconnected = false;
    let mut files_to_write: Vec<(String, Vec<u8>)> = Vec::new();
    let mut request_paths: Option<Vec<String>> = None;
    let mut send_sync_complete = false;
    let mut transition_to_ingame = false;

    while let Some(line) = read_next_line(stream, &mut read_buffer, &mut disconnected) {
        match serde_json::from_str::<ServerMessage>(&line) {
            Ok(ServerMessage::AssetManifest(entries)) => {
                let missing: Vec<String> = entries
                    .iter()
                    .filter(|e| !is_asset_current(&e.path, &e.hash, &resolver))
                    .map(|e| e.path.clone())
                    .collect();

                sync_state.manifest_received = true;
                sync_state.total_needed = missing.len();
                sync_state.received_count = 0;
                sync_state.pending_paths.clone_from(&missing);

                if missing.is_empty() {
                    info!("asset sync: all {} assets up to date", entries.len());
                    send_sync_complete = true;
                    transition_to_ingame = true;
                } else {
                    info!(
                        "asset sync: need {} of {} assets",
                        missing.len(),
                        entries.len()
                    );
                    request_paths = Some(missing);
                }
            }
            Ok(ServerMessage::AssetData { path, data }) => match BASE64.decode(&data) {
                Ok(bytes) => {
                    files_to_write.push((path.clone(), bytes));
                    sync_state.pending_paths.retain(|p| p != &path);
                    sync_state.received_count += 1;
                    let msg = format!(
                        "[{}/{}] {}",
                        sync_state.received_count, sync_state.total_needed, path
                    );
                    info!("asset sync: {}", msg);
                    sync_state.log_messages.push(msg);

                    if sync_state.pending_paths.is_empty() {
                        info!("asset sync: all assets downloaded");
                        send_sync_complete = true;
                        transition_to_ingame = true;
                    }
                }
                Err(err) => warn!("asset sync: failed to decode {}: {err}", path),
            },
            Ok(_) => {}
            Err(error) => warn!("asset sync: failed to parse server message: {error}"),
        }
    }

    if let Some(paths) = request_paths {
        let mut disc = false;
        write_message(stream, &ClientMessage::AssetRequest(paths), &mut disc);
    }
    if send_sync_complete {
        let mut disc = false;
        write_message(stream, &ClientMessage::SyncComplete, &mut disc);
    }

    if disconnected {
        warn!(
            "lost TCP connection to {} during asset sync",
            config.server_addr
        );
        connection.stream = None;
        connection.read_buffer.clear();
    } else {
        connection.read_buffer = read_buffer;
    }

    if let Some(xdg_dir) = resolver.xdg_assets_dir() {
        for (path, bytes) in files_to_write {
            let target = xdg_dir.join(&path);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(err) = std::fs::write(&target, &bytes) {
                warn!("asset sync: failed to write {}: {err}", path);
            }
        }
    }

    if transition_to_ingame {
        next_state.set(ClientAppState::InGame);
    }
}

fn is_asset_current(path: &str, expected_hash: &str, resolver: &AssetResolver) -> bool {
    let bundled = PathBuf::from("assets").join(path);
    let candidates: Vec<PathBuf> = resolver
        .xdg_assets_dir()
        .map(|d| vec![d.join(path), bundled.clone()])
        .unwrap_or_else(|| vec![bundled]);

    for candidate in &candidates {
        if let Ok(data) = std::fs::read(candidate) {
            if hash_bytes(&data) == expected_hash {
                return true;
            }
        }
    }
    false
}

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
                let mut starter = crate::player::components::Inventory::default();
                crate::player::setup::seed_starter_inventory(&mut starter, &mut object_registry);
                commands.entity(player_entity).insert(starter);

                info!("TCP client connected from {address}");
                server_state.peers.insert(
                    connection_id,
                    TcpServerPeer {
                        connection_id,
                        player_id,
                        player_entity,
                        stream,
                        read_buffer: Vec::new(),
                        last_projection: None,
                        sync_complete: false,
                        manifest_sent: false,
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
                Ok(ClientMessage::AssetRequest(paths)) => {
                    info!(
                        "peer {} requested {} assets",
                        peer.connection_id.0,
                        paths.len()
                    );
                    for path in &paths {
                        let file_path = PathBuf::from("assets").join(path);
                        match std::fs::read(&file_path) {
                            Ok(data) => {
                                let encoded = BASE64.encode(&data);
                                let mut disc = false;
                                write_message(
                                    &mut peer.stream,
                                    &ServerMessage::AssetData {
                                        path: path.clone(),
                                        data: encoded,
                                    },
                                    &mut disc,
                                );
                                if disc {
                                    disconnected = true;
                                    break;
                                }
                            }
                            Err(err) => {
                                warn!("asset sync: failed to read {}: {err}", path);
                            }
                        }
                    }
                }
                Ok(ClientMessage::SyncComplete) => {
                    info!("peer {} asset sync complete", peer.connection_id.0);
                    peer.sync_complete = true;
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
    player_query: crate::game::projection::ProjectionPlayerQuery,
    object_query: crate::game::projection::ProjectionObjectQuery,
    world_object_query: crate::game::projection::ProjectionWorldObjectQuery,
    container_query: crate::game::projection::ProjectionContainerQuery,
    space_manager: Res<SpaceManager>,
    mut commands: Commands,
) {
    let peer_ui_events = std::mem::take(&mut pending_ui_events.peer_events);
    let broadcast_ui_events = std::mem::take(&mut pending_ui_events.events);

    let connection_ids = server_state.peers.keys().copied().collect::<Vec<_>>();
    let mut disconnected_peers = Vec::new();

    for connection_id in connection_ids {
        let Some(peer) = server_state.peers.get_mut(&connection_id) else {
            continue;
        };

        let mut disconnected = false;

        if peer.sync_complete {
            // Per-peer event stream — the sole state-replication path. Passing the
            // peer's last projection as the baseline (or default, for bootstrap)
            // produces the exact delta the peer needs; apply_event_to_state then
            // advances the baseline so subsequent diffs stay coherent.
            let default_baseline = ClientGameState::default();
            let baseline = peer.last_projection.as_ref().unwrap_or(&default_baseline);
            let events = crate::game::projection::compute_events_for_peer(
                peer.player_id,
                baseline,
                &player_query,
                &object_query,
                &world_object_query,
                &container_query,
                &space_manager,
            );
            if !events.is_empty() {
                if !write_message(
                    &mut peer.stream,
                    &ServerMessage::Events(events.clone()),
                    &mut disconnected,
                ) {
                    warn!("failed to send events to TCP client");
                } else {
                    let mut next_baseline = peer.last_projection.take().unwrap_or_default();
                    for event in events {
                        crate::game::projection::apply_event_to_state(&mut next_baseline, event);
                    }
                    peer.last_projection = Some(next_baseline);
                }
            }

            let mut outgoing_ui_events = peer_ui_events
                .get(&peer.player_id)
                .cloned()
                .unwrap_or_default();
            outgoing_ui_events.extend(broadcast_ui_events.iter().cloned());
            if !outgoing_ui_events.is_empty()
                && !write_message(
                    &mut peer.stream,
                    &ServerMessage::UiEvents(outgoing_ui_events),
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
    mut pending_game_events: ResMut<crate::game::resources::PendingGameEvents>,
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
            Ok(ServerMessage::Events(events)) => {
                pending_game_events.events.extend(events);
            }
            Ok(ServerMessage::UiEvents(events)) => {
                pending_ui_events.events.extend(events);
            }
            Ok(ServerMessage::AssetManifest(_) | ServerMessage::AssetData { .. }) => {}
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
    let origin = TilePosition::ground(width / 2, height / 2);

    for radius in 0..width.max(height) {
        for y in -radius..=radius {
            for x in -radius..=radius {
                if radius > 0 && x.abs() != radius && y.abs() != radius {
                    continue;
                }

                let candidate = TilePosition::ground(origin.x + x, origin.y + y);
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
        PlayerIdentity, VitalStats, WeaponDamage,
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
                (AttackProfile::melee(), WeaponDamage::default()),
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
                crate::world::components::TilePosition::ground(x, y),
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
            crate::world::components::TilePosition::ground(x, y),
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
        let center = crate::world::components::TilePosition::ground(
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
            crate::world::components::TilePosition::ground(center.x + 1, center.y)
        );
    }

    #[test]
    fn peer_projection_separates_local_and_remote_players() {
        let mut app = setup_server_app();
        spawn_player(&mut app, 1, 10, 10);
        spawn_player(&mut app, 2, 12, 10);
        let barrel_id = spawn_container(&mut app, "barrel", 11, 10);

        type PeerProjectionState<'w, 's> = SystemState<(
            crate::game::projection::ProjectionPlayerQuery<'w, 's>,
            crate::game::projection::ProjectionObjectQuery<'w, 's>,
            crate::game::projection::ProjectionWorldObjectQuery<'w, 's>,
            crate::game::projection::ProjectionContainerQuery<'w, 's>,
            Res<'w, crate::world::resources::SpaceManager>,
        )>;
        let mut state: PeerProjectionState = SystemState::new(app.world_mut());
        let (player_query, object_query, world_object_query, container_query, space_manager) =
            state.get(app.world_mut());

        // Fold the bootstrap events (diff from default) into a baseline; this is
        // exactly what a freshly connected peer would do on the client side.
        let events = crate::game::projection::compute_events_for_peer(
            PlayerId(1),
            &ClientGameState::default(),
            &player_query,
            &object_query,
            &world_object_query,
            &container_query,
            &space_manager,
        );
        let mut projection = ClientGameState::default();
        for event in events {
            crate::game::projection::apply_event_to_state(&mut projection, event);
        }

        assert_eq!(projection.local_player_id, Some(PlayerId(1)));
        assert_eq!(
            projection.player_tile_position,
            Some(crate::world::components::TilePosition::ground(10, 10))
        );
        assert_eq!(projection.remote_players.len(), 1);
        assert_eq!(
            projection
                .remote_players
                .get(&PlayerId(2))
                .unwrap()
                .tile_position,
            crate::world::components::TilePosition::ground(12, 10)
        );
        assert!(projection.container_slots.contains_key(&barrel_id));
        assert!(projection
            .world_objects
            .values()
            .any(|object| object.vitals.is_none()));
    }

    #[test]
    fn peer_projection_emits_only_deltas_after_bootstrap() {
        // Bootstrap one peer into a baseline, then verify that with no ECS changes
        // the next compute_events_for_peer call emits zero events, and that a
        // single player move emits exactly one PlayerPositionChanged event.
        use crate::game::resources::GameEvent;
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 10, 10);

        type PeerProjectionState<'w, 's> = SystemState<(
            crate::game::projection::ProjectionPlayerQuery<'w, 's>,
            crate::game::projection::ProjectionObjectQuery<'w, 's>,
            crate::game::projection::ProjectionWorldObjectQuery<'w, 's>,
            crate::game::projection::ProjectionContainerQuery<'w, 's>,
            Res<'w, crate::world::resources::SpaceManager>,
        )>;

        let mut state: PeerProjectionState = SystemState::new(app.world_mut());
        let (player_query, object_query, world_object_query, container_query, space_manager) =
            state.get(app.world_mut());

        let bootstrap = crate::game::projection::compute_events_for_peer(
            PlayerId(1),
            &ClientGameState::default(),
            &player_query,
            &object_query,
            &world_object_query,
            &container_query,
            &space_manager,
        );
        let mut baseline = ClientGameState::default();
        for event in bootstrap {
            crate::game::projection::apply_event_to_state(&mut baseline, event);
        }

        // Idle tick with no ECS changes — must emit zero events.
        let idle_events = crate::game::projection::compute_events_for_peer(
            PlayerId(1),
            &baseline,
            &player_query,
            &object_query,
            &world_object_query,
            &container_query,
            &space_manager,
        );
        assert!(
            idle_events.is_empty(),
            "expected zero events when nothing changed, got: {idle_events:?}"
        );
        drop((
            player_query,
            object_query,
            world_object_query,
            container_query,
            space_manager,
        ));

        // Move the player; the next diff should contain exactly one PlayerPositionChanged.
        app.world_mut()
            .entity_mut(player)
            .insert(crate::world::components::TilePosition::ground(11, 10));

        let mut state: PeerProjectionState = SystemState::new(app.world_mut());
        let (player_query, object_query, world_object_query, container_query, space_manager) =
            state.get(app.world_mut());

        let move_events = crate::game::projection::compute_events_for_peer(
            PlayerId(1),
            &baseline,
            &player_query,
            &object_query,
            &world_object_query,
            &container_query,
            &space_manager,
        );
        let position_change_count = move_events
            .iter()
            .filter(|event| matches!(event, GameEvent::PlayerPositionChanged { .. }))
            .count();
        assert_eq!(position_change_count, 1, "events: {move_events:?}");
    }
}
