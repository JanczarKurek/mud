use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::game::resources::{ClientGameState, GameUiEvent};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ClientMessage {
    Command(GameCommand),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ServerMessage {
    Snapshot(ClientGameState),
    UiEvents(Vec<GameUiEvent>),
}
