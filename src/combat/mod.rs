pub mod components;
pub mod damage;
pub mod damage_expr;
pub mod damage_type;
pub mod formulas;
pub mod modifiers;
pub mod npc_casting;
pub mod resources;
pub mod scheduled;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::{simulation_active, ClientAppState};
use crate::combat::damage::apply_pending_damage;
use crate::combat::modifiers::{tick_item_modifiers, ItemModifierTickTimer};
use crate::combat::resources::{BattleTurnTimer, PendingModifierConsumption, PendingNpcSummons};
use crate::combat::scheduled::{tick_scheduled_impacts, ScheduledImpacts};
use crate::combat::systems::{
    apply_pending_modifier_consumption, apply_pending_npc_summons, clear_invalid_combat_targets,
    resolve_battle_turn,
};
use crate::game::systems::process_game_commands;
use crate::magic::effects::tick_dot_effects;
use crate::npc::systems::update_roaming_npcs;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(BattleTurnTimer::default())
            .insert_resource(ItemModifierTickTimer::default())
            .insert_resource(PendingModifierConsumption::default())
            .init_resource::<PendingNpcSummons>()
            .init_resource::<ScheduledImpacts>()
            // Drop any in-flight missiles / pending AoE waves when leaving the
            // world, so they can't flash VFX into a freshly-loaded one.
            .add_systems(
                OnExit(ClientAppState::InGame),
                |mut scheduled: ResMut<ScheduledImpacts>| scheduled.items.clear(),
            )
            .add_systems(
                Update,
                (clear_invalid_combat_targets, resolve_battle_turn)
                    .chain()
                    .after(process_game_commands)
                    .after(update_roaming_npcs)
                    .run_if(simulation_active),
            )
            // Deferred spell resolution (missiles, patterned AoE). Must run
            // after the cast handlers push impacts and before the damage drain,
            // so delay-0 impacts still land the same frame.
            .add_systems(
                Update,
                tick_scheduled_impacts
                    .after(process_game_commands)
                    .before(apply_pending_damage)
                    .run_if(simulation_active),
            )
            // Boss adds. Must run after the cast queues them and before the
            // damage drain, so a summon that lands this turn is a real entity
            // by the time anything looks for targets.
            .add_systems(
                Update,
                apply_pending_npc_summons
                    .after(resolve_battle_turn)
                    .before(apply_pending_damage)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                apply_pending_modifier_consumption
                    .after(resolve_battle_turn)
                    .before(crate::game::projection::collect_game_events_from_authority)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                tick_item_modifiers
                    .before(crate::game::projection::collect_game_events_from_authority)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                apply_pending_damage
                    .after(process_game_commands)
                    .after(resolve_battle_turn)
                    .after(update_roaming_npcs)
                    .after(tick_dot_effects)
                    .after(tick_scheduled_impacts)
                    .before(crate::game::projection::collect_game_events_from_authority)
                    .run_if(simulation_active),
            );
    }
}
