use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::player::classes::Class;
use crate::player::components::AttributeKind;
use crate::player::skills::Skill;
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
    /// One of the using player's own inventory/equipment items. Used to apply
    /// a consumable's `grants_item_modifier` enchantment to a chosen item
    /// (e.g. coating a weapon with a poison flask).
    ItemSlot(ItemSlotRef),
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
        /// Player consents to an Athletics climb if the step would resolve
        /// as `dz_climbed > CLIMB_FREE_DZ`. Driven by SHIFT in the keyboard
        /// input handler. Without this, only flat steps, the 1-half-block
        /// auto-step, and falls are legal — taller ledges silently block.
        climb: bool,
    },
    /// Athletic jump to a tile within `JUMP_MAX_RANGE`. Server validates
    /// same-space + range, rolls an Athletics check whose DC scales with
    /// `ceil(hypot(dx, dy)) + dz_up_half_blocks` (Euclidean XY), and either
    /// teleports the player to the landing column or lands them short along
    /// the line. Either path may trigger fall damage on the landing dz.
    JumpTo {
        target_tile: TilePosition,
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
    /// Player-invoked Hide on a nearby world object whose definition has a
    /// `can_hide:` block. Server runs a Stealth check (DC 10, the item's
    /// `sneakiness` as situational bonus). On success, inserts the `Hidden`
    /// component with `dc = total/2` and seeds the placer into `detected_by`
    /// so they keep seeing the object. On failure, emits a narrator line
    /// and leaves the object visible. Drained by `process_hide_commands`
    /// in `CommandIntercept`.
    HideObject {
        object_id: u64,
    },
    /// Server-internal command pushed by `handle_use_item_on` when a tool was
    /// used on a target with a matching `tool_gate` interaction. Runs the same
    /// interaction pipeline as `InteractWithObject` (skill_gate, state
    /// transition, grants_items, respawn, side_effects) but bypasses the
    /// tool_gate check — `handle_use_item_on` already validated the tool was
    /// present and consumed its charge. Never emitted by clients.
    ApplyToolInteraction {
        target_object_id: u64,
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
    /// Tile-target cast. Used by `SpellTargeting::TargetedTile` spells
    /// (firewall, fireball). The server validates that the spell exists,
    /// is in range (`chebyshev_distance(caster_tile, target_tile)`), and
    /// then resolves AoE damage / pattern-spawn effects centered on
    /// `target_tile`.
    CastSpellAtTile {
        source: ItemReference,
        spell_id: String,
        target_tile: TilePosition,
    },
    /// Item-target cast. Used by `SpellTargeting::TargetedItem` spells
    /// (weapon enchants). The server validates the spell, then applies its
    /// `effects.enchant_item` modifier to the item at `target` via the
    /// TYPE_EX/LVL anti-stack rule.
    CastSpellAtItem {
        source: ItemReference,
        spell_id: String,
        target: ItemSlotRef,
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
        skill: Skill,
        ranks: u8,
    },
    /// Admin-only: grant raw XP. Pushed into `PendingXpGrants` so the canonical
    /// `apply_xp_grants` pipeline picks it up — level-ups, skill-point grants,
    /// and HUD toasts fire normally. Drained by
    /// `process_admin_progression_commands` in `CommandIntercept`.
    AdminGrantXp {
        amount: u64,
    },
    /// Admin-only: hard-set the target player's level. Sets `current_xp` to
    /// `xp_for_level(level)` and grants skill points for every level crossed
    /// upward. Downward changes do not refund anything.
    AdminSetLevel {
        level: u32,
    },
    /// Admin-only: increase `SkillSheet.available_points` by `amount` without
    /// requiring a level-up.
    AdminGrantSkillPoints {
        amount: u32,
    },
    /// Admin-only: overwrite a single skill's rank, bypassing the
    /// class/level cap and point cost.
    AdminSetSkillRank {
        skill: Skill,
        rank: u8,
    },
    /// Admin-only: overwrite a single attribute on `BaseStats.attributes`.
    /// Bypasses the [8,18] point-buy clamp; the next frame's
    /// `refresh_derived_player_stats` recomputes `DerivedStats` and reclamps
    /// `VitalStats` accordingly.
    AdminSetAttribute {
        kind: AttributeKind,
        value: i32,
    },
    /// Admin-only: switch the target's `Class`. Does not redistribute skill
    /// ranks — the admin is expected to clean those up explicitly.
    AdminSetClass {
        class: Class,
    },
    /// Admin-only: restore health and mana to their respective maxes.
    AdminFullHeal,
    /// Player-invoked Read on a book/tombstone/inscription. Server validates
    /// adjacency (world) or ownership (inventory), then emits an
    /// `OpenBookPanel` UI event to that peer with the captured text snapshot.
    ReadBook {
        source: ItemReference,
    },
    /// Player-invoked Write on a book. Requires a pen in inventory (unless
    /// the book is itself in the actor's inventory — in which case the pen
    /// must still be present; the gating is symmetric). Server clamps title
    /// to 64 chars, body to 4096, strips control chars, and writes
    /// `properties["title"]`, `properties["text"]`, `properties["author_name"]`.
    WriteBook {
        source: ItemReference,
        title: String,
        text: String,
    },
    /// Player-invoked Engrave on an `engravable: true` item. Requires a pen
    /// in inventory and rejects re-engraving an already-inscribed item. Sets
    /// `properties["inscription"]` (clamped to 32 chars).
    Engrave {
        source: ItemReference,
        inscription: String,
    },
}
