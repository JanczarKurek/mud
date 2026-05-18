pub mod components;
pub mod damage;
pub mod damage_expr;
pub mod damage_type;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::combat::damage::apply_pending_damage;
use crate::combat::resources::BattleTurnTimer;
use crate::combat::systems::{clear_invalid_combat_targets, resolve_battle_turn};
use crate::game::systems::process_game_commands;
use crate::magic::effects::tick_dot_effects;
use crate::npc::systems::update_roaming_npcs;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(BattleTurnTimer::default())
            .add_systems(
                Update,
                (clear_invalid_combat_targets, resolve_battle_turn)
                    .chain()
                    .after(process_game_commands)
                    .after(update_roaming_npcs)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                apply_pending_damage
                    .after(process_game_commands)
                    .after(resolve_battle_turn)
                    .after(update_roaming_npcs)
                    .after(tick_dot_effects)
                    .before(crate::game::projection::collect_game_events_from_authority)
                    .run_if(simulation_active),
            );
    }
}
