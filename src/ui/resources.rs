use bevy::prelude::*;

use crate::ui::components::ItemSlotKind;
use crate::world::components::TilePosition;
use crate::world::object_definitions::EquipmentSlot;

#[derive(Resource)]
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
        self.equipment_slots.iter().find_map(
            |(equipment_slot, item)| {
                if *equipment_slot == slot {
                    *item
                } else {
                    None
                }
            },
        )
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

#[derive(Resource)]
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

#[derive(Clone, Copy)]
pub enum ContextMenuTarget {
    World(Entity, u64),
    Slot(ItemSlotKind, u64),
}

#[derive(Resource, Default)]
pub struct ContextMenuState {
    pub target: Option<ContextMenuTarget>,
    pub position: Vec2,
    pub can_open: bool,
    pub can_use: bool,
    pub can_attack: bool,
}

impl ContextMenuState {
    pub fn show(
        &mut self,
        position: Vec2,
        target: ContextMenuTarget,
        can_open: bool,
        can_use: bool,
        can_attack: bool,
    ) {
        self.position = position;
        self.target = Some(target);
        self.can_open = can_open;
        self.can_use = can_use;
        self.can_attack = can_attack;
    }

    pub fn hide(&mut self) {
        self.target = None;
        self.can_open = false;
        self.can_use = false;
        self.can_attack = false;
    }

    pub fn is_visible(&self) -> bool {
        self.target.is_some()
    }
}

pub enum DragSource {
    World(Entity),
    UiSlot(ItemSlotKind),
}

#[derive(Resource, Default)]
pub struct OpenContainerState {
    pub entity: Option<Entity>,
}

#[derive(Resource, Default)]
pub struct DragState {
    pub source: Option<DragSource>,
    pub object_id: Option<u64>,
    pub world_origin: Option<TilePosition>,
}

#[derive(Resource, Default)]
pub struct UseOnState {
    pub source: Option<ContextMenuTarget>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorMode {
    #[default]
    Default,
    UseOn,
}

impl CursorMode {}

#[derive(Resource, Default)]
pub struct CursorState {
    pub mode: CursorMode,
}
