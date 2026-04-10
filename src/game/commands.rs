use bevy::prelude::*;

use crate::world::components::TilePosition;
use crate::world::object_definitions::EquipmentSlot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemSlotRef {
    Backpack(usize),
    Equipment(EquipmentSlot),
    Container { object_id: u64, slot_index: usize },
}

#[derive(Clone, Copy, Debug)]
pub enum ItemReference {
    WorldObject(u64),
    Slot(ItemSlotRef),
}

#[derive(Clone, Copy, Debug)]
pub enum ItemDestination {
    Slot(ItemSlotRef),
    WorldTile(TilePosition),
}

#[derive(Clone, Copy, Debug)]
pub enum UseTarget {
    Player,
    Object(u64),
}

#[derive(Clone, Copy, Debug)]
pub enum InspectTarget {
    Object(u64),
}

#[derive(Clone, Debug)]
pub enum GameCommand {
    MovePlayer { delta: IVec2 },
    SetCombatTarget { target_object_id: Option<u64> },
    OpenContainer { object_id: u64 },
    Inspect { target: InspectTarget },
    UseItem { source: ItemReference },
    UseItemOn { source: ItemReference, target: UseTarget },
    CastSpellAt {
        source: ItemReference,
        spell_id: String,
        target_object_id: u64,
    },
    MoveItem {
        source: ItemReference,
        destination: ItemDestination,
    },
    AdminSpawn {
        type_id: String,
        tile_position: TilePosition,
    },
}
