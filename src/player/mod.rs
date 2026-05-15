pub mod classes;
pub mod components;
pub mod lifecycle;
pub mod progression;
pub mod regen;
pub mod setup;
pub mod skills;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::{simulation_active, ClientAppState};
use crate::player::lifecycle::{
    handle_player_deaths, handle_set_home_commands, PendingPlayerDeaths,
};
use crate::player::progression::{apply_xp_grants, PendingXpGrants};
use crate::player::regen::{tick_regen_buffs, tick_vital_regen};
use crate::player::setup::spawn_player_visual;
use crate::player::skills::process_allocate_skill_commands;
use crate::player::systems::{
    move_player_on_grid, refresh_derived_player_stats, rotate_nearby_object_on_shortcut,
    set_home_on_keypress, sync_authoritative_player_display,
    sync_authoritative_player_position_view, sync_projected_player_from_client_state,
};

pub struct PlayerServerPlugin;

pub struct PlayerClientPlugin;

impl Plugin for PlayerServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingPlayerDeaths>()
            .init_resource::<PendingXpGrants>()
            .add_systems(Update, refresh_derived_player_stats)
            .add_systems(
                Update,
                apply_xp_grants
                    .after(crate::combat::systems::resolve_battle_turn)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                (tick_regen_buffs, tick_vital_regen).run_if(simulation_active),
            )
            // Drain SetHome from PendingGameCommands *before* process_game_commands;
            // CommandIntercept handles the cross-plugin ordering that a bare
            // `.before(...)` would silently drop (per project memory note).
            .add_systems(
                Update,
                handle_set_home_commands
                    .in_set(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Skill-point allocation: same `CommandIntercept` pattern so the
            // main `process_game_commands` only sees a no-op warning arm.
            .add_systems(
                Update,
                process_allocate_skill_commands
                    .in_set(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Handle deaths after combat resolution. resolve_battle_turn fills
            // PendingPlayerDeaths; this drains it.
            .add_systems(
                Update,
                handle_player_deaths
                    .after(crate::combat::systems::resolve_battle_turn)
                    .run_if(simulation_active),
            );
    }
}

impl Plugin for PlayerClientPlugin {
    fn build(&self, app: &mut App) {
        crate::ui::skills_panel::register(app);
        app.add_systems(OnEnter(ClientAppState::InGame), spawn_player_visual)
            .add_systems(
                Update,
                (
                    sync_authoritative_player_display,
                    sync_authoritative_player_position_view,
                    sync_projected_player_from_client_state,
                )
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    move_player_on_grid,
                    rotate_nearby_object_on_shortcut,
                    set_home_on_keypress,
                )
                    .run_if(in_state(ClientAppState::InGame))
                    .run_if(bevy_terminal::terminal_not_focused),
            );
    }
}
