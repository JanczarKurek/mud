use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::world::object_definitions::EquipmentSlot;

#[derive(Clone, Debug, Deserialize, PartialEq, Resource, Serialize)]
pub struct InventoryState {
    pub backpack_slots: Vec<Option<u64>>,
    pub equipment_slots: Vec<(EquipmentSlot, Option<u64>)>,
}

impl Default for InventoryState {
    fn default() -> Self {
        Self {
            backpack_slots: vec![None; 16],
            equipment_slots: EquipmentSlot::ALL
                .into_iter()
                .map(|slot| (slot, None))
                .collect(),
        }
    }
}

impl InventoryState {
    pub fn equipment_item(&self, slot: EquipmentSlot) -> Option<u64> {
        self.equipment_slots.iter().find_map(|(equipment_slot, item)| {
            if *equipment_slot == slot {
                *item
            } else {
                None
            }
        })
    }

    pub fn take_equipment_item(&mut self, slot: EquipmentSlot) -> Option<u64> {
        self.equipment_slots
            .iter_mut()
            .find_map(|(equipment_slot, item)| {
                if *equipment_slot == slot {
                    item.take()
                } else {
                    None
                }
            })
    }

    pub fn place_equipment_item(&mut self, slot: EquipmentSlot, object_id: u64) -> bool {
        for (equipment_slot, item) in &mut self.equipment_slots {
            if *equipment_slot != slot {
                continue;
            }

            if item.is_some() {
                return false;
            }

            *item = Some(object_id);
            return true;
        }

        false
    }

    pub fn restore_equipment_item(&mut self, slot: EquipmentSlot, object_id: u64) {
        for (equipment_slot, item) in &mut self.equipment_slots {
            if *equipment_slot == slot {
                *item = Some(object_id);
                return;
            }
        }
    }
}

#[derive(Clone, Resource)]
pub struct ChatLogState {
    pub lines: Vec<String>,
    pub max_lines: usize,
}

impl Default for ChatLogState {
    fn default() -> Self {
        Self {
            lines: vec![
                "[Narrator]: Right-click an item to inspect it.".to_owned(),
                "[Narrator]: Right-click a nearby barrel to open it.".to_owned(),
            ],
            max_lines: 8,
        }
    }
}

impl ChatLogState {
    pub fn push_line(&mut self, message: impl Into<String>) {
        self.lines.push(message.into());
        if self.lines.len() > self.max_lines {
            let overflow = self.lines.len() - self.max_lines;
            self.lines.drain(0..overflow);
        }
    }

    pub fn push_narrator(&mut self, message: impl Into<String>) {
        self.push_line(format!("[Narrator]: {}", message.into()));
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum GameUiEvent {
    OpenContainer { object_id: u64 },
}

#[derive(Resource, Default)]
pub struct PendingGameCommands {
    pub commands: Vec<GameCommand>,
}

impl PendingGameCommands {
    pub fn push(&mut self, command: GameCommand) {
        self.commands.push(command);
    }
}

#[derive(Resource, Default)]
pub struct PendingGameUiEvents {
    pub events: Vec<GameUiEvent>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ClientVitalStats {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientWorldObjectState {
    pub object_id: u64,
    pub definition_id: String,
    pub tile_position: crate::world::components::TilePosition,
    pub is_container: bool,
    pub is_npc: bool,
    pub is_movable: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum GameEvent {
    InventoryChanged { inventory: InventoryState },
    ChatLogChanged { lines: Vec<String> },
    PlayerPositionChanged {
        tile_position: crate::world::components::TilePosition,
    },
    PlayerVitalsChanged { vitals: ClientVitalStats },
    PlayerStorageChanged { storage_slots: usize },
    CombatTargetChanged { target_object_id: Option<u64> },
    ContainerChanged { object_id: u64, slots: Vec<Option<u64>> },
    ContainerRemoved { object_id: u64 },
    WorldObjectUpserted { object: ClientWorldObjectState },
    WorldObjectRemoved { object_id: u64 },
}

#[derive(Resource, Default)]
pub struct PendingGameEvents {
    pub events: Vec<GameEvent>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Resource, Serialize)]
pub struct ClientGameState {
    pub inventory: InventoryState,
    pub chat_log_lines: Vec<String>,
    pub player_tile_position: Option<crate::world::components::TilePosition>,
    pub player_vitals: Option<ClientVitalStats>,
    pub player_storage_slots: usize,
    pub current_target_object_id: Option<u64>,
    pub container_slots: HashMap<u64, Vec<Option<u64>>>,
    pub world_objects: HashMap<u64, ClientWorldObjectState>,
}
