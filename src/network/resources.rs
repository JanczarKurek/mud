use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Instant;

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
    /// Set to true after the first connect attempt (success or failure) so the
    /// per-frame `ensure_tcp_client_connected` callers don't redial the server
    /// every frame. Reset to `false` by the title screen when the user clicks
    /// Connect again.
    pub connect_attempted: bool,
    /// Populated by `ensure_tcp_client_connected` when `TcpStream::connect_timeout`
    /// or TLS setup fails. Drained by the auth screen to surface the failure
    /// reason and bounce back to the title screen.
    pub error_message: Option<String>,
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
    /// Credentials accepted. Accepts only `ListCharacters` / `CreateCharacter`
    /// / `SelectCharacter` / `DeleteCharacter`. No player entity exists yet.
    AwaitingCharacter { account_id: i64 },
    /// A character has been selected. Asset sync and gameplay messages are
    /// allowed from here on. `account_id` owns the character; `character_id`
    /// is the gameplay identity (and is the source of `PlayerId`).
    Authed { account_id: i64, character_id: i64 },
}

pub struct TcpServerPeer {
    pub connection_id: ConnectionId,
    pub auth_state: PeerAuthState,
    /// Remote socket address captured at accept time. Stays valid even after
    /// the underlying stream is gone, so the disconnect log can still cite it.
    pub remote_addr: Option<SocketAddr>,
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
    pub latency: PeerLatencyState,
}

/// Per-peer RTT tracking, populated by the Ping/Pong cycle. All fields are
/// `None` until the first pong returns.
#[derive(Default, Clone, Copy, Debug)]
pub struct PeerLatencyState {
    /// Nonce of the most recent outstanding ping. A pong carrying a different
    /// nonce is silently dropped.
    pub last_ping_nonce: Option<u64>,
    pub last_ping_sent_at: Option<Instant>,
    pub last_rtt_ms: Option<f64>,
    /// Exponential moving average of recent RTTs (alpha = 0.2).
    pub ema_rtt_ms: Option<f64>,
}

impl TcpServerPeer {
    pub fn is_authed(&self) -> bool {
        matches!(self.auth_state, PeerAuthState::Authed { .. })
    }

    pub fn is_awaiting_character(&self) -> bool {
        matches!(self.auth_state, PeerAuthState::AwaitingCharacter { .. })
    }

    pub fn account_id(&self) -> Option<i64> {
        match self.auth_state {
            PeerAuthState::Authed { account_id, .. } => Some(account_id),
            PeerAuthState::AwaitingCharacter { account_id } => Some(account_id),
            PeerAuthState::AwaitingAuth => None,
        }
    }

    pub fn character_id(&self) -> Option<i64> {
        match self.auth_state {
            PeerAuthState::Authed { character_id, .. } => Some(character_id),
            _ => None,
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
    pub character_id: i64,
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

/// Drives `send_periodic_pings`. Cadence: every `interval_seconds`, the server
/// emits a fresh `ServerMessage::Ping` to each authed peer. Mirrors
/// `AutosaveTimer` (`src/accounts/autosave.rs`).
#[derive(Resource)]
pub struct PingTimer {
    pub elapsed_since_ping: f64,
    pub interval_seconds: f64,
    /// Monotonic counter used as the next ping nonce. Wraps harmlessly.
    pub next_nonce: u64,
}

impl Default for PingTimer {
    fn default() -> Self {
        Self {
            elapsed_since_ping: 0.0,
            interval_seconds: 5.0,
            next_nonce: 1,
        }
    }
}

/// Drives `report_peer_latency`. Cadence: every `interval_seconds`, the server
/// info-logs one line per connected peer with the last observed RTT + EMA.
#[derive(Resource)]
pub struct LatencyReportTimer {
    pub elapsed_since_report: f64,
    pub interval_seconds: f64,
}

impl Default for LatencyReportTimer {
    fn default() -> Self {
        Self {
            elapsed_since_report: 0.0,
            interval_seconds: 60.0,
        }
    }
}
