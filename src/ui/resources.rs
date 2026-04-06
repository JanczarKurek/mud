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
            backpack_slots: vec![None; 8],
            equipment_slots: EquipmentSlot::ALL
                .into_iter()
                .map(|slot| (slot, None))
                .collect(),
        }
    }
}

impl InventoryState {
    pub fn equipment_item(&self, slot: EquipmentSlot) -> Option<u64> {
        self.equipment_slots
            .iter()
            .find_map(|(equipment_slot, item)| (*equipment_slot == slot).then_some(*item))
            .flatten()
    }

    pub fn take_equipment_item(&mut self, slot: EquipmentSlot) -> Option<u64> {
        self.equipment_slots
            .iter_mut()
            .find_map(|(equipment_slot, item)| (*equipment_slot == slot).then_some(item.take()))
            .flatten()
    }

    pub fn place_equipment_item(&mut self, slot: EquipmentSlot, object_id: u64) -> bool {
        let Some(item) = self
            .equipment_slots
            .iter_mut()
            .find_map(|(equipment_slot, item)| (*equipment_slot == slot).then_some(item))
        else {
            return false;
        };

        if item.is_some() {
            return false;
        }

        *item = Some(object_id);
        true
    }

    pub fn restore_equipment_item(&mut self, slot: EquipmentSlot, object_id: u64) {
        if let Some(item) = self
            .equipment_slots
            .iter_mut()
            .find_map(|(equipment_slot, item)| (*equipment_slot == slot).then_some(item))
        {
            *item = Some(object_id);
        }
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
