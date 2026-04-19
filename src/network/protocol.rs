use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::game::resources::{ClientGameState, GameUiEvent};

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
    Snapshot(ClientGameState),
    UiEvents(Vec<GameUiEvent>),
    AssetManifest(Vec<AssetEntry>),
    AssetData { path: String, data: String },
}
