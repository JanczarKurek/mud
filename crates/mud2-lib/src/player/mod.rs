#[cfg(feature = "server-sim")]
pub mod admin_progression;
pub mod check;
pub mod classes;
pub mod components;
#[cfg(feature = "server-sim")]
pub mod debug_presets;
pub mod exertion;
#[cfg(feature = "server-sim")]
pub mod lifecycle;
#[cfg(feature = "server-sim")]
pub mod loadout;
pub mod progression;
#[cfg(feature = "server-sim")]
pub mod regen;
#[cfg(feature = "server-sim")]
pub mod sense;
pub mod setup;
pub mod skills;
pub mod systems;

use bevy::prelude::*;

#[cfg(feature = "server-sim")]
use crate::app::state::simulation_active;
use crate::app::state::ClientAppState;
#[cfg(feature = "server-sim")]
use crate::player::admin_progression::{
    process_admin_progression_commands, process_admin_toggle_commands,
};
#[cfg(feature = "server-sim")]
use crate::player::exertion::tick_exertion;
#[cfg(feature = "server-sim")]
use crate::player::lifecycle::{
    handle_player_deaths, handle_set_home_commands, process_acknowledge_death_commands,
    PendingPlayerDeaths,
};
#[cfg(feature = "server-sim")]
use crate::player::progression::{apply_xp_grants, PendingXpGrants};
#[cfg(feature = "server-sim")]
use crate::player::regen::{tick_regen_buffs, tick_vital_regen};
use crate::player::setup::{
    apply_player_appearance, propagate_player_animation_to_layers, spawn_player_recolor_layers,
    spawn_player_visual, swap_player_visual_on_class_change,
};
#[cfg(feature = "server-sim")]
use crate::player::skills::process_allocate_skill_commands;
#[cfg(feature = "server-sim")]
use crate::player::systems::refresh_derived_player_stats;
use crate::player::systems::{
    move_player_on_grid, rotate_nearby_object_on_shortcut, set_home_on_keypress,
    sync_player_appearance_from_client_state, sync_player_death_visibility,
    sync_projected_player_from_client_state, toggle_auto_retaliate_on_keypress,
    toggle_aware_on_keypress, toggle_sneak_on_keypress,
};

#[cfg(feature = "server-sim")]
pub struct PlayerServerPlugin;

/// Startup cross-check: panic on any loadout that references a missing object
/// type_id or equips an item into the wrong slot. Runs after
/// `OverworldObjectDefinitions` is inserted so the registry is populated —
/// same posture as `validate_recipes_against_objects`.
#[cfg(feature = "server-sim")]
fn validate_loadouts_against_objects(
    loadouts: Res<crate::player::loadout::Loadouts>,
    objects: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
) {
    loadouts.validate_against(&objects);
}

pub struct PlayerClientPlugin;

#[cfg(feature = "server-sim")]
impl Plugin for PlayerServerPlugin {
    fn build(&self, app: &mut App) {
        let loadouts = crate::player::loadout::Loadouts::load_from_disk();
        let debug_presets = crate::player::debug_presets::DebugCharacterPresets::load_from_disk();
        debug_presets.validate_against(&loadouts);
        app.init_resource::<PendingPlayerDeaths>()
            .init_resource::<PendingXpGrants>()
            .insert_resource(loadouts)
            .insert_resource(debug_presets)
            .add_systems(Startup, validate_loadouts_against_objects)
            .add_systems(Update, refresh_derived_player_stats)
            // `split_party_xp_grants` rewrites kill grants into per-member
            // shares between the damage drain (which tags them) and the bank.
            // Registered here rather than in GameServerPlugin so the
            // `.before(apply_xp_grants)` fn-edge stays same-plugin.
            .add_systems(
                Update,
                crate::game::party::split_party_xp_grants
                    .after(crate::combat::damage::apply_pending_damage)
                    .before(apply_xp_grants)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                apply_xp_grants
                    .after(crate::combat::systems::resolve_battle_turn)
                    .after(crate::combat::damage::apply_pending_damage)
                    .run_if(simulation_active),
            )
            // `tick_exertion` decays the fatigue meter; it runs before
            // `tick_vital_regen` so the regen penalty reads the post-decay value.
            .add_systems(
                Update,
                (tick_regen_buffs, tick_exertion, tick_vital_regen).run_if(simulation_active),
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
            // Ability-bump allocation: same CommandIntercept pattern.
            .add_systems(
                Update,
                crate::player::skills::process_allocate_ability_bump_commands
                    .in_set(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Admin progression mutations (grant XP, set level, etc.). Same
            // CommandIntercept pattern.
            .add_systems(
                Update,
                process_admin_progression_commands
                    .in_set(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Debug/GM marker toggles (god mode, noclip). Same CommandIntercept
            // pattern; separate system because it needs Commands + Entity.
            .add_systems(
                Update,
                process_admin_toggle_commands
                    .in_set(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Respawn acknowledgement: drains AcknowledgeDeath *before*
            // process_game_commands so the dead player's other commands are
            // still blocked when the main processor runs. Same CommandIntercept
            // pattern as handle_set_home_commands.
            .add_systems(
                Update,
                process_acknowledge_death_commands
                    .in_set(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Handle deaths after damage resolution. apply_pending_damage
            // fills PendingPlayerDeaths; this drains it. The .before edge on
            // the network flush guarantees the de-spatialization Commands are
            // applied before the same frame's projection, so the death frame
            // replicates HP-0 vitals + drained inventory + the death chat line
            // in one batch and the body vanishes from other peers immediately.
            .add_systems(
                Update,
                handle_player_deaths
                    .after(crate::combat::systems::resolve_battle_turn)
                    .after(crate::combat::damage::apply_pending_damage)
                    .before(crate::network::sets::NetServerSend)
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
                    sync_projected_player_from_client_state,
                    sync_player_death_visibility,
                )
                    .run_if(in_state(ClientAppState::InGame)),
            )
            // Player visual sync runs in both InGame and MapEditor so the
            // player's recolor layers stay in sync with the shared animation
            // atlas (idle frame cycles, customization tints) when entering
            // the editor. `PlayerLayersInitialized` makes re-attach
            // idempotent. Single registration avoids ambiguous SystemTypeSet
            // ordering errors that arise from per-state duplicated registration.
            .add_systems(
                Update,
                (
                    // Appearance lands on the stub before the layer spawner
                    // looks for it; the class swap replaces the base sprite
                    // before layers attach to a stale atlas.
                    sync_player_appearance_from_client_state.before(spawn_player_recolor_layers),
                    swap_player_visual_on_class_change.before(spawn_player_recolor_layers),
                    spawn_player_recolor_layers
                        .after(crate::world::animation::attach_animated_sprite),
                    propagate_player_animation_to_layers
                        .after(crate::world::animation::trigger_movement_animation)
                        .after(crate::world::animation::return_to_idle_animation),
                    apply_player_appearance.after(spawn_player_recolor_layers),
                )
                    .run_if(crate::world::in_game_or_editor),
            )
            .add_systems(
                Update,
                (
                    move_player_on_grid,
                    rotate_nearby_object_on_shortcut,
                    set_home_on_keypress,
                    toggle_sneak_on_keypress,
                    toggle_aware_on_keypress,
                    toggle_auto_retaliate_on_keypress,
                )
                    // Before the client outbox flush so a keypress crosses the
                    // wire (and, on loopback, lands on screen) the same frame.
                    .before(crate::network::sets::NetClientSend)
                    .run_if(in_state(ClientAppState::InGame))
                    .run_if(bevy_terminal::terminal_not_focused),
            );
    }
}
