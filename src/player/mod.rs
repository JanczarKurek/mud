pub mod components;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::player::setup::spawn_player_visual;
use crate::player::systems::{
    move_player_on_grid, refresh_derived_player_stats, rotate_nearby_object_on_shortcut,
    sync_authoritative_player_display, sync_authoritative_player_position_view,
    sync_projected_player_from_client_state,
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
        app.add_systems(OnEnter(ClientAppState::InGame), spawn_player_visual)
            .add_systems(
                Update,
                (
                    sync_authoritative_player_display,
                    sync_authoritative_player_position_view,
                    sync_projected_player_from_client_state,
                    move_player_on_grid,
                    rotate_nearby_object_on_shortcut,
                )
                    .run_if(in_state(ClientAppState::InGame)),
            );
    }
}
