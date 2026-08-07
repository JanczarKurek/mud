pub mod components;
pub mod debug_overlay;
pub mod detection;
pub mod routine;
pub mod social;
pub mod spawn_groups;
pub mod spellcasting;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::npc::social::{tick_social_chatter, ConversationRegistry};
use crate::npc::spawn_groups::{
    bootstrap_spawn_groups, tick_spawn_groups, PendingSpawnGroupDumps, SpawnGroupRegistry,
};
use crate::npc::systems::{despawn_orphaned_companions, update_roaming_npcs};
use crate::world::setup::WorldStartupSet;

pub struct NpcPlugin;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnGroupRegistry>()
            .init_resource::<PendingSpawnGroupDumps>()
            .init_resource::<ConversationRegistry>()
            .add_systems(
                Startup,
                bootstrap_spawn_groups.after(WorldStartupSet::InitializeRuntimeSpaces),
            )
            .add_systems(
                Update,
                (
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
            );
    }
}
