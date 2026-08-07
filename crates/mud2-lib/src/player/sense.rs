//! Player-side stealth sensing: while sneaking, the player periodically rolls
//! **Perception** to "read" nearby hostile NPCs. Each success reveals that NPC's
//! awareness of the player for a few seconds, which the projection turns into an
//! over-head marker (see `game::resources::NpcAwareness` and
//! `client_effects::awareness`). A low-Perception sneaker reads guards
//! unreliably and is often sneaking blind — that uncertainty is the point.
//!
//! Server-authoritative: this only populates `SenseReveals` on the player; the
//! marker itself is replicated through the normal world-object diff and rendered
//! client-side from `ClientGameState`.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::npc::components::{HostileBehavior, Npc};
use crate::player::components::{
    Aware, BaseStats, Exertion, Player, Sneaking, AWARE_PERCEPTION_BONUS,
};
use crate::player::exertion::{exertion_dc_modifier, EXERTION_COST_SNEAK_PER_SEC};
use crate::player::skills::{skill_check, Skill};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};

/// Seconds between sensing rolls. `[tunable]`
const SENSE_INTERVAL: f32 = 1.0;

/// How long a successful read of an NPC lingers before it must be re-read.
/// Refreshed on every successful roll. `[tunable]`
const REVEAL_DURATION: f32 = 3.0;

/// Max Chebyshev distance (tiles) at which the player attempts to read an NPC.
/// `[tunable]`
const SENSE_RANGE: i32 = 10;

/// Base Perception DC to read an NPC; the NPC's distance is added on top, so a
/// guard right next to you is easy to read and a far one is hard. `[tunable]`
const SENSE_BASE_DC: i32 = 8;

/// Per-player sensing state: roll cooldown + the set of currently-read NPCs
/// (object id → elapsed-seconds expiry). Session-only; never persisted. The
/// projection treats the live keys as "revealed right now".
#[derive(Component, Default)]
pub struct SenseReveals {
    cooldown: f32,
    pub revealed: HashMap<u64, f32>,
}

impl SenseReveals {
    /// True if the player has a live read on this NPC.
    pub fn reveals(&self, object_id: u64) -> bool {
        self.revealed.contains_key(&object_id)
    }
}

fn chebyshev(a: TilePosition, b: TilePosition) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// Drains the sense cooldown and, while the player is sneaking, rolls Perception
/// against each nearby hostile NPC to refresh `SenseReveals`. Expired reads are
/// pruned every frame so they fade out shortly after the player stops sensing.
#[allow(clippy::type_complexity)]
pub fn tick_player_sense(
    time: Res<Time>,
    mut commands: Commands,
    mut player_q: Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &BaseStats,
            &crate::player::skills::SkillSheet,
            Has<Sneaking>,
            Has<Aware>,
            Option<&mut SenseReveals>,
            Option<&mut Exertion>,
        ),
        With<Player>,
    >,
    npc_q: Query<
        (&OverworldObject, &SpaceResident, &TilePosition),
        (With<Npc>, With<HostileBehavior>),
    >,
) {
    let dt = time.delta_secs();
    let elapsed = time.elapsed_secs();

    for (
        entity,
        player_space,
        player_tile,
        base_stats,
        skill_sheet,
        sneaking,
        is_aware,
        reveals,
        mut exertion,
    ) in &mut player_q
    {
        let Some(mut reveals) = reveals else {
            // First time we see this player — attach the component; it starts
            // sensing next frame.
            commands.entity(entity).insert(SenseReveals::default());
            continue;
        };

        // Always prune expired reads so markers fade even when not sneaking.
        reveals.revealed.retain(|_, expiry| *expiry > elapsed);

        // Sensing is a sneaking activity. When not sneaking, reads just lapse.
        if !sneaking {
            continue;
        }

        reveals.cooldown -= dt;
        if reveals.cooldown > 0.0 {
            continue;
        }
        reveals.cooldown = SENSE_INTERVAL;

        // Sustained sneaking is tiring (once-per-interval cost), and fatigue
        // raises the read DC. Read the penalty before charging the cost.
        let fatigue_dc = exertion_dc_modifier(exertion.as_deref());
        if let Some(e) = exertion.as_mut() {
            e.add(EXERTION_COST_SNEAK_PER_SEC);
        }

        for (object, npc_space, npc_tile) in &npc_q {
            if npc_space.space_id != player_space.space_id {
                continue;
            }
            let distance = chebyshev(*player_tile, *npc_tile);
            if distance > SENSE_RANGE {
                continue;
            }
            let dc = SENSE_BASE_DC + distance + fatigue_dc;
            let aware_bonus = if is_aware { AWARE_PERCEPTION_BONUS } else { 0 };
            let result = skill_check(
                skill_sheet,
                &base_stats.attributes,
                Skill::Perception,
                dc,
                aware_bonus,
            );
            if result.success {
                reveals
                    .revealed
                    .insert(object.object_id, elapsed + REVEAL_DURATION);
            }
        }
    }
}
