use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::player::components::PlayerId;
use crate::world::components::{SpaceId, TilePosition};

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Npc;

/// Which side a combatant fights on. Faction enmity is one of the two gates
/// in `npc::hostility::is_hostile_toward` (the other is tag overlap). Players
/// default to `PlayerSide`; map-hostile mobs are tagged `MonsterSide` at
/// spawn; a companion inherits its owner's side. `Neutral` is for creatures
/// that fight only through tags (a wolf hunting `livestock`) or not at all
/// (sheep): nobody's faction enemy, but still a valid combatant/target. A
/// faction-less NPC (shopkeeper, quest-giver) reads as the `PlayerSide`
/// default — never an enemy of the player side, so companions and
/// player-allied creatures leave it alone.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum Faction {
    #[default]
    PlayerSide,
    MonsterSide,
    Neutral,
}

impl Faction {
    pub fn is_enemy_of(self, other: Faction) -> bool {
        matches!(
            (self, other),
            (Faction::PlayerSide, Faction::MonsterSide)
                | (Faction::MonsterSide, Faction::PlayerSide)
        )
    }
}

/// Marks an NPC as a companion fighting for `owner`. `owner_player` is the
/// player to credit kills to (XP / quest / kill feed) — `None` for a companion
/// owned by another NPC. Deliberately **not** persisted: `owner` is a live
/// `Entity` that is meaningless across save/load, and summoned companions are
/// ephemeral (mirrors `HazardOwner`).
#[derive(Component, Clone, Copy, Debug)]
pub struct Companion {
    pub owner: Entity,
    pub owner_player: Option<PlayerId>,
    /// When no enemy is visible, the companion follows its owner until it is
    /// within this many tiles, then idles/wanders in place.
    pub follow_close_tiles: i32,
}

#[derive(Component, Clone, Debug, Deserialize, Serialize)]
pub struct SpawnGroupMember {
    pub space_id: SpaceId,
    pub group_id: String,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamingBehavior {
    pub bounds: RoamBounds,
    pub step_interval_seconds: f32,
    /// Random extra time added to each step interval, sampled uniformly from
    /// `[0, step_interval_jitter_seconds]`. Desynchronizes NPCs that share a
    /// spawn group so they don't all decide on the same frame.
    #[serde(default)]
    pub step_interval_jitter_seconds: f32,
    /// Probability per Wander step of pausing in place instead of moving.
    /// Lets idle NPCs look around between movements.
    #[serde(default = "default_idle_pause_chance")]
    pub idle_pause_chance: f32,
    /// Weight on continuing in the previous step's direction during Wander.
    /// 0.0 = uniform random, 1.0 = always continue. Default ~0.6 gives a
    /// natural drift while still letting the NPC turn.
    #[serde(default = "default_momentum_bias")]
    pub momentum_bias: f32,
}

fn default_idle_pause_chance() -> f32 {
    0.3
}

fn default_momentum_bias() -> f32 {
    0.6
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct HostileBehavior {
    pub detect_distance_tiles: i32,
    pub disengage_distance_tiles: i32,
    /// While Alert, NPC walks toward the last-seen target tile for this many
    /// seconds before giving up and returning to Wander.
    #[serde(default = "default_alert_duration_seconds")]
    pub alert_duration_seconds: f32,
    /// If true, this NPC requires an unobstructed line-of-sight to a player
    /// to acquire / maintain aggro. If false, aggro is purely distance-based.
    #[serde(default = "default_requires_line_of_sight")]
    pub requires_line_of_sight: bool,
    /// Perception bonus added to this NPC's spotting roll when contesting a
    /// player's Stealth (see `detection_outcome`). Higher = sharper-eyed guard.
    /// `detect_distance_tiles` stays the hard maximum sensing range; within it,
    /// whether the NPC actually notices a sneaking player is this opposed roll.
    #[serde(default)]
    pub perception: i32,
}

fn default_alert_duration_seconds() -> f32 {
    4.0
}

fn default_requires_line_of_sight() -> bool {
    true
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamingStepTimer {
    pub remaining_seconds: f32,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamingRandomState {
    pub seed: u64,
}

impl RoamingRandomState {
    /// Advance the LCG one step. Shared by the wander RNG helpers in
    /// `npc::systems` and the routine decision logic in `npc::routine` so both
    /// draw from the same deterministic stream.
    pub fn advance(&mut self) {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    }

    /// Uniform `[0, 1)` from the high 24 bits of the next LCG state.
    pub fn next_f32(&mut self) -> f32 {
        self.advance();
        let bits = (self.seed >> 40) as u32 & 0x00FF_FFFF;
        bits as f32 / 16_777_216.0
    }

    /// Uniform index in `0..modulo` from the next LCG state. Caller guarantees
    /// `modulo > 0`.
    pub fn next_index(&mut self, modulo: usize) -> usize {
        self.advance();
        ((self.seed >> 32) as usize) % modulo
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl RoamBounds {
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

/// Current AI state for an NPC. Drives which `tick_*` branch
/// `update_roaming_npcs` dispatches to. State transitions are decided every
/// AI tick based on player visibility, range, and elapsed time.
#[derive(Component, Clone, Copy, Debug, Default)]
pub enum AiState {
    /// No target. Wander around the roam bounds with momentum and pauses.
    #[default]
    Wander,
    /// Lost a target; head toward where we last saw them. Reverts to Wander
    /// when `expires_at_seconds` (in elapsed seconds since startup) is reached.
    Alert {
        last_seen: TilePosition,
        expires_at_seconds: f32,
    },
    /// Have a target, not yet in attack range. Path to them via A*.
    Pursue { target: Entity },
    /// Have a target and in attack range. Hold (melee) or kite (ranged).
    Engage { target: Entity },
    /// Running away from `from`. See `FleeReason` for what started it; the
    /// reason also decides how the flee sustains and ends (re-engage when a
    /// path opens vs. calm down when the predator is out of detect range).
    Flee {
        from: Entity,
        expires_at_seconds: f32,
        reason: FleeReason,
    },
}

/// Why an NPC is in `AiState::Flee`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FleeReason {
    /// Took damage from a target A* proved unreachable (the player camped a
    /// ledge we can't climb to). Move away and try to break line of sight;
    /// re-engage the moment a path to the attacker opens up.
    UnreachableAttacker,
    /// A creature bearing one of our `flees_from` tags is nearby (prey
    /// behavior — sheep running from a wolf). Keeps running while the
    /// predator stays within detect range, then calms down to Wander.
    Fear,
    /// Hit by `from` while having no way to fight back (no `HostileBehavior`
    /// — a villager or sheep under attack). Sustained like `Fear` (calm down
    /// once the attacker is out of detect range) and additionally refreshed
    /// while damage stays fresh, so a ranged attacker keeps the victim
    /// running.
    Attacked,
}

/// Prey behavior: the NPC runs from creatures bearing any of its
/// `flees_from` tags (resolved into `TagProfile.flees_from`). Attached at
/// spawn when the definition lists `flees_from`; carries its own detection
/// numbers because prey (sheep) usually has no `HostileBehavior` to borrow
/// them from.
#[derive(Component, Clone, Copy, Debug)]
pub struct PreyBehavior {
    /// Chebyshev radius within which a feared creature is noticed — and
    /// within which an ongoing flee keeps refreshing instead of expiring.
    pub detect_distance_tiles: i32,
    /// When true, a feared creature behind an unbroken wall goes unnoticed.
    pub requires_line_of_sight: bool,
}

/// Per-NPC scratch memory the FSM reads and writes between ticks.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct AiMemory {
    /// Last cardinal/diagonal step the NPC took during Wander, used by the
    /// momentum bias. `None` immediately after spawn or after a pause.
    pub last_step: Option<IVec2>,
    /// `time.elapsed_secs()` at which this NPC last emitted any speech
    /// bubble. Used to rate-limit ambient mutters so a chatty NPC doesn't
    /// spam the bubble overlay. Zero on spawn means "never spoken".
    pub last_bark_seconds: f32,
    /// Elapsed-seconds deadline through which a Pursue/Engage keeps its
    /// CombatTarget after a soft contact loss (LoS flicker or a brush past the
    /// leash). Refreshed on every healthy contact tick; when `elapsed` passes
    /// it while contact is still broken, the NPC drops to Alert. Zero on spawn
    /// means "no live contact".
    pub contact_grace_until: f32,
}

/// Pools of utterances the AI can draw from for floating speech bubbles.
/// Resolved from the NPC's `BarkDef` at spawn time. Component is omitted
/// entirely for NPCs whose definition has no bark lists.
#[derive(Component, Clone, Debug, Default)]
pub struct Barks {
    pub aggro: Vec<String>,
    pub mutter: Vec<String>,
    /// Shouted when raising the alarm about a witnessed crime (protectors).
    pub alarm: Vec<String>,
}

/// Elapsed-seconds timestamp of the last time this entity took damage.
/// Inserted by `apply_pending_damage` on every successful damage application.
/// Drives the AI's flee-trigger ("hurt recently AND can't reach attacker").
#[derive(Component, Clone, Copy, Debug)]
pub struct LastDamagedAt(pub f32);

/// Minimum seconds between two bubbles from the same NPC. Caps spam even
/// when several rolls succeed in a row, and prevents an aggro bark from
/// being immediately stepped on by a mutter. Shared by `npc::systems` and
/// `npc::social` so ambient mutters and social chatter draw on the same
/// per-NPC cooldown. Lives here (not `npc::systems`) so the sim-gated
/// boundary doesn't cut the `npc::social` import.
pub(crate) const BUBBLE_COOLDOWN_SECONDS: f32 = 8.0;
