pub mod components;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::player::setup::spawn_player_visual;
use crate::player::systems::{
    move_player_on_grid, refresh_derived_player_stats, sync_player_client_state,
};

pub struct PlayerServerPlugin;

pub struct PlayerClientPlugin;

impl Plugin for PlayerServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, refresh_derived_player_stats);
    }
}

impl Plugin for PlayerClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_player_visual)
            .add_systems(Update, (sync_player_client_state, move_player_on_grid));
    }
}
