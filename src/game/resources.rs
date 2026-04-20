use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::player::components::{ChatLog, Inventory, InventoryStack, PlayerId};
use crate::world::components::{SpacePosition, TilePosition};

pub type InventoryState = Inventory;
pub type ChatLogState = ChatLog;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum GameUiEvent {
    OpenContainer {
        object_id: u64,
    },
    ProjectileFired {
        from_tile: TilePosition,
        to_tile: TilePosition,
        sprite_definition_id: String,
    },
}

#[derive(Clone, Debug)]
pub struct QueuedGameCommand {
    pub player_id: Option<PlayerId>,
    pub command: GameCommand,
}

#[derive(Resource, Default)]
pub struct PendingGameCommands {
    pub commands: Vec<QueuedGameCommand>,
}

impl PendingGameCommands {
    pub fn push(&mut self, command: GameCommand) {
        self.commands.push(QueuedGameCommand {
            player_id: None,
            command,
        });
    }

    pub fn push_for_player(&mut self, player_id: PlayerId, command: GameCommand) {
        self.commands.push(QueuedGameCommand {
            player_id: Some(player_id),
            command,
        });
    }
}

#[derive(Resource, Default)]
pub struct PendingGameUiEvents {
    pub events: Vec<GameUiEvent>,
    pub peer_events: HashMap<PlayerId, Vec<GameUiEvent>>,
}

impl PendingGameUiEvents {
    pub fn push(&mut self, player_id: PlayerId, event: GameUiEvent) {
        self.events.push(event.clone());
        self.peer_events.entry(player_id).or_default().push(event);
    }

    pub fn push_broadcast(&mut self, event: GameUiEvent) {
        self.events.push(event);
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ClientVitalStats {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientWorldObjectState {
    pub object_id: u64,
    pub definition_id: String,
    pub position: SpacePosition,
    pub tile_position: TilePosition,
    pub vitals: Option<ClientVitalStats>,
    pub is_container: bool,
    pub is_npc: bool,
    pub is_movable: bool,
    pub quantity: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientRemotePlayerState {
    pub player_id: PlayerId,
    pub object_id: u64,
    pub position: SpacePosition,
    pub tile_position: TilePosition,
    pub vitals: ClientVitalStats,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientSpaceState {
    pub space_id: crate::world::components::SpaceId,
    pub authored_id: String,
    pub width: i32,
    pub height: i32,
    pub fill_object_type: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum GameEvent {
    /// Emitted once per peer when the bootstrap stream begins so the client
    /// learns its own `PlayerId` + player `object_id`. These two fields cannot be
    /// reconstructed from any other event, so without this variant a wire-only
    /// client has no way to distinguish its own avatar from remote players.
    LocalPlayerIdentified {
        player_id: PlayerId,
        object_id: u64,
    },
    InventoryChanged {
        inventory: Inventory,
    },
    ChatLogChanged {
        lines: Vec<String>,
    },
    PlayerPositionChanged {
        position: SpacePosition,
        tile_position: TilePosition,
    },
    CurrentSpaceChanged {
        space: ClientSpaceState,
    },
    PlayerVitalsChanged {
        vitals: ClientVitalStats,
    },
    PlayerStorageChanged {
        storage_slots: usize,
    },
    CombatTargetChanged {
        target_object_id: Option<u64>,
    },
    ContainerChanged {
        object_id: u64,
        slots: Vec<Option<InventoryStack>>,
    },
    ContainerRemoved {
        object_id: u64,
    },
    WorldObjectUpserted {
        object: ClientWorldObjectState,
    },
    WorldObjectRemoved {
        object_id: u64,
    },
    RemotePlayerUpserted {
        player: ClientRemotePlayerState,
    },
    RemotePlayerRemoved {
        player_id: PlayerId,
    },
}

#[derive(Resource, Default)]
pub struct PendingGameEvents {
    pub events: Vec<GameEvent>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Resource, Serialize)]
pub struct ClientGameState {
    pub local_player_id: Option<PlayerId>,
    pub inventory: Inventory,
    pub chat_log_lines: Vec<String>,
    pub player_position: Option<SpacePosition>,
    pub player_tile_position: Option<TilePosition>,
    pub current_space: Option<ClientSpaceState>,
    pub player_vitals: Option<ClientVitalStats>,
    pub player_storage_slots: usize,
    pub current_target_object_id: Option<u64>,
    pub local_player_object_id: Option<u64>,
    pub remote_players: HashMap<PlayerId, ClientRemotePlayerState>,
    pub container_slots: HashMap<u64, Vec<Option<InventoryStack>>>,
    pub world_objects: HashMap<u64, ClientWorldObjectState>,
}
