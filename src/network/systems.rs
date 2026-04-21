use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bevy::log::{error, info, warn};
use bevy::prelude::*;

use crate::accounts::{AccountDbHandle, AuthError};
use crate::app::state::ClientAppState;
use crate::assets::AssetResolver;
use crate::game::resources::{ClientGameState, PendingGameCommands, PendingGameUiEvents};
use crate::network::asset_sync::{build_server_manifest, hash_bytes};
use crate::network::protocol::{ClientMessage, ServerMessage};
use crate::network::resources::{
    AssetSyncState, ConnectionId, PeerAuthState, PendingPlayerSaves, ServerAssetManifest,
    TcpClientConfig, TcpClientConnection, TcpServerConfig, TcpServerPeer, TcpServerState,
};
use crate::network::transport::{ClientTransport, ServerTransport};
use crate::player::components::{Inventory, Player, PlayerId};
use crate::player::setup::{
    seed_starter_inventory, spawn_player_authoritative_in_space, spawn_player_from_dump,
};
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
        if peer.manifest_sent || !peer.is_authed() {
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
    server_config: Res<TcpServerConfig>,
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

                let transport = match &server_config.tls_config {
                    Some(tls_config) => match rustls::ServerConnection::new(tls_config.clone()) {
                        Ok(conn) => {
                            ServerTransport::Tls(Box::new(rustls::StreamOwned::new(conn, stream)))
                        }
                        Err(err) => {
                            warn!("failed to create TLS server connection for {address}: {err}");
                            continue;
                        }
                    },
                    None => ServerTransport::Plain(stream),
                };

                let connection_id = ConnectionId(server_state.next_connection_id);
                server_state.next_connection_id += 1;

                info!("TCP client connected from {address} (awaiting auth)");
                server_state.peers.insert(
                    connection_id,
                    TcpServerPeer {
                        connection_id,
                        auth_state: PeerAuthState::AwaitingAuth,
                        player_id: None,
                        player_entity: None,
                        stream: transport,
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
    mut pending_saves: ResMut<PendingPlayerSaves>,
    db: Option<Res<AccountDbHandle>>,
    mut var_stores: Option<ResMut<crate::dialog::resources::CharacterVarStores>>,
    world_config: Res<WorldConfig>,
    authored_spaces: Res<SpaceDefinitions>,
    space_manager: Res<SpaceManager>,
    collider_query: Query<(&SpaceResident, &TilePosition), With<Collider>>,
    player_position_query: Query<(&SpaceResident, &TilePosition), With<Player>>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut commands: Commands,
) {
    let connection_ids = server_state.peers.keys().copied().collect::<Vec<_>>();
    let mut disconnected_peers = Vec::new();

    for connection_id in connection_ids {
        let Some(peer) = server_state.peers.get_mut(&connection_id) else {
            continue;
        };

        // Drain messages into a local vec before dispatching so we don't hold
        // a borrow of `peer` across auth spawning (which mutates other
        // resources via the shared `commands`).
        let mut disconnected = false;
        let mut incoming: Vec<String> = Vec::new();
        while let Some(line) =
            read_next_line(&mut peer.stream, &mut peer.read_buffer, &mut disconnected)
        {
            incoming.push(line);
        }

        for line in incoming {
            let Some(peer) = server_state.peers.get_mut(&connection_id) else {
                break;
            };
            match serde_json::from_str::<ClientMessage>(&line) {
                Ok(ClientMessage::Login { username, password }) => {
                    let is_register = false;
                    handle_auth_attempt(
                        peer,
                        &username,
                        &password,
                        is_register,
                        db.as_deref(),
                        var_stores.as_deref_mut(),
                        &mut commands,
                        &world_config,
                        &authored_spaces,
                        &space_manager,
                        &collider_query,
                        &player_position_query,
                        &mut object_registry,
                        &mut disconnected,
                    );
                }
                Ok(ClientMessage::Register { username, password }) => {
                    let is_register = true;
                    handle_auth_attempt(
                        peer,
                        &username,
                        &password,
                        is_register,
                        db.as_deref(),
                        var_stores.as_deref_mut(),
                        &mut commands,
                        &world_config,
                        &authored_spaces,
                        &space_manager,
                        &collider_query,
                        &player_position_query,
                        &mut object_registry,
                        &mut disconnected,
                    );
                }
                Ok(ClientMessage::Command(command)) => {
                    if let Some(player_id) = peer.player_id {
                        pending_commands.push_for_player(player_id, command);
                    }
                    // Commands from unauthed peers are silently dropped.
                }
                Ok(ClientMessage::AssetRequest(paths)) => {
                    if !peer.is_authed() {
                        continue;
                    }
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
                    if !peer.is_authed() {
                        continue;
                    }
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
        disconnect_peer(
            &mut server_state,
            connection_id,
            &mut pending_saves,
            &mut commands,
        );
    }
}

/// Attempts to authenticate a peer against the account DB, then spawns the
/// player entity (either loaded from the DB or freshly initialized). Writes an
/// `AuthResult` to the peer either way.
#[allow(clippy::too_many_arguments)]
fn handle_auth_attempt(
    peer: &mut TcpServerPeer,
    username: &str,
    password: &str,
    is_register: bool,
    db: Option<&AccountDbHandle>,
    var_stores: Option<&mut crate::dialog::resources::CharacterVarStores>,
    commands: &mut Commands,
    world_config: &WorldConfig,
    authored_spaces: &SpaceDefinitions,
    space_manager: &SpaceManager,
    collider_query: &Query<(&SpaceResident, &TilePosition), With<Collider>>,
    player_position_query: &Query<(&SpaceResident, &TilePosition), With<Player>>,
    object_registry: &mut ObjectRegistry,
    disconnected: &mut bool,
) {
    if peer.is_authed() {
        // Auth attempted on an already-authed peer — ignore, don't respond.
        return;
    }

    let Some(db) = db else {
        warn!(
            "peer {} attempted auth but no account DB is configured",
            peer.connection_id.0
        );
        send_auth_failure(peer, "server has no account database", disconnected);
        return;
    };

    let auth_result = {
        let mut guard = db.lock();
        if is_register {
            guard
                .create_account(username, password)
                .and_then(|account_id| {
                    // After create, log them in to record last_login_at and to
                    // exercise the same code path as Login.
                    guard.verify_login(username, password)?;
                    Ok(account_id)
                })
        } else {
            guard.verify_login(username, password)
        }
    };

    let account_id = match auth_result {
        Ok(id) => id,
        Err(err) => {
            info!("peer {} auth rejected: {err}", peer.connection_id.0);
            let reason = reason_for_auth_error(&err);
            send_auth_failure(peer, &reason, disconnected);
            return;
        }
    };

    // Resolve the player entity — either restored from the DB or spawned fresh.
    let player_id = PlayerId(account_id as u64);
    let existing = db.lock().load_character(account_id).ok().flatten();

    let entity = if let Some(dump) = existing {
        let dump_player_id = dump.player_id.0;
        let yarn_vars = dump.yarn_vars.clone();
        let (entity, _combat_target) = spawn_player_from_dump(
            commands,
            object_registry,
            dump,
            world_config.current_space_id,
        );
        if let Some(stores) = var_stores {
            stores.restore(dump_player_id, yarn_vars);
        }
        entity
    } else {
        let Some((spawn_space_id, spawn_tile)) = find_spawn_location(
            world_config,
            authored_spaces,
            space_manager,
            collider_query,
            player_position_query,
        ) else {
            warn!(
                "peer {} authenticated but no free spawn tile is available",
                peer.connection_id.0
            );
            send_auth_failure(peer, "no free spawn location", disconnected);
            return;
        };
        let object_id = object_registry.allocate_runtime_id("player");
        let entity = spawn_player_authoritative_in_space(
            commands,
            player_id,
            object_id,
            spawn_space_id,
            spawn_tile,
        );
        let mut starter = Inventory::default();
        seed_starter_inventory(&mut starter, object_registry);
        commands.entity(entity).insert(starter);
        entity
    };

    peer.auth_state = PeerAuthState::Authed { account_id };
    peer.player_id = Some(player_id);
    peer.player_entity = Some(entity);
    info!(
        "peer {} authenticated as account {account_id} ({username})",
        peer.connection_id.0
    );
    write_message(
        &mut peer.stream,
        &ServerMessage::AuthResult {
            ok: true,
            reason: None,
        },
        disconnected,
    );
}

fn send_auth_failure(peer: &mut TcpServerPeer, reason: &str, disconnected: &mut bool) {
    write_message(
        &mut peer.stream,
        &ServerMessage::AuthResult {
            ok: false,
            reason: Some(reason.to_owned()),
        },
        disconnected,
    );
}

fn reason_for_auth_error(err: &AuthError) -> String {
    match err {
        AuthError::UsernameInvalid(msg) => format!("username invalid: {msg}"),
        AuthError::PasswordInvalid(msg) => format!("password invalid: {msg}"),
        AuthError::UsernameTaken => "username already taken".to_owned(),
        AuthError::UnknownUser => "unknown user".to_owned(),
        AuthError::WrongPassword => "wrong password".to_owned(),
        // Don't leak internal errors to the client.
        AuthError::Database(_) | AuthError::Hashing(_) => "internal server error".to_owned(),
    }
}

pub fn flush_server_messages(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    mut server_state: ResMut<TcpServerState>,
    mut pending_saves: ResMut<PendingPlayerSaves>,
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

        if !peer.is_authed() {
            continue;
        }

        let mut disconnected = false;

        if peer.sync_complete {
            let Some(player_id) = peer.player_id else {
                continue;
            };
            // Per-peer event stream — the sole state-replication path. Passing the
            // peer's last projection as the baseline (or default, for bootstrap)
            // produces the exact delta the peer needs; apply_event_to_state then
            // advances the baseline so subsequent diffs stay coherent.
            let default_baseline = ClientGameState::default();
            let baseline = peer.last_projection.as_ref().unwrap_or(&default_baseline);
            let events = crate::game::projection::compute_events_for_peer(
                player_id,
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

            let mut outgoing_ui_events =
                peer_ui_events.get(&player_id).cloned().unwrap_or_default();
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
        disconnect_peer(
            &mut server_state,
            connection_id,
            &mut pending_saves,
            &mut commands,
        );
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
            Ok(ServerMessage::AuthResult { .. }) => {
                // AuthResult is handled by the Authenticating-state systems;
                // reaching InGame means auth already succeeded.
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

pub fn ensure_tcp_client_connected(config: &TcpClientConfig, connection: &mut TcpClientConnection) {
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

    let transport = match &config.tls {
        Some(tls) => {
            let server_name = match rustls::pki_types::ServerName::try_from(tls.server_name.clone())
            {
                Ok(name) => name,
                Err(err) => {
                    warn!("invalid TLS server_name {:?}: {err}", tls.server_name);
                    return;
                }
            };
            match rustls::ClientConnection::new(tls.config.clone(), server_name) {
                Ok(conn) => ClientTransport::Tls(Box::new(rustls::StreamOwned::new(conn, stream))),
                Err(err) => {
                    warn!("failed to create TLS client connection: {err}");
                    return;
                }
            }
        }
        None => ClientTransport::Plain(stream),
    };

    info!(
        "connected to TCP server at {} (TLS: {})",
        config.server_addr,
        config.tls.is_some()
    );
    connection.stream = Some(transport);
}

fn disconnect_peer(
    server_state: &mut TcpServerState,
    connection_id: ConnectionId,
    pending_saves: &mut PendingPlayerSaves,
    commands: &mut Commands,
) {
    if let Some(peer) = server_state.peers.remove(&connection_id) {
        info!("TCP client disconnected");
        if let (Some(account_id), Some(player_entity)) = (peer.account_id(), peer.player_entity) {
            // Defer the snapshot+despawn to `persist_disconnected_players` in
            // the `Last` schedule — that system holds the heavy player query
            // needed to build a `PlayerStateDump`.
            pending_saves
                .entries
                .push(crate::network::resources::PendingPlayerSave {
                    account_id,
                    player_entity,
                });
        } else if let Some(entity) = peer.player_entity {
            commands.entity(entity).despawn();
        }
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

pub fn read_next_line<S: Read>(
    stream: &mut S,
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

pub fn write_message<S: Write>(
    stream: &mut S,
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
