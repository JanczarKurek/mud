use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::game::resources::{GameEvent, GameUiEvent};
use crate::player::classes::Class;
use crate::player::components::AttributeSet;

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
    /// Post-auth, pre-character: ask the server for this account's character
    /// roster. The server replies with `ServerMessage::CharacterList`.
    ListCharacters,
    /// Post-auth, pre-character: create a new character for the current
    /// account. Server replies with `ServerMessage::CharacterCreateResult`;
    /// on success the client should re-issue `ListCharacters` to see the new
    /// roster.
    CreateCharacter {
        name: String,
        class: Class,
        attributes: AttributeSet,
    },
    /// Post-auth, pre-character: pick a character to play. Server spawns the
    /// player entity, sends `ServerMessage::CharacterSelected`, then begins
    /// the asset-manifest + gameplay-event stream.
    SelectCharacter {
        character_id: i64,
    },
    /// Post-auth, pre-character: delete a character. Server replies with an
    /// updated `CharacterList`.
    DeleteCharacter {
        character_id: i64,
    },
}

/// Summary of a character shown on the Character Select screen.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CharacterSummary {
    pub character_id: i64,
    pub name: String,
    pub class: Class,
    pub level: u32,
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
    /// transitions to "awaiting character selection". On `ok = false`, `reason`
    /// is a short human-readable string for the client to display.
    AuthResult {
        ok: bool,
        reason: Option<String>,
    },
    /// Response to `ClientMessage::ListCharacters` (or sent unsolicited after
    /// a successful `CreateCharacter` / `DeleteCharacter`).
    CharacterList(Vec<CharacterSummary>),
    /// Response to `ClientMessage::CreateCharacter`.
    CharacterCreateResult {
        ok: bool,
        character_id: Option<i64>,
        reason: Option<String>,
    },
    /// Sent after a successful `ClientMessage::SelectCharacter`. The peer is
    /// now "in game" — the asset manifest + gameplay event stream follow.
    CharacterSelected {
        character_id: i64,
    },
}
