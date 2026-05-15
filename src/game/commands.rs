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

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ItemSlotRef {
    Backpack(usize),
    Equipment(EquipmentSlot),
    Container {
        object_id: u64,
        slot_index: usize,
    },
    /// A sub-slot inside a pouch that lives in the player's backpack at
    /// `backpack_slot`. Pouches carry their contents as
    /// `InventoryStack::contained_slots` so this ref does not need a runtime
    /// `object_id`. The recursion guard ensures the parent pouch's
    /// definition has `accepts_storable_containers: false`, so we never
    /// have to address a pouch-inside-a-pouch.
    PouchInBackpack {
        backpack_slot: usize,
        sub_slot: usize,
    },
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
    /// Save the player's current `(space, tile)` as their respawn point.
    /// Future deaths return them to this location instead of map center.
    /// Persisted on the next autosave (or immediately if an account DB is
    /// attached).
    SetHome,
    /// Open a trade window with a target (another player or, in later phases,
    /// a shopkeeper NPC). Server validates adjacency and routes
    /// `OpenTradePanel` UI events to both sides.
    InitiateTrade {
        target: crate::game::trade::TradeTarget,
    },
    /// Add (or merge into an existing entry of) an item from one of the
    /// acting player's personal slots into the trade's "us" column.
    /// Auto-resets both sides' Ready/Confirm flags.
    OfferTradeItem {
        session_id: crate::game::trade::TradeSessionId,
        source: ItemSlotRef,
        quantity: u32,
    },
    /// Remove the offer at `offer_index` from the acting player's "us" column.
    /// Auto-resets both sides' Ready/Confirm flags.
    WithdrawTradeItem {
        session_id: crate::game::trade::TradeSessionId,
        offer_index: usize,
    },
    /// Toggle the acting side's Ready flag. When both sides Ready, the panel
    /// is "locked" — items can still be modified but doing so clears Ready.
    ToggleTradeReady {
        session_id: crate::game::trade::TradeSessionId,
    },
    /// Set the acting side's Confirm flag. Once both sides have Ready+Confirm,
    /// the trade commits transactionally.
    ConfirmTrade {
        session_id: crate::game::trade::TradeSessionId,
    },
    /// Abort the trade. Both panels close with outcome `Cancelled`.
    CancelTrade {
        session_id: crate::game::trade::TradeSessionId,
    },
    /// Shop-trade only: add `quantity` of the ware at `ware_index` to the
    /// THEY column. The server auto-balances the buyer's coin payment by
    /// adding the cheapest sufficient mix of copper/silver/gold from the
    /// buyer's inventory into the US column. Rejects if the buyer has
    /// insufficient funds or the ware is out of stock.
    BrowseShopBuy {
        session_id: crate::game::trade::TradeSessionId,
        ware_index: usize,
        quantity: u32,
    },
    /// Write or delete a key in the acting player's `CharacterStash`.
    /// `value = Some(json)` upserts; `value = None` deletes. Drained by
    /// `process_stash_commands` (CraftingServerPlugin) in `CommandIntercept`.
    ///
    /// Sources: Python `world.stash_set/delete`, Yarn `<<stash_set>>`, and
    /// future server-side systems. Like `AdminSpawn` and friends, this is
    /// currently not gated at the wire boundary — a malicious TCP client
    /// could push it directly. Wire-side gating of admin/internal commands
    /// is a separate issue tracked outside this feature.
    StashMutate {
        key: String,
        value: Option<serde_json::Value>,
    },
    /// Grant a recipe to the acting player's `recipes:known` set.
    /// Idempotent — re-granting a known recipe emits no events. Sources:
    /// Yarn `<<give_recipe>>`, `UseItem` on a scroll with
    /// `learns_recipe: <id>`, auto-learn on level/class match.
    LearnRecipe {
        recipe_id: String,
    },
    /// Craft `recipe_id` for the acting player. Server validates: player
    /// knows the recipe, has all inputs in their backpack, and (when
    /// `station` is set) is adjacent to a matching world object. On success
    /// the inputs are consumed and outputs granted; the player's chat log
    /// gets a narrator line and a `GameEvent::ItemCrafted` is emitted.
    CraftItem {
        recipe_id: String,
    },
    /// Player typed a chat message. Server trims the text, validates length
    /// (1..=`chat::CHAT_MAX_LEN`), and pushes a formatted `"[name]: text"`
    /// line into every player's `ChatLog` whose `SpaceResident` matches the
    /// speaker and is within `chat::CHAT_RADIUS_TILES` Chebyshev distance —
    /// the speaker included.
    Say {
        text: String,
    },
    /// Create or replace a log entry for the acting player. Drained by
    /// `process_log_commands` (LogServerPlugin) in `CommandIntercept`.
    ///
    /// Validation: server enforces length caps and owner-gating. Player
    /// writes with `owner: Engine` are rejected. Engine writes preserve
    /// any existing `player_notes` on the targeted entry.
    UpsertLogEntry {
        section: String,
        subsection: String,
        title: String,
        body: String,
        owner: crate::log::LogOwner,
    },
    /// Remove a log entry. Server rejects deletes targeting engine-owned
    /// entries.
    DeleteLogEntry {
        section: String,
        subsection: String,
    },
    /// Update the player-editable notes tail under a quest entry. Only the
    /// `player_notes` field is mutated; the engine-owned body is untouched.
    /// Rejected when the targeted quest entry does not exist.
    SetQuestPlayerNotes {
        quest_name: String,
        text: String,
    },
    /// Spend skill points on `skill`, raising its rank by up to `ranks` (the
    /// handler buys ranks one at a time, stopping at the class/level cap or
    /// when points run out). Drained by `process_allocate_skill_commands` in
    /// `CommandIntercept` before `process_game_commands` runs.
    AllocateSkillPoint {
        skill: crate::player::skills::Skill,
        ranks: u8,
    },
}
