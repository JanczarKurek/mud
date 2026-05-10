use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::player::classes::Class;
use crate::player::components::{AttributeSet, ChatLog, Inventory, InventoryStack, PlayerId};
use crate::player::progression::ExperienceView;
use crate::world::components::{SpaceId, SpacePosition, TilePosition};
use crate::world::direction::Direction;
use crate::world::floor_definitions::FloorTypeId;
use crate::world::floor_map::FloorMap;
use crate::world::map_layout::SpaceLightingDef;

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
    /// Display a single line of dialog and wait for the player to click
    /// "Continue" (which sends `GameCommand::DialogAdvance`).
    DialogLine {
        session_id: u64,
        speaker: Option<String>,
        text: String,
    },
    /// Display a set of selectable dialog options. The player picks one by
    /// sending `GameCommand::DialogChoose { option_idx }`.
    DialogOptions {
        session_id: u64,
        options: Vec<String>,
    },
    /// The dialog panel should be closed (dialogue completed or aborted).
    DialogClose {
        session_id: u64,
    },
    /// The local player just leveled up — show a transient overlay toast.
    LevelUpToast {
        new_level: u32,
    },
    /// Post-death recap dialog: lists what dropped on the corpse and how
    /// much XP was zeroed by the death penalty.
    DeathSummary {
        items_dropped: Vec<InventoryStackSummary>,
        xp_lost: u64,
    },
    /// A trade session has just opened for this peer — spawn the trade panel.
    /// The actual trade contents arrive via `GameEvent::TradeStateChanged`.
    OpenTradePanel {
        session_id: crate::game::trade::TradeSessionId,
    },
    /// The trade session has ended. The client closes the panel and surfaces
    /// the outcome (completed/cancelled/etc.) to the user.
    CloseTradePanel {
        session_id: crate::game::trade::TradeSessionId,
        outcome: crate::game::trade::TradeOutcome,
    },
}

/// Tiny self-contained snapshot of a dropped stack for the DeathSummary
/// recap. Distinct from `InventoryStack` so the summary can survive
/// definition lookups going stale and serialize cheaply.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct InventoryStackSummary {
    pub type_id: String,
    pub display_name: String,
    pub quantity: u32,
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

/// Snapshot of an active food/drink regen buff replicated to the client. The
/// HUD renders this as a small "Well Fed: M:SS" badge near the HP/MP bars.
/// `None` on `ClientGameState::regen_buff` means no active buff.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RegenBuffState {
    pub multiplier: f32,
    pub remaining_seconds: f32,
}

/// Replicated snapshot of the local player's carry weight. The HUD renders
/// it as `current/soft kg` next to the inventory; the encumbered flag drives
/// a "🐢" icon and the slow-walk visual. The server diffs at 0.05 kg
/// resolution to avoid wire spam.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ClientCarryWeight {
    pub current_kg: f32,
    pub soft_cap_kg: f32,
    pub hard_cap_kg: f32,
    pub encumbered: bool,
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
    #[serde(default)]
    pub is_rotatable: bool,
    pub quantity: u32,
    pub has_dialog: bool,
    #[serde(default)]
    pub facing: Direction,
    /// Current discrete-state name for objects whose definition declares
    /// `states:` (e.g. "open" / "closed"). `None` for stateless objects.
    #[serde(default)]
    pub state: Option<String>,
    /// True when this object is a merchant NPC. Drives the "Trade" /
    /// "Browse Wares" entry on the right-click context menu and the
    /// `InitiateTrade { Shopkeeper(_) }` command path.
    #[serde(default)]
    pub is_shopkeeper: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientRemotePlayerState {
    pub player_id: PlayerId,
    pub object_id: u64,
    pub position: SpacePosition,
    pub tile_position: TilePosition,
    pub vitals: ClientVitalStats,
    #[serde(default)]
    pub facing: Direction,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientSpaceState {
    pub space_id: SpaceId,
    pub authored_id: String,
    pub width: i32,
    pub height: i32,
    pub fill_floor_type: FloorTypeId,
    #[serde(default)]
    pub lighting: SpaceLightingDef,
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
        #[serde(default)]
        facing: Direction,
    },
    CurrentSpaceChanged {
        space: ClientSpaceState,
    },
    PlayerVitalsChanged {
        vitals: ClientVitalStats,
    },
    /// Active regen buff state for the local player (`None` clears it).
    /// Replication parity for `RegenBuffs`; the HUD shows remaining time.
    PlayerRegenBuffChanged {
        buff: Option<RegenBuffState>,
    },
    PlayerStorageChanged {
        storage_slots: usize,
    },
    PlayerCarryWeightChanged {
        carry: ClientCarryWeight,
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
    /// Full-grid replacement for the floor map at (space, z). Sent on space
    /// switch / initial sync.
    FloorMapReplaced {
        space_id: SpaceId,
        z: i32,
        width: i32,
        height: i32,
        tiles: Vec<Option<FloorTypeId>>,
    },
    /// Single-tile floor change. Sent for editor edits and runtime spell effects.
    FloorTileSet {
        space_id: SpaceId,
        z: i32,
        x: i32,
        y: i32,
        floor_type: Option<FloorTypeId>,
    },
    /// Server-authoritative world clock advance. `time_of_day ∈ [0, 1)`.
    /// Emitted when the value moves by more than ~0.001 (≈ 1.2 in-game
    /// seconds at a 20-minute day) or after a 10s heartbeat.
    WorldTimeChanged {
        time_of_day: f32,
    },
    /// Baseline / corrective replication of the local player's
    /// `Experience`. Emitted on first projection and whenever the projected
    /// view diverges from the peer's last-seen baseline (e.g. after death's
    /// XP-zero rule fires). `ExperienceGained` / `LevelUp` /
    /// `ExperienceLost` carry the deltas; this variant carries truth.
    PlayerExperienceChanged {
        experience: ExperienceView,
    },
    /// Delta event: amount of XP added by the most recent grant. Useful for
    /// chat-log and floating-text feedback.
    ExperienceGained {
        amount: u64,
    },
    /// Delta event: the local player crossed into `new_level`.
    LevelUp {
        new_level: u32,
    },
    /// Delta event: amount of XP removed by the death penalty.
    ExperienceLost {
        amount: u64,
    },
    /// Replicated when the local player's selected class changes (or is first
    /// projected). Driven by the `ChooseClass` command + bootstrap diff.
    PlayerClassChanged {
        class: Class,
    },
    /// Replicated when the player's `class_chosen` flag flips (typically once,
    /// the first time they pick from the class picker). Drives the UI to hide
    /// the picker overlay.
    PlayerClassChosenChanged {
        chosen: bool,
    },
    /// Replicates the *effective* attribute set (base + equipment bonuses)
    /// for the local player. Drives the Character sheet's attributes grid;
    /// fired when `DerivedStats.attributes` changes between projection ticks.
    PlayerAttributesChanged {
        attributes: AttributeSet,
    },
    /// Replicates the local player's currently active trade session
    /// (or `None` when the player has no active trade). Sole authority for
    /// the trade panel's contents — the projection diffs the snapshot and
    /// emits this whenever any trade-related field changes.
    TradeStateChanged {
        state: Option<crate::game::trade::ClientTradeView>,
    },
}

#[derive(Resource, Default)]
pub struct PendingGameEvents {
    pub events: Vec<GameEvent>,
}

/// Tracks which players currently have a container's panel open. Drives the
/// derived "open" / "closed" visual state for chests and other containers
/// that pair `container_capacity` with a stateful `iron_chest`-style
/// definition. Transient — never persisted.
#[derive(Resource, Default)]
pub struct ContainerViewers {
    viewers: HashMap<u64, HashSet<PlayerId>>,
}

impl ContainerViewers {
    /// Insert `(object_id, player)`. Returns `true` if this is the first
    /// viewer (caller flips the visual to "open").
    pub fn insert(&mut self, object_id: u64, player: PlayerId) -> bool {
        let entry = self.viewers.entry(object_id).or_default();
        let first = entry.is_empty();
        entry.insert(player);
        first
    }

    /// Remove `(object_id, player)`. Returns `true` if this was the last
    /// viewer (caller flips the visual back to "closed").
    pub fn remove(&mut self, object_id: u64, player: PlayerId) -> bool {
        let Some(entry) = self.viewers.get_mut(&object_id) else {
            return false;
        };
        let removed = entry.remove(&player);
        let now_empty = entry.is_empty();
        if now_empty {
            self.viewers.remove(&object_id);
        }
        removed && now_empty
    }

    /// Drop all entries for a given player (used on disconnect). Returns the
    /// list of object ids that just lost their last viewer.
    pub fn drop_player(&mut self, player: PlayerId) -> Vec<u64> {
        let mut emptied = Vec::new();
        self.viewers.retain(|object_id, viewers| {
            if viewers.remove(&player) && viewers.is_empty() {
                emptied.push(*object_id);
                return false;
            }
            !viewers.is_empty()
        });
        emptied
    }

    /// Whether any player is currently viewing the given container.
    pub fn has_viewers(&self, object_id: u64) -> bool {
        self.viewers
            .get(&object_id)
            .is_some_and(|set| !set.is_empty())
    }
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
    pub player_facing: Option<Direction>,
    /// Mirror of authoritative FloorMaps; populated by FloorMapReplaced events.
    pub floor_maps: HashMap<(SpaceId, i32), FloorMap>,
    /// Server-replicated world clock in [0, 1). 0.5 = noon. Defaults to 0.0
    /// (midnight) on bootstrap; the very first projection tick emits a
    /// `WorldTimeChanged` event that fixes the value before lighting reads it.
    #[serde(default)]
    pub world_time: f32,
    /// Active food/drink regen buff for the local player, or `None` when no
    /// buff is active. Driven by `PlayerRegenBuffChanged` events; the HUD
    /// renders the remaining time near the HP/MP bars.
    #[serde(default)]
    pub regen_buff: Option<RegenBuffState>,
    /// Replicated carry-weight snapshot for the local player. `None` until
    /// the first `PlayerCarryWeightChanged` event arrives — typically on the
    /// first frame the player exists.
    #[serde(default)]
    pub carry_weight: Option<ClientCarryWeight>,
    /// Replicated XP / level snapshot for the local player. `None` until the
    /// first `PlayerExperienceChanged` event lands.
    #[serde(default)]
    pub experience: Option<ExperienceView>,
    /// Replicated class for the local player. `None` until the first
    /// `PlayerClassChanged` event lands.
    #[serde(default)]
    pub class: Option<Class>,
    /// Replicated `class_chosen` flag for the local player. UI picker shows
    /// when `class_chosen == false` (and `local_player_id` is `Some`). On
    /// bootstrap defaults to `false` so the very first frame doesn't flash
    /// the picker for already-chosen characters — the projection
    /// immediately corrects it.
    #[serde(default)]
    pub class_chosen: bool,
    /// Replicated effective attribute set (base + equipment) for the local
    /// player. `None` until the first `PlayerAttributesChanged` event lands.
    #[serde(default)]
    pub attributes: Option<AttributeSet>,
    /// Snapshot of the local player's active trade, or `None`. Updated by
    /// `GameEvent::TradeStateChanged`; the trade panel reads from this.
    #[serde(default)]
    pub current_trade: Option<crate::game::trade::ClientTradeView>,
}
