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
}
