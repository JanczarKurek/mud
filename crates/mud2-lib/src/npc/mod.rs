#[cfg(feature = "server-sim")]
pub mod aggro;
#[cfg(feature = "server-sim")]
pub mod bestiary;
pub mod components;
pub mod debug_overlay;
#[cfg(feature = "server-sim")]
pub mod detection;
#[cfg(feature = "server-sim")]
pub mod guilt;
#[cfg(feature = "server-sim")]
pub mod hostility;
pub mod routine;
pub mod social;
#[cfg(feature = "server-sim")]
pub mod social_read;
#[cfg(feature = "server-sim")]
pub mod spawn_groups;
pub mod spellcasting;
#[cfg(feature = "server-sim")]
pub mod systems;
#[cfg(feature = "server-sim")]
pub mod witness;

#[cfg(feature = "server-sim")]
use bevy::prelude::*;

#[cfg(feature = "server-sim")]
use crate::app::state::simulation_active;
#[cfg(feature = "server-sim")]
use crate::npc::social::{tick_social_chatter, ConversationRegistry};
#[cfg(feature = "server-sim")]
use crate::npc::spawn_groups::{
    bootstrap_spawn_groups, tick_spawn_groups, PendingSpawnGroupDumps, SpawnGroupRegistry,
};
#[cfg(feature = "server-sim")]
use crate::npc::systems::{despawn_orphaned_companions, update_roaming_npcs};
#[cfg(feature = "server-sim")]
use crate::world::setup::WorldStartupSet;

#[cfg(feature = "server-sim")]
pub struct NpcPlugin;

#[cfg(feature = "server-sim")]
impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnGroupRegistry>()
            .init_resource::<PendingSpawnGroupDumps>()
            .init_resource::<ConversationRegistry>()
            .init_resource::<crate::npc::aggro::PendingNpcAggro>()
            .add_systems(
                Startup,
                bootstrap_spawn_groups.after(WorldStartupSet::InitializeRuntimeSpaces),
            )
            .add_systems(
                Update,
                (
                    // Resolve tag/faction components for freshly-spawned NPCs
                    // before the AI tick so a new guard/prey acts on its tags
                    // from its very first step.
                    crate::npc::hostility::resolve_npc_tag_components.before(update_roaming_npcs),
                    // Free orphaned companions before the AI tick so a companion
                    // never steps against a dangling owner lookup.
                    despawn_orphaned_companions.before(update_roaming_npcs),
                    update_roaming_npcs,
                    // After the AI tick so it reads post-step positions and the
                    // final per-NPC AiState.
                    tick_social_chatter.after(update_roaming_npcs),
                    tick_spawn_groups,
                )
                    .run_if(simulation_active),
            )
            // Claims the judge commands (`RequestCrimeList` / `PayCrime`) out
            // of the command queue before the main dispatcher sees them, the
            // same way dialog and trade claim theirs.
            .add_systems(
                Update,
                crate::npc::guilt::process_judge_commands
                    .in_set(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Social reads: peeks at `Inspect` (leaving it queued for the main
            // dispatcher) and claims `RequestSocialRead`. Its chat summaries
            // are deferred through `PendingSocialReadLines` so they print
            // after the inspect description.
            .add_systems(
                Update,
                crate::npc::social_read::process_social_read
                    .in_set(crate::game::CommandIntercept)
                    // Queues People-codex updates for `CodexSet::Apply`.
                    .in_set(crate::codex::CodexSet::Reveal)
                    .run_if(simulation_active),
            )
            // Passive Bestiary observation. Same set, so its tier-ups land in
            // the same `CodexSet::Apply` pass as any read this frame.
            .add_systems(
                Update,
                crate::npc::bestiary::tick_bestiary_observation
                    .in_set(crate::codex::CodexSet::Reveal)
                    .run_if(simulation_active),
            );
    }
}
