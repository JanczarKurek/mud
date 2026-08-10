pub mod components;
#[cfg(feature = "server-sim")]
pub mod damage;
pub mod damage_expr;
pub mod damage_type;
#[cfg(feature = "server-sim")]
pub mod formulas;
pub mod modifiers;
#[cfg(feature = "server-sim")]
pub mod npc_casting;
#[cfg(feature = "server-sim")]
pub mod resources;
#[cfg(feature = "server-sim")]
pub mod scheduled;
#[cfg(feature = "server-sim")]
pub mod systems;

#[cfg(feature = "server-sim")]
use bevy::prelude::*;

#[cfg(feature = "server-sim")]
use crate::app::state::{simulation_active, ClientAppState};
#[cfg(feature = "server-sim")]
use crate::combat::damage::apply_pending_damage;
#[cfg(feature = "server-sim")]
use crate::combat::modifiers::{tick_item_modifiers, ItemModifierTickTimer};
#[cfg(feature = "server-sim")]
use crate::combat::resources::{
    BattleTurnTimer, PendingModifierConsumption, PendingNpcSummons, PendingRetaliations,
};
#[cfg(feature = "server-sim")]
use crate::combat::scheduled::{tick_scheduled_impacts, ScheduledImpacts};
#[cfg(feature = "server-sim")]
use crate::combat::systems::{
    apply_auto_retaliation, apply_pending_modifier_consumption, apply_pending_npc_summons,
    clear_invalid_combat_targets, resolve_battle_turn,
};
#[cfg(feature = "server-sim")]
use crate::game::systems::process_game_commands;
#[cfg(feature = "server-sim")]
use crate::magic::effects::tick_dot_effects;
#[cfg(feature = "server-sim")]
use crate::npc::systems::update_roaming_npcs;

#[cfg(feature = "server-sim")]
pub struct CombatPlugin;

#[cfg(feature = "server-sim")]
impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(BattleTurnTimer::default())
            .insert_resource(ItemModifierTickTimer::default())
            .insert_resource(PendingModifierConsumption::default())
            .init_resource::<PendingNpcSummons>()
            .init_resource::<PendingRetaliations>()
            .init_resource::<ScheduledImpacts>()
            // Drop any in-flight missiles / pending AoE waves (and stale
            // retaliation records, whose Entities would dangle) when leaving
            // the world, so they can't leak into a freshly-loaded one.
            .add_systems(
                OnExit(ClientAppState::InGame),
                |mut scheduled: ResMut<ScheduledImpacts>,
                 mut retaliations: ResMut<PendingRetaliations>| {
                    scheduled.items.clear();
                    retaliations.items.clear();
                },
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
            // Auto-retaliate: lock an attacker for target-less players in that
            // stance. Before the projection so the new target replicates the
            // same frame it was recorded.
            .add_systems(
                Update,
                apply_auto_retaliation
                    .after(resolve_battle_turn)
                    .before(crate::network::sets::NetServerSend)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                apply_pending_modifier_consumption
                    .after(resolve_battle_turn)
                    .before(crate::network::sets::NetServerSend)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                tick_item_modifiers
                    .before(crate::network::sets::NetServerSend)
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
                    .before(crate::network::sets::NetServerSend)
                    .run_if(simulation_active),
            )
            // Aggro-on-damage: surviving NPC victims lock onto their attacker.
            // After the damage drain (which pushes the events) and before the
            // projection, so a shot NPC replicates its new target this frame;
            // its actual first pursue step lands on the next AI tick.
            .add_systems(
                Update,
                crate::npc::aggro::apply_damage_aggro
                    .after(apply_pending_damage)
                    .before(crate::network::sets::NetServerSend)
                    .run_if(simulation_active),
            );
    }
}
