use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::components::{SpaceId, TilePosition};

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Npc;

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
    /// Took damage from a target we can't reach (the player camped a ledge
    /// we can't climb to). Move away from `from` and try to break line of
    /// sight. Expires to Wander at `expires_at_seconds` if no new damage
    /// arrives and the attacker can't see us.
    Flee {
        from: Entity,
        expires_at_seconds: f32,
    },
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
}

/// Pools of utterances the AI can draw from for floating speech bubbles.
/// Resolved from the NPC's `BarkDef` at spawn time. Component is omitted
/// entirely for NPCs whose definition has no bark lists.
#[derive(Component, Clone, Debug, Default)]
pub struct Barks {
    pub aggro: Vec<String>,
    pub mutter: Vec<String>,
}

/// Elapsed-seconds timestamp of the last time this entity took damage.
/// Inserted by `apply_pending_damage` on every successful damage application.
/// Drives the AI's flee-trigger ("hurt recently AND can't reach attacker").
#[derive(Component, Clone, Copy, Debug)]
pub struct LastDamagedAt(pub f32);
