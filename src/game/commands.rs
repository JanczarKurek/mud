use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::components::{SpaceId, TilePosition};
use crate::world::direction::Direction;
use crate::world::floor_definitions::FloorTypeId;
use crate::world::object_definitions::EquipmentSlot;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RotationDirection {
    Clockwise,
    CounterClockwise,
}

impl RotationDirection {
    pub fn apply(self, direction: Direction) -> Direction {
        match self {
            Self::Clockwise => direction.turn_clockwise(),
            Self::CounterClockwise => direction.turn_counter_clockwise(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MoveDelta {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ItemSlotRef {
    Backpack(usize),
    Equipment(EquipmentSlot),
    Container { object_id: u64, slot_index: usize },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum ItemReference {
    WorldObject(u64),
    Slot(ItemSlotRef),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum ItemDestination {
    Slot(ItemSlotRef),
    WorldTile(TilePosition),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum UseTarget {
    Player,
    Object(u64),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum InspectTarget {
    /// A world object — quantity is looked up from ObjectRegistry.
    Object(u64),
    /// An inventory/container slot — quantity is read from the InventoryStack.
    SlotItem(ItemSlotRef),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum GameCommand {
    MovePlayer {
        delta: MoveDelta,
    },
    /// Rotate a nearby world object that has the `Rotatable` component.
    /// Server validates adjacency + rotatable flag, then advances the object's
    /// `Facing` one 90° step in the requested direction. The resulting facing
    /// change replicates through the existing `WorldObjectUpserted` diff.
    RotateObject {
        object_id: u64,
        rotation: RotationDirection,
    },
    SetCombatTarget {
        target_object_id: Option<u64>,
    },
    OpenContainer {
        object_id: u64,
    },
    Inspect {
        target: InspectTarget,
    },
    UseItem {
        source: ItemReference,
    },
    UseItemOn {
        source: ItemReference,
        target: UseTarget,
    },
    CastSpellAt {
        source: ItemReference,
        spell_id: String,
        target_object_id: u64,
    },
    MoveItem {
        source: ItemReference,
        destination: ItemDestination,
    },
    TakeFromStack {
        source: ItemReference,
        amount: u32,
        destination: ItemDestination,
    },
    AdminSpawn {
        type_id: String,
        tile_position: TilePosition,
    },
    /// Open a dialog with the given NPC. Server looks up the NPC's
    /// `DialogNode`, starts a Yarn runner, and replies with `DialogLine` or
    /// `DialogOptions` UI events.
    TalkToNpc {
        npc_object_id: u64,
    },
    /// Advance past a line currently displayed in the dialog panel
    /// (client clicked "Continue").
    DialogAdvance {
        session_id: u64,
    },
    /// Pick one of the currently displayed dialog options by index into the
    /// `Vec<String>` most recently sent via `DialogOptions`.
    DialogChoose {
        session_id: u64,
        option_idx: usize,
    },
    /// Abort a running dialog (player closed the panel).
    DialogEnd {
        session_id: u64,
    },
    /// Grant `count` instances of `type_id` to the acting player's backpack.
    /// Stackable definitions merge into existing stacks; otherwise each copy
    /// consumes an empty slot. Grants that don't fit are silently dropped —
    /// callers are expected to gate on inventory space when that matters.
    GiveItem {
        type_id: String,
        count: u32,
    },
    /// Remove up to `count` instances of `type_id` from the acting player's
    /// backpack. Used by Yarn `<<take_item>>` for fetch-quest turn-in.
    TakeItem {
        type_id: String,
        count: u32,
    },
    /// Set (or clear) the floor type at a single tile of a space's floor map.
    /// Authoritative path for runtime edits (editor brush, future spell effects).
    EditorSetFloorTile {
        space_id: SpaceId,
        z: i32,
        x: i32,
        y: i32,
        floor_type: Option<FloorTypeId>,
    },
}
