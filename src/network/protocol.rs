use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::game::resources::{GameEvent, GameUiEvent};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AssetEntry {
    pub path: String,
    pub hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ClientMessage {
    Command(GameCommand),
    AssetRequest(Vec<String>),
    SyncComplete,
    /// Pre-auth only: present credentials for an existing account.
    Login {
        username: String,
        password: String,
    },
    /// Pre-auth only: create a new account + log in in one step.
    Register {
        username: String,
        password: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ServerMessage {
    /// State replication — full replay on first tick after `sync_complete`,
    /// deltas thereafter. See `compute_events_for_peer` in
    /// `crate::game::projection`.
    Events(Vec<GameEvent>),
    /// One-shot UI signals orthogonal to state replication (e.g. "open this
    /// container"). Not delta-coded; sent once, acted on once.
    UiEvents(Vec<GameUiEvent>),
    AssetManifest(Vec<AssetEntry>),
    AssetData {
        path: String,
        data: String,
    },
    /// Response to `ClientMessage::Login` / `Register`. On `ok = true` the peer
    /// transitions to authed; the asset sync + gameplay stream follow. On
    /// `ok = false`, `reason` is a short human-readable string for the client
    /// to display.
    AuthResult {
        ok: bool,
        reason: Option<String>,
    },
}
