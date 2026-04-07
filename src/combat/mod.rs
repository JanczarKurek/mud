pub mod components;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::combat::resources::BattleTurnTimer;
use crate::combat::systems::{clear_invalid_combat_targets, resolve_battle_turn};
use crate::npc::systems::update_roaming_npcs;
use crate::player::systems::move_player_on_grid;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(BattleTurnTimer::default()).add_systems(
            Update,
            (clear_invalid_combat_targets, resolve_battle_turn)
                .chain()
                .after(move_player_on_grid)
                .after(update_roaming_npcs),
        );
    }
}
