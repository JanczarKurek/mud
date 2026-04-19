use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::network::protocol::AssetEntry;

use crate::player::components::PlayerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionId(pub u64);

#[derive(Resource)]
pub struct TcpClientConfig {
    pub server_addr: String,
    pub active: bool,
}

#[derive(Resource, Default)]
pub struct TcpClientConnection {
    pub stream: Option<TcpStream>,
    pub read_buffer: Vec<u8>,
}

#[derive(Resource)]
pub struct TcpServerConfig {
    pub bind_addr: String,
}

pub struct TcpServerPeer {
    pub connection_id: ConnectionId,
    pub player_id: PlayerId,
    pub player_entity: Entity,
    pub stream: TcpStream,
    pub read_buffer: Vec<u8>,
    /// Per-peer projection baseline used to emit delta events. Starts `None`;
    /// on the first tick after `sync_complete`, `compute_events_for_peer`
    /// diffs against a default `ClientGameState` and produces a full bootstrap
    /// stream.
    pub last_projection: Option<ClientGameState>,
    pub sync_complete: bool,
    pub manifest_sent: bool,
}

#[derive(Resource, Default)]
pub struct TcpServerState {
    pub listener: Option<TcpListener>,
    pub next_connection_id: u64,
    pub peers: HashMap<ConnectionId, TcpServerPeer>,
}

#[derive(Resource)]
pub struct ServerAssetManifest(pub Vec<AssetEntry>);

#[derive(Resource, Default)]
pub struct AssetSyncState {
    pub manifest_received: bool,
    pub pending_paths: Vec<String>,
    pub received_count: usize,
    pub total_needed: usize,
    pub log_messages: Vec<String>,
}
