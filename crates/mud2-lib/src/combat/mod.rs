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
            // retaliation records and crime reports, whose Entities would
            // dangle) when leaving the world, so they can't leak into a
            // freshly-loaded one.
            .add_systems(
                OnExit(ClientAppState::InGame),
                |mut scheduled: ResMut<ScheduledImpacts>,
                 mut retaliations: ResMut<PendingRetaliations>,
                 mut pending_crimes: ResMut<crate::npc::witness::PendingCrimes>,
                 mut crime_log: ResMut<crate::npc::witness::CrimeLog>,
                 mut pending_learns: ResMut<crate::npc::guilt::PendingCrimeLearns>,
                 mut pending_clears: ResMut<crate::npc::guilt::PendingGuiltClears>| {
                    scheduled.items.clear();
                    retaliations.items.clear();
                    pending_crimes.items.clear();
                    crime_log.clear();
                    pending_learns.items.clear();
                    pending_clears.items.clear();
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
            // Witnessed crimes: fold the reports the damage drain filed into
            // the lingering log NPCs sample on their AI ticks. Same anchor
            // rationale as the aggro/guilt systems below; no `NetServerSend`
            // edge because the log mutates no replicated state (crimes live
            // ~5s, so frame latency is irrelevant).
            .add_systems(
                Update,
                crate::npc::witness::update_crime_log
                    .after(apply_pending_damage)
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
            )
            // Guilt: gossip queues learns for nearby NPCs, then the memory
            // sweep drains every learn/clear queued this frame — by the crime
            // log (surviving victims), the AI tick (witnesses), gossip, the
            // death handler, and the judge. Registered here rather than in
            // `NpcPlugin` for the same reason as the aggro system above — the
            // `.after(...)` edges only bind inside the plugin that owns the
            // anchors. Before the projection so a guard that just learned
            // enough to want you dead replicates as hostile this frame.
            .add_systems(
                Update,
                (
                    crate::npc::guilt::tick_crime_gossip,
                    crate::npc::guilt::apply_crime_memory_updates,
                )
                    .chain()
                    .after(crate::npc::witness::update_crime_log)
                    .after(update_roaming_npcs)
                    .before(crate::network::sets::NetServerSend)
                    .run_if(simulation_active),
            );
    }
}
