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
    /// Closing a container panel. Counterpart to `OpenContainer`; the server
    /// removes the player from `ContainerViewers` and, when the viewer set
    /// becomes empty, flips the object's state back to "closed" (chests).
    CloseContainer {
        object_id: u64,
    },
    /// Player-invoked verb on a nearby stateful world object (e.g. "open" on
    /// a closed door, "light" on an unlit torch). Server validates adjacency,
    /// looks up the matching `ObjectInteractionDef` in the object's
    /// definition, applies the transition, and runs declared side-effects.
    InteractWithObject {
        object_id: u64,
        verb: String,
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
    /// Move the acting player (or `player_id` if specified) to a tile.
    /// `space_id` of `None` means "current space"; non-None requires the
    /// space to already exist in `SpaceManager`.
    AdminTeleport {
        space_id: Option<SpaceId>,
        tile_position: TilePosition,
    },
    /// Despawn a world object by id. The next projection tick replicates the
    /// removal via `WorldObjectRemoved`.
    AdminDespawn {
        object_id: u64,
    },
    /// Override the acting player's health and/or mana directly. Each `Some`
    /// value clamps into [0, max]. The next projection tick emits a
    /// `PlayerVitalsChanged` event.
    AdminSetVitals {
        health: Option<f32>,
        mana: Option<f32>,
    },
    /// Force a discrete-state change on a stateful world object — the same
    /// path that `InteractWithObject` uses internally, but bypassing the
    /// definition's `interactions` whitelist. Useful for scripts that need
    /// to set up scene state without triggering a player verb.
    AdminSetObjectState {
        object_id: u64,
        state: String,
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
