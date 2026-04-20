use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;

use bevy::prelude::*;
use rustls::{ClientConfig, ServerConfig};

use crate::game::resources::ClientGameState;
use crate::network::protocol::AssetEntry;
use crate::network::transport::{ClientTransport, ServerTransport};

use crate::player::components::PlayerId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionId(pub u64);

#[derive(Resource)]
pub struct TcpClientConfig {
    pub server_addr: String,
    pub active: bool,
    /// When `Some`, outgoing connections are wrapped in TLS. `server_name` is
    /// the SNI hostname used during the handshake — typically the host part
    /// of `server_addr`.
    pub tls: Option<TcpClientTlsConfig>,
}

#[derive(Clone)]
pub struct TcpClientTlsConfig {
    pub config: Arc<ClientConfig>,
    pub server_name: String,
}

#[derive(Resource, Default)]
pub struct TcpClientConnection {
    pub stream: Option<ClientTransport>,
    pub read_buffer: Vec<u8>,
}

#[derive(Resource)]
pub struct TcpServerConfig {
    pub bind_addr: String,
    /// When `Some`, accepted connections are wrapped in TLS during accept.
    pub tls_config: Option<Arc<ServerConfig>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerAuthState {
    /// Accepts only `Login` / `Register` messages. No player entity exists yet.
    AwaitingAuth,
    /// Credentials accepted. Asset sync and gameplay messages are allowed from
    /// here on. `account_id` is the primary key in the accounts DB.
    Authed { account_id: i64 },
}

pub struct TcpServerPeer {
    pub connection_id: ConnectionId,
    pub auth_state: PeerAuthState,
    /// Some(_) iff `auth_state == Authed`.
    pub player_id: Option<PlayerId>,
    /// Some(_) iff `auth_state == Authed`.
    pub player_entity: Option<Entity>,
    pub stream: ServerTransport,
    pub read_buffer: Vec<u8>,
    /// Per-peer projection baseline used to emit delta events. Starts `None`;
    /// on the first tick after `sync_complete`, `compute_events_for_peer`
    /// diffs against a default `ClientGameState` and produces a full bootstrap
    /// stream.
    pub last_projection: Option<ClientGameState>,
    pub sync_complete: bool,
    pub manifest_sent: bool,
}

impl TcpServerPeer {
    pub fn is_authed(&self) -> bool {
        matches!(self.auth_state, PeerAuthState::Authed { .. })
    }

    pub fn account_id(&self) -> Option<i64> {
        match self.auth_state {
            PeerAuthState::Authed { account_id } => Some(account_id),
            PeerAuthState::AwaitingAuth => None,
        }
    }
}

#[derive(Resource, Default)]
pub struct TcpServerState {
    pub listener: Option<TcpListener>,
    pub next_connection_id: u64,
    pub peers: HashMap<ConnectionId, TcpServerPeer>,
}

/// Queue of player entities whose DB state needs to be saved before despawn.
/// Populated by the network layer on disconnect; drained by
/// `persist_disconnected_players` running in the `Last` schedule.
#[derive(Resource, Default)]
pub struct PendingPlayerSaves {
    pub entries: Vec<PendingPlayerSave>,
}

pub struct PendingPlayerSave {
    pub account_id: i64,
    pub player_entity: Entity,
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
