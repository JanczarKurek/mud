use std::path::PathBuf;

use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct WorldSnapshotStatus {
    pub loaded: bool,
    /// True when the snapshot had ≥1 player entries — used by
    /// `spawn_embedded_player_authoritative` to avoid spawning a duplicate
    /// when the snapshot was empty (e.g. server saved after all clients left).
    pub players_restored: bool,
}

#[derive(Resource, Clone, Debug)]
pub struct WorldSaveConfig {
    pub path: PathBuf,
}
