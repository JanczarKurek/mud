pub mod commands;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::game::resources::{
    ChatLogState, ClientGameState, InventoryState, PendingGameCommands, PendingGameEvents,
    PendingGameUiEvents,
};
use crate::game::systems::{
    apply_game_events_to_client_state, collect_game_events_from_authority, process_game_commands,
    tick_player_movement_cooldowns,
};
use crate::player::systems::move_player_on_grid;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InventoryState::default())
            .insert_resource(ChatLogState::default())
            .insert_resource(PendingGameCommands::default())
            .insert_resource(PendingGameEvents::default())
            .insert_resource(PendingGameUiEvents::default())
            .insert_resource(ClientGameState::default())
            .add_systems(
                Update,
                (
                    tick_player_movement_cooldowns,
                    process_game_commands,
                    collect_game_events_from_authority,
                    apply_game_events_to_client_state,
                )
                    .chain()
                    .after(move_player_on_grid),
            );
    }
}
