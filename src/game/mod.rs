pub mod commands;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::combat::systems::resolve_battle_turn;
use crate::game::resources::{
    ClientGameState, PendingGameCommands, PendingGameEvents, PendingGameUiEvents,
};
use crate::game::systems::{
    apply_game_events_to_client_state, collect_game_events_from_authority, process_game_commands,
    tick_player_movement_cooldowns,
};
use crate::npc::systems::update_roaming_npcs;
use crate::player::systems::move_player_on_grid;

pub struct GameServerPlugin;

pub struct GameClientPlugin;

impl Plugin for GameServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingGameCommands::default())
            .insert_resource(PendingGameEvents::default())
            .insert_resource(PendingGameUiEvents::default())
            .insert_resource(ClientGameState::default())
            .add_systems(
                Update,
                tick_player_movement_cooldowns.after(move_player_on_grid),
            )
            .add_systems(
                Update,
                process_game_commands.after(tick_player_movement_cooldowns),
            )
            .add_systems(
                Update,
                collect_game_events_from_authority
                    .after(process_game_commands)
                    .after(update_roaming_npcs)
                    .after(resolve_battle_turn),
            )
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
