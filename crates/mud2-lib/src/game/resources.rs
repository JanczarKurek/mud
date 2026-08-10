use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::game::commands::GameCommand;
use crate::player::classes::Class;
use crate::player::components::{
    AttributeSet, ChatLog, Inventory, InventoryStack, PlayerAppearance, PlayerId,
};
use crate::player::progression::ExperienceView;
use crate::world::components::{SpaceId, SpacePosition, TilePosition};
use crate::world::direction::Direction;
use crate::world::floor_definitions::FloorTypeId;
use crate::world::floor_map::FloorMap;
use crate::world::map_layout::SpaceLightingDef;
use bevy::math::Vec2;

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
        /// Flight time in seconds. The client lerps the missile from→to over
        /// this duration (matching the server's scheduled-impact delay for
        /// spell missiles; ranged attacks pass a fixed default).
        duration_seconds: f32,
        /// When set, the missile homes — the client re-reads this object's
        /// current view position each frame as the flight endpoint. `None`
        /// flies to the fixed `to_tile` (ranged attacks, tile-target spells).
        target_object_id: Option<u64>,
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
    /// One-shot visual effect spawn. Looked up by `definition_id` in the
    /// client's `VfxDefinitions` resource; missing ids are skipped silently.
    /// The substrate underlying hit/cast/impact/death animations.
    VfxSpawn {
        definition_id: String,
        anchor: VfxAnchor,
    },
    /// Transient overlay shown when the local player learns a recipe.
    /// Carries the human-readable `recipe_name` for display; clients fall
    /// back to `recipe_id` when the name isn't in their local
    /// `RecipeDefinitions`.
    RecipeLearnedToast {
        recipe_id: String,
        recipe_name: String,
    },
    /// One-shot: ask the client to open the recipe-book panel. When
    /// `filter_station` is set the panel filters to recipes that require
    /// that station type — used by the right-click "Craft" verb on station
    /// objects.
    OpenRecipeBook {
        filter_station: Option<String>,
    },
    /// One-shot: ask the client to open the skills panel (e.g. from the
    /// HUD button or a future tutorial trigger).
    OpenSkillsPanel,
    /// Transient overlay: the local player just gained `amount` skill
    /// points (typically from a level-up). HUD shows a short toast.
    SkillPointsToast {
        amount: u32,
    },
    /// Transient overlay: the local player banked an ability bump (at level
    /// 4/8/12/16/20) and can raise an attribute. `available` is the new total.
    AbilityBumpAvailable {
        available: u32,
    },
    /// An attack missed the target via the dodge mechanic. Pure presentation
    /// signal — no state changed (attacker still consumed any ammo, target
    /// still owes no damage).
    AttackDodged {
        attacker_object_id: u64,
        target_object_id: u64,
    },
    /// A hostile NPC just spotted the local player (fresh aggro). One-shot cue
    /// for a "you've been seen!" toast/sound. `npc_object_id` is the spotter.
    Spotted {
        npc_object_id: u64,
    },
    /// An attack was partially mitigated by the target's shield. `amount` is
    /// the absorbed damage (post-roll), useful for floating-text feedback.
    AttackBlocked {
        attacker_object_id: u64,
        target_object_id: u64,
        amount: i32,
    },
    /// A landed attack was a critical hit (raw d20 ≥ the weapon's crit
    /// threshold): the damage expression was rolled twice. Pure presentation
    /// signal for a HUD flourish — the damage itself rides `DamageEvent`.
    AttackCrit {
        attacker_object_id: u64,
        target_object_id: u64,
    },
    /// Open the book/tombstone/inscription reader-editor panel on the client.
    /// Server emits this in response to `GameCommand::ReadBook` after
    /// validating reach + reading the target's `properties` snapshot. The
    /// payload carries the current text; subsequent changes ride on
    /// `WorldObjectUpserted` / `InventoryChanged` so the panel re-fetches
    /// when the user clicks "Read" again.
    OpenBookPanel {
        source: crate::game::commands::ItemReference,
        kind: crate::world::object_definitions::TextKind,
        title: String,
        text: String,
        author_name: Option<String>,
        can_edit: bool,
    },
    /// A short floating text bubble shown over a speaker for a few seconds.
    /// One-shot presentation signal — fires for player `/say`, NPC aggro
    /// barks, and ambient mutters. The client looks up `speaker_object_id`
    /// via `ClientGameState` and attaches the bubble via `AttachedToObject`.
    SpeechBubble {
        speaker_object_id: u64,
        text: String,
        style: SpeechBubbleStyle,
    },
    /// Server-side Python REPL response to `GameCommand::AdminExec` /
    /// `AdminReplReset`. `lines` is captured stdout; `error` a rendered
    /// traceback or rejection reason; `incomplete` means the input opened a
    /// multi-line block — the console shows a `...` continuation prompt and
    /// keeps buffering server-side.
    ReplOutput {
        lines: Vec<String>,
        error: Option<String>,
        incomplete: bool,
    },
    /// A party invitation arrived for this player. Opens the invite popup;
    /// the roster itself replicates separately via
    /// `GameEvent::PartyStateChanged` once the invite is accepted.
    PartyInviteReceived {
        from_player_id: crate::player::components::PlayerId,
        from_name: String,
        party_size: usize,
    },
    /// The pending invitation shown to this player is no longer actionable —
    /// answered, expired, withdrawn by disconnect, or the party filled up.
    /// Dismisses the invite popup.
    PartyInviteClosed,
}

/// Visual treatment for a floating speech bubble. Drives backdrop color and
/// (optionally) font weight on the client. Server picks based on the source:
/// `Say` for player chat, `Bark` for NPC aggro, `Mutter` for ambient.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SpeechBubbleStyle {
    Say,
    Bark,
    Mutter,
}

/// Anchor for a `VfxSpawn` event. `Tile` parks the effect at a static world
/// tile; `FollowObject` makes it track the named object's position each
/// frame so it stays attached to a moving target.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum VfxAnchor {
    Tile {
        space_id: SpaceId,
        tile: TilePosition,
    },
    FollowObject {
        object_id: u64,
        #[serde(default)]
        offset_pixels: [f32; 2],
    },
}

impl VfxAnchor {
    pub fn follow(object_id: u64) -> Self {
        Self::FollowObject {
            object_id,
            offset_pixels: [0.0, 0.0],
        }
    }

    pub fn follow_with_offset(object_id: u64, offset: Vec2) -> Self {
        Self::FollowObject {
            object_id,
            offset_pixels: [offset.x, offset.y],
        }
    }

    pub fn tile(space_id: SpaceId, tile: TilePosition) -> Self {
        Self::Tile { space_id, tile }
    }
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

/// Client-side command outbox: player intent (input, UI clicks) waiting to
/// cross the wire. Drained exclusively by `flush_client_commands_to_server`
/// (`NetClientSend`); the server re-attributes each command to the sending
/// peer on ingest.
///
/// Deliberately separate from [`PendingGameCommands`]: in a unified embedded
/// App both queues exist, and routing client intent through its own resource
/// guarantees it always crosses the loopback wire instead of being consumed
/// locally by a server-side drainer's "no player_id → first player" fallback.
#[derive(Resource, Default)]
pub struct ClientPendingCommands {
    pub commands: Vec<GameCommand>,
}

impl ClientPendingCommands {
    pub fn push(&mut self, command: GameCommand) {
        self.commands.push(command);
    }
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

    /// Drain only the commands `matcher` claims, leaving everything else queued
    /// in order. The matcher returns `Ok(payload)` to claim a command or
    /// `Err(command)` to hand it back untouched.
    pub fn drain_matching<T>(
        &mut self,
        mut matcher: impl FnMut(GameCommand) -> Result<T, GameCommand>,
    ) -> Vec<(Option<PlayerId>, T)> {
        let drained = std::mem::take(&mut self.commands);
        let mut remaining = Vec::with_capacity(drained.len());
        let mut claimed = Vec::new();
        for queued in drained {
            match matcher(queued.command) {
                Ok(payload) => claimed.push((queued.player_id, payload)),
                Err(command) => remaining.push(QueuedGameCommand {
                    player_id: queued.player_id,
                    command,
                }),
            }
        }
        self.commands = remaining;
        claimed
    }
}

#[derive(Resource, Default)]
pub struct PendingGameUiEvents {
    /// Client-side inbox: consumed by `apply_game_ui_events` (and refilled by
    /// the client-effects re-queue). Filled by `poll_tcp_client_messages`.
    /// Server code must never write here directly — in a unified App the
    /// server flush would steal client-requeued events and echo them back
    /// over the loopback pipe.
    pub events: Vec<GameUiEvent>,
    /// Server-side per-player outbox, drained by `flush_server_messages`.
    pub peer_events: HashMap<PlayerId, Vec<GameUiEvent>>,
    /// Server-side broadcast outbox, drained by `flush_server_messages`.
    /// Each entry carries an optional spatial scope that the flush uses to
    /// decide which peers receive it; the scope never crosses the wire.
    pub broadcast: Vec<ScopedUiBroadcast>,
}

/// A server-side broadcast queued for delivery. Not a wire type: the flush
/// strips the scope and sends the bare `GameUiEvent`.
#[derive(Clone, Debug)]
pub struct ScopedUiBroadcast {
    pub event: GameUiEvent,
    /// `None` = every synced peer; `Some` = only peers in this space within
    /// the interest radius of the origin tile (z-aware).
    pub scope: Option<(SpaceId, TilePosition)>,
}

impl PendingGameUiEvents {
    /// Queue an event for one player only. Delivered by
    /// `flush_server_messages` to that player's peer (TCP or loopback).
    /// Must not also land in `broadcast`: that list goes to every peer,
    /// which would both duplicate the event for its owner and leak it to
    /// other players.
    pub fn push(&mut self, player_id: PlayerId, event: GameUiEvent) {
        self.peer_events.entry(player_id).or_default().push(event);
    }

    /// Queue an event for every synced peer, regardless of location.
    pub fn push_broadcast(&mut self, event: GameUiEvent) {
        self.broadcast
            .push(ScopedUiBroadcast { event, scope: None });
    }

    /// Queue an event for peers near `origin` in `space_id` (within the
    /// replication interest radius, z-aware). Use for localized effects —
    /// combat VFX, speech bubbles — so they don't leak across spaces/floors.
    pub fn push_broadcast_near(
        &mut self,
        space_id: SpaceId,
        origin: TilePosition,
        event: GameUiEvent,
    ) {
        self.broadcast.push(ScopedUiBroadcast {
            event,
            scope: Some((space_id, origin)),
        });
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ClientVitalStats {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
}

/// Replicated snapshot of the local player's Exertion (fatigue) meter. Drives
/// the HUD fatigue bar. `current` is diffed at whole-point resolution in the
/// projection to avoid per-frame event spam as the meter decays.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ClientExertion {
    pub current: f32,
    pub max: f32,
}

/// Snapshot of an active food/drink regen buff replicated to the client. The
/// HUD renders this as a small "Well Fed: M:SS" badge near the HP/MP bars.
/// `None` on `ClientGameState::regen_buff` means no active buff.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RegenBuffState {
    pub multiplier: f32,
    pub remaining_seconds: f32,
}

/// Replicated snapshot of one active timed magical effect on the local
/// player. Mirrors `magic::effects::ActiveEffect` but lives in the wire-shape
/// module so the client doesn't need to import server-only types. Spelled
/// `Client...` for consistency with `ClientVitalStats` etc.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientActiveEffect {
    pub kind: crate::magic::resources::EffectKind,
    pub magnitude: f32,
    pub remaining_seconds: f32,
    /// Per-kind second parameter (only `Chill` uses it today). `None` for
    /// kinds that don't use a secondary magnitude.
    #[serde(default)]
    pub secondary_magnitude: Option<f32>,
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

/// Snapshot of every fighting-related stat for the local player. The server
/// derives all of these (formulas live in `src/combat/formulas.rs`) and ships
/// the result over the wire so the character sheet UI never has to mirror the
/// combat math. Fields that don't apply to the current loadout (no shield →
/// no block) come through with the corresponding boolean / value zeroed so
/// the UI can hide that row.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientCombatStats {
    pub attack_kind: crate::combat::components::AttackKind,
    pub damage_type: crate::combat::damage_type::DamageType,
    pub damage_min: i32,
    pub damage_max: i32,
    pub attack_bonus: i32,
    pub dodge_dc: i32,
    pub armor: i32,
    pub block: i32,
    pub block_chance_pct: i32,
    pub has_shield: bool,
}

/// What a hostile NPC currently knows about the local player, as revealed to
/// them by a successful Perception "read" (see `player::sense`). Drives the
/// over-head awareness marker. `None` on the world object means the player
/// hasn't read this NPC (or the read lapsed) — they're sneaking blind.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub enum NpcAwareness {
    /// Hasn't noticed the player (Wander, no target).
    Unaware,
    /// Suspicious — investigating a last-seen tile or a noise (Alert).
    Searching,
    /// Has the player as its combat target (Pursue/Engage) — it sees you.
    Alerted,
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
    /// True when this object is a Judge who can absolve guilt for coin. Drives
    /// the "Pay fine" context-menu entry and the `PayGuiltFine` command path.
    #[serde(default)]
    pub is_judge: bool,
    /// True when this object currently carries the server-side `Hidden`
    /// component. The local player only sees the object at all when they
    /// are in `Hidden.detected_by` (the projection filters otherwise), so
    /// this flag tells the UI "you can see it because it's been hidden /
    /// you've spotted it" — used to suppress the "Hide" action on objects
    /// that are already hidden.
    #[serde(default)]
    pub is_hidden: bool,
    /// True when the server-side NPC carries a `HostileBehavior` component.
    /// Drives the threat-color dot in the Nearby NPCs panel.
    #[serde(default)]
    pub is_hostile: bool,
    /// True when this NPC's `CombatTarget` points at the local player's
    /// entity (i.e. it is currently aggroed onto you). Computed per-peer in
    /// `compute_events_for_peer`.
    #[serde(default)]
    pub is_targeting_local_player: bool,
    /// This NPC's awareness of the local player, but only when the player has
    /// successfully "read" it via a Perception check (`player::sense`). `None`
    /// when unread — the over-head marker is the player-facing payoff of the
    /// Perception/Stealth contest. Computed per-peer in `compute_events_for_peer`.
    #[serde(default)]
    pub awareness: Option<NpcAwareness>,
    /// Monotonic server-side placement stamp. Tiebreaker after `tile_position.z`
    /// for both the renderer and the pickup selector, so the most-recently
    /// placed item at a tile is visually on top and is picked up first.
    #[serde(default)]
    pub placement_seq: u64,
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
    /// The remote player's class — selects their class sprite sheet
    /// (`player_<class>` definition). `#[serde(default)]` (→ Fighter) keeps
    /// older peers deserializable; cosmetic-only fallback.
    #[serde(default)]
    pub class: Class,
    /// The remote player's chosen appearance colors, tinting their recolor
    /// layers (modulated by the remote ghost tint client-side).
    #[serde(default)]
    pub appearance: PlayerAppearance,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    /// Active magical effects (spell-driven buffs/debuffs) on the local
    /// player. Full vector each tick — debounced at integer-second
    /// resolution; an empty vec clears the HUD.
    PlayerEffectsChanged {
        effects: Vec<ClientActiveEffect>,
    },
    /// The local player's sneaking state changed. Drives the HUD "Sneaking"
    /// indicator. State, not a one-shot signal — folded into
    /// `ClientGameState.sneaking`.
    PlayerSneakingChanged {
        sneaking: bool,
    },
    /// The local player's Aware state changed. Drives the HUD mode indicator.
    /// State, not a one-shot signal — folded into `ClientGameState.aware`.
    PlayerAwareChanged {
        aware: bool,
    },
    /// The local player's Auto-Retaliate state changed. Drives the HUD mode
    /// indicator. State, not a one-shot signal — folded into
    /// `ClientGameState.auto_retaliate`.
    PlayerAutoRetaliateChanged {
        auto_retaliate: bool,
    },
    /// The local player's Exertion (fatigue) meter changed. State, not a
    /// one-shot — folded into `ClientGameState.exertion`. Diffed at whole-point
    /// resolution in the projection so a continuously-decaying meter doesn't
    /// emit every frame.
    PlayerExertionChanged {
        exertion: ClientExertion,
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
    /// Replication of the local player's `Experience`. Emitted on first
    /// projection and whenever the projected view diverges from the peer's
    /// last-seen baseline (XP grants, level-ups, death's XP-zero rule).
    /// One-shot feedback (level-up toast) rides `GameUiEvent` instead.
    PlayerExperienceChanged {
        experience: ExperienceView,
    },
    /// Replicated when the local player's selected class changes (or is first
    /// projected). Driven by the bootstrap diff after a character is loaded.
    PlayerClassChanged {
        class: Class,
    },
    /// Replicated when the local player's appearance colors change (or are
    /// first projected). Folded into `ClientGameState.appearance`, which the
    /// client copies onto the projected stub so the recolor sprite layers can
    /// spawn/tint.
    PlayerAppearanceChanged {
        appearance: PlayerAppearance,
    },
    /// Replicates the *effective* attribute set (base + equipment bonuses)
    /// for the local player. Drives the Character sheet's attributes grid;
    /// fired when `DerivedStats.attributes` changes between projection ticks.
    PlayerAttributesChanged {
        attributes: AttributeSet,
    },
    /// Replicates the local player's derived combat stats (to-hit, damage
    /// range, dodge DC, armor, block). Drives the Character sheet's Combat
    /// section; fired when any field of the projected `ClientCombatStats`
    /// changes between projection ticks.
    PlayerCombatStatsChanged {
        stats: ClientCombatStats,
    },
    /// Replicates the local player's currently active trade session
    /// (or `None` when the player has no active trade). Sole authority for
    /// the trade panel's contents — the projection diffs the snapshot and
    /// emits this whenever any trade-related field changes.
    TradeStateChanged {
        state: Option<crate::game::trade::ClientTradeView>,
    },
    /// Replicates the local player's party roster (or `None` when unpartied).
    /// Sole authority for the party panel's contents — the projection diffs
    /// the whole snapshot and emits this whenever any member row changes.
    PartyStateChanged {
        party: Option<crate::game::party::ClientPartyView>,
    },
    /// Baseline / corrective replication of the local player's learned
    /// recipe set. Same pattern as `PlayerExperienceChanged` — emitted on
    /// bootstrap and whenever the projection detects drift between the
    /// last-projected set and the player's `CharacterStash`. One-shot
    /// feedback (recipe-learned toast) rides `GameUiEvent`; craft/learn
    /// narration rides `ChatLogChanged`.
    LearnedRecipesChanged {
        recipes: std::collections::BTreeSet<String>,
    },
    /// Baseline / corrective replication of the local player's `LogState`
    /// (quests + notes). Same pattern as `LearnedRecipesChanged`: emitted
    /// on bootstrap and whenever the projection detects drift between the
    /// last-projected log and the player's `CharacterStash["log"]`.
    LogStateChanged {
        state: crate::log::LogState,
    },
    /// Baseline / corrective replication of the local player's `SkillSheet`.
    /// Emitted on bootstrap and whenever the projection detects drift
    /// between the projected ranks/points and the authoritative sheet.
    /// Same pattern as `LearnedRecipesChanged` / `PlayerExperienceChanged`.
    SkillSheetChanged {
        ranks: [u8; 10],
        available_points: u32,
        /// Unspent ability bumps (earned at L4/8/12/16/20). Replicated on this
        /// baseline diff; the bump's attribute change rides
        /// `PlayerAttributesChanged`.
        #[serde(default)]
        available_ability_bumps: u32,
    },
    /// Baseline replication of the local player's full `DiscoveredTiles`
    /// state. Sent on first projection / when the projection detects drift.
    /// The fold overwrites `ClientGameState.discovered_tiles`. Tiles are
    /// grouped by `SpaceId` so a multi-space discovery history travels in one
    /// payload. Tuples are 2D `(x, y)` — fog of war ignores floor.
    DiscoveredTilesReplaced {
        tiles: HashMap<SpaceId, Vec<(i32, i32)>>,
    },
    /// Delta event: the local player just revealed `tiles` in `space_id`.
    /// Emitted by `compute_events_for_peer` when authoritative
    /// `DiscoveredTiles` grew between projection ticks. Folded as a union
    /// into `ClientGameState.discovered_tiles`. Tuples are 2D `(x, y)`.
    TilesDiscovered {
        space_id: SpaceId,
        tiles: Vec<(i32, i32)>,
    },
}

#[derive(Resource, Default)]
pub struct PendingGameEvents {
    pub events: Vec<GameEvent>,
}

/// Server-side monotonic counter stamped onto every world-object placement.
/// Drives last-placed-wins (LIFO) stack ordering for items sharing the same
/// `(space, x, y, z)` — most relevant for flat (`block_size == 0`) items, which
/// otherwise all collapse to `z = 0`. The renderer and the pickup selector
/// both use `(tile.z, placement_seq)` as the ordering key so visual top
/// matches pickup top. Runtime-only; no persistence.
#[derive(Resource, Default)]
pub struct PlacementSeqCounter(u64);

impl PlacementSeqCounter {
    /// Returns a fresh seq and increments. Call this from every site that
    /// places an `OverworldObject` onto a tile or moves it between tiles.
    // Named `next` deliberately — counter semantics, not an `Iterator`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        let v = self.0;
        self.0 = self.0.wrapping_add(1);
        v
    }

    /// Current counter value (next `next()` returns this). For tests / debug.
    #[cfg(test)]
    pub fn current(&self) -> u64 {
        self.0
    }
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
    /// Active magical effects on the local player. Driven by
    /// `PlayerEffectsChanged`; the HUD renders the list and presentation
    /// systems (e.g. Glimmer light expansion) read from it.
    #[serde(default)]
    pub active_effects: Vec<ClientActiveEffect>,
    /// Whether the local player is currently sneaking. Driven by
    /// `PlayerSneakingChanged`; the HUD renders a "Sneaking" indicator.
    #[serde(default)]
    pub sneaking: bool,
    /// Whether the local player is currently in Aware mode. Driven by
    /// `PlayerAwareChanged`; the HUD mode box renders an indicator.
    #[serde(default)]
    pub aware: bool,
    /// Whether the local player has Auto-Retaliate enabled. Driven by
    /// `PlayerAutoRetaliateChanged`; the HUD mode box renders an indicator.
    #[serde(default)]
    pub auto_retaliate: bool,
    /// Replicated Exertion (fatigue) snapshot for the local player. `None`
    /// until the first `PlayerExertionChanged` event arrives. Drives the HUD
    /// fatigue bar.
    #[serde(default)]
    pub exertion: Option<ClientExertion>,
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
    /// Replicated appearance colors for the local player. `None` until the
    /// first `PlayerAppearanceChanged` event lands.
    #[serde(default)]
    pub appearance: Option<PlayerAppearance>,
    /// Replicated effective attribute set (base + equipment) for the local
    /// player. `None` until the first `PlayerAttributesChanged` event lands.
    #[serde(default)]
    pub attributes: Option<AttributeSet>,
    /// Replicated combat stats for the local player. `None` until the first
    /// `PlayerCombatStatsChanged` event lands. Drives the Combat section of
    /// the Character sheet.
    #[serde(default)]
    pub combat_stats: Option<ClientCombatStats>,
    /// Snapshot of the local player's active trade, or `None`. Updated by
    /// `GameEvent::TradeStateChanged`; the trade panel reads from this.
    #[serde(default)]
    pub current_trade: Option<crate::game::trade::ClientTradeView>,
    /// Snapshot of the local player's party roster, or `None` when unpartied.
    /// Updated by `GameEvent::PartyStateChanged`; the party panel, minimap
    /// dots, and world markers all read from this.
    #[serde(default)]
    pub party: Option<crate::game::party::ClientPartyView>,
    /// Recipes the local player has learned. Drives the recipe-book UI.
    /// Folded from `GameEvent::LearnedRecipesChanged`. `BTreeSet` for
    /// deterministic iteration in the UI.
    #[serde(default)]
    pub learned_recipes: std::collections::BTreeSet<String>,
    /// Local player's per-character log (Quests + Notes + future sections).
    /// Folded from `GameEvent::LogStateChanged`. Drives the Log panel UI.
    #[serde(default)]
    pub log_state: crate::log::LogState,
    /// Local player's skill ranks (indexed by `Skill::index()`). Folded from
    /// `GameEvent::SkillSheetChanged`.
    #[serde(default)]
    pub skill_ranks: [u8; 10],
    /// Unspent skill points the local player can allocate. Folded from
    /// `SkillSheetChanged`.
    #[serde(default)]
    pub available_skill_points: u32,
    /// Unspent ability bumps (earned at L4/8/12/16/20). Folded from
    /// `SkillSheetChanged` (baseline). Drives the ability-bump UI.
    #[serde(default)]
    pub available_ability_bumps: u32,
    /// Tiles the local player has ever seen, grouped by space. Drives the
    /// fog-of-war overlay on the main view and the minimap. Folded from
    /// `DiscoveredTilesReplaced` (baseline) and `TilesDiscovered` (delta).
    /// Stored as 2D `(x, y)` — fog of war ignores floor.
    #[serde(default)]
    pub discovered_tiles: HashMap<SpaceId, HashSet<(i32, i32)>>,
}

/// Per-domain change counters for `ClientGameState`, bumped only by
/// `apply_game_events_to_client_state` (the client fold system) when the
/// relevant event variants land.
///
/// `ClientGameState` is a monolithic resource: any single applied event
/// `DerefMut`s the whole thing, so `ClientGameState::is_changed()` is true on
/// nearly every frame and is therefore useless as a redraw gate. Presentation
/// systems that consume a *large* slice of state (the world-object map, the
/// minimap) compare these `u64`s against a `Local<u64>` instead of snapshotting
/// the whole collection each frame. Panels that render a small slice should
/// prefer a snapshot `Local` of exactly the fields they read.
///
/// Client-only: maintained by the client fold, never by the server's per-peer
/// baseline advance (which calls `apply_event_to_state` directly).
#[derive(Clone, Copy, Debug, Default, Resource)]
pub struct ClientStateRevisions {
    /// Bumped on `WorldObjectUpserted` / `WorldObjectRemoved`.
    pub world_objects: u64,
    /// Bumped on `RemotePlayerUpserted` / `RemotePlayerRemoved`.
    pub remote_players: u64,
    /// Bumped on `FloorMapReplaced` / `FloorTileSet` / `DiscoveredTilesReplaced`
    /// / `TilesDiscovered` — i.e. anything that changes the painted map window.
    pub map_tiles: u64,
    /// Bumped on `FloorMapReplaced` / `FloorTileSet` only — actual floor-grid
    /// edits, *not* fog-of-war discovery. Discovery fires on nearly every step,
    /// so systems that only care about painted tiles (floor render, indoor
    /// map) must gate on this instead of `map_tiles`.
    pub floor_maps: u64,
    /// Bumped on `DiscoveredTilesReplaced` / `TilesDiscovered` — fog-of-war
    /// progress for the local player.
    pub discovered: u64,
    /// Bumped on `LogStateChanged` — the per-character log (quests/notes).
    pub log: u64,
    /// Bumped on `InventoryChanged` / `ContainerChanged` / `ContainerRemoved`
    /// / `PlayerStorageChanged` — everything the backpack / equipment /
    /// container-slot UIs render from.
    pub inventory: u64,
    /// Bumped on `PartyStateChanged`. The minimap gates its party dots on
    /// this — out-of-interest-radius members never touch `remote_players`.
    pub party: u64,
}
