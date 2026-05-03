pub mod commands;
pub mod helpers;
pub mod projection;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::combat::systems::resolve_battle_turn;
use crate::game::projection::{
    apply_game_events_to_client_state, collect_game_events_from_authority,
};
use crate::game::resources::{
    ClientGameState, ContainerViewers, PendingGameCommands, PendingGameEvents, PendingGameUiEvents,
};
use crate::game::systems::{
    process_floor_commands, process_game_commands, process_rotate_commands,
    tick_player_movement_cooldowns,
};
use crate::npc::systems::update_roaming_npcs;
use crate::player::systems::move_player_on_grid;
use crate::world::interactions::{process_interact_commands, sync_container_visual_state};

pub struct GameServerPlugin;

pub struct GameClientPlugin;

/// Runs before `process_game_commands`. Plugins (e.g. dialog) that want to
/// drain specific `GameCommand` variants before the main processor sees them
/// should register their systems `.in_set(CommandIntercept)`.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, SystemSet)]
pub struct CommandIntercept;

impl Plugin for GameServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingGameCommands::default())
            .insert_resource(PendingGameEvents::default())
            .insert_resource(PendingGameUiEvents::default())
            .insert_resource(ClientGameState::default())
            .insert_resource(ContainerViewers::default())
            .configure_sets(
                Update,
                CommandIntercept
                    .after(tick_player_movement_cooldowns)
                    .before(process_game_commands),
            )
            .add_systems(
                Update,
                tick_player_movement_cooldowns
                    .after(move_player_on_grid)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                process_rotate_commands
                    .in_set(CommandIntercept)
                    .run_if(simulation_active),
            )
            // Not gated on `simulation_active`: the only command this drains is
            // `EditorSetFloorTile`, which originates from `MapEditor` (where
            // simulation is paused).
            .add_systems(Update, process_floor_commands.in_set(CommandIntercept))
            .add_systems(
                Update,
                process_interact_commands
                    .in_set(CommandIntercept)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                process_game_commands
                    .after(tick_player_movement_cooldowns)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                sync_container_visual_state
                    .after(process_game_commands)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                collect_game_events_from_authority
                    .after(process_game_commands)
                    .after(sync_container_visual_state)
                    .after(update_roaming_npcs)
                    .after(resolve_battle_turn)
                    .run_if(simulation_active),
            )
            // Unconditional — mirrors GameClientPlugin so that WorldClientPlugin's
            // .after(apply_game_events_to_client_state) ordering resolves identically
            // in EmbeddedClient mode and TcpClient mode.
            .add_systems(
                Update,
                apply_game_events_to_client_state.after(collect_game_events_from_authority),
            );
    }
}

impl Plugin for GameClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingGameCommands::default())
            .insert_resource(PendingGameEvents::default())
            .insert_resource(PendingGameUiEvents::default())
            .insert_resource(ClientGameState::default())
            .add_systems(Update, apply_game_events_to_client_state);
    }
}
