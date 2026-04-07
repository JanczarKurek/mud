pub mod components;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::player::setup::spawn_player;
use crate::player::systems::{move_player_on_grid, refresh_derived_player_stats};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_player)
            .add_systems(Update, (refresh_derived_player_stats, move_player_on_grid));
    }
}
