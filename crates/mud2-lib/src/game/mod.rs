#[cfg(feature = "server-sim")]
pub mod chat;
pub mod commands;
pub mod currency;
#[cfg(feature = "server-sim")]
pub mod discovery;
pub mod helpers;
pub mod projection;
pub mod resources;
pub mod shop;
#[cfg(feature = "server-sim")]
pub(crate) mod slots;
#[cfg(feature = "server-sim")]
pub mod systems;
pub mod trade;
pub mod traversal;

use bevy::prelude::*;

#[cfg(feature = "server-sim")]
use crate::app::state::simulation_active;
#[cfg(feature = "server-sim")]
use crate::combat::damage::PendingDamageEvents;
#[cfg(feature = "server-sim")]
use crate::combat::systems::resolve_battle_turn;
#[cfg(feature = "server-sim")]
use crate::game::chat::process_say_commands;
#[cfg(feature = "server-sim")]
use crate::game::discovery::{
    apply_pending_discovery, discover_around_players, PendingDiscoveryEvents,
};
use crate::game::projection::apply_game_events_to_client_state;
use crate::game::resources::{
    ClientGameState, ClientStateRevisions, PendingGameCommands, PendingGameEvents,
    PendingGameUiEvents,
};
#[cfg(feature = "server-sim")]
use crate::game::resources::{ContainerViewers, PlacementSeqCounter};
#[cfg(feature = "server-sim")]
use crate::game::systems::{
    process_floor_commands, process_game_commands, process_rotate_commands,
    tick_player_movement_cooldowns,
};
#[cfg(feature = "server-sim")]
use crate::game::trade::{cleanup_invalid_trades, process_trade_commands, ActiveTrades};
#[cfg(feature = "server-sim")]
use crate::npc::systems::update_roaming_npcs;
#[cfg(feature = "server-sim")]
use crate::player::systems::move_player_on_grid;
#[cfg(feature = "server-sim")]
use crate::world::hide_action::process_hide_commands;
#[cfg(feature = "server-sim")]
use crate::world::interactions::{
    process_interact_commands, sync_container_visual_state, tick_respawn_timers,
};

#[cfg(feature = "server-sim")]
pub struct GameServerPlugin;

pub struct GameClientPlugin;

/// Runs before `process_game_commands`. Plugins (e.g. dialog) that want to
/// drain specific `GameCommand` variants before the main processor sees them
/// should register their systems `.in_set(CommandIntercept)`.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, SystemSet)]
pub struct CommandIntercept;

#[cfg(feature = "server-sim")]
impl Plugin for GameServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingGameCommands::default())
            .insert_resource(crate::game::resources::ClientPendingCommands::default())
            .insert_resource(PendingGameEvents::default())
            .insert_resource(PendingGameUiEvents::default())
            .insert_resource(PendingDamageEvents::default())
            // Deferred spell impacts. `process_game_commands`' `CommandOutputs`
            // reads this, so it must exist wherever the game plugin runs (incl.
            // test apps that omit `CombatPlugin`, which also `init_resource`s it).
            .insert_resource(crate::combat::scheduled::ScheduledImpacts::default())
            .insert_resource(PendingDiscoveryEvents::default())
            .insert_resource(ClientGameState::default())
            .insert_resource(ClientStateRevisions::default())
            .insert_resource(ContainerViewers::default())
            .insert_resource(PlacementSeqCounter::default())
            .insert_resource(ActiveTrades::default())
            .insert_resource(crate::world::noise::PendingNoiseEvents::default())
            .insert_resource(crate::world::noise::NoiseField::default())
            .configure_sets(
                Update,
                CommandIntercept
                    .after(tick_player_movement_cooldowns)
                    .before(process_game_commands),
            )
            // Anchor the server's per-peer flush (`NetServerSend`, see
            // `network::sets`) after everything that mutates projected state
            // this frame. References to systems registered by other plugins
            // (npc, combat) are fine: they resolve when those plugins are
            // present and are inert no-ops in test apps that omit them.
            .configure_sets(
                Update,
                crate::network::sets::NetServerSend
                    .after(process_game_commands)
                    .after(sync_container_visual_state)
                    .after(tick_respawn_timers)
                    .after(update_roaming_npcs)
                    .after(resolve_battle_turn)
                    .after(apply_pending_discovery),
            )
            .add_systems(
                Update,
                tick_player_movement_cooldowns
                    .after(move_player_on_grid)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                process_rotate_commands
                    .in_set(CommandIntercept)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                process_say_commands
                    .in_set(CommandIntercept)
                    .run_if(simulation_active),
            )
            // Not gated on `simulation_active`: the only command this drains is
            // `EditorSetFloorTile`, which originates from `MapEditor` (where
            // simulation is paused).
            .add_systems(Update, process_floor_commands.in_set(CommandIntercept))
            .add_systems(
                Update,
                process_interact_commands
                    .in_set(CommandIntercept)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                process_hide_commands
                    .in_set(CommandIntercept)
                    .after(process_interact_commands)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                process_trade_commands
                    .in_set(CommandIntercept)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                cleanup_invalid_trades
                    .after(process_trade_commands)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                process_game_commands
                    .after(tick_player_movement_cooldowns)
                    .after(CommandIntercept)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                sync_container_visual_state
                    .after(process_game_commands)
                    .run_if(simulation_active),
            )
            .add_systems(
                Update,
                tick_respawn_timers
                    .after(process_game_commands)
                    .run_if(simulation_active),
            )
            // Decay the noise field and fold in this frame's emissions, after
            // the systems that produce noise (movement, interactions, combat).
            // NPCs sample the lingering field next tick — exact ordering vs
            // `update_roaming_npcs` is unimportant since noise lives ~1.5s.
            .add_systems(
                Update,
                crate::world::noise::update_noise_field
                    .after(process_game_commands)
                    .after(process_interact_commands)
                    .after(resolve_battle_turn)
                    .run_if(simulation_active),
            )
            // Player stealth sensing: refresh per-player NPC awareness reads
            // before the projection serializes them into world-object state.
            .add_systems(
                Update,
                crate::player::sense::tick_player_sense
                    .after(update_roaming_npcs)
                    .before(crate::network::sets::NetServerSend)
                    .run_if(simulation_active),
            )
            // Map discovery: publisher sweeps positions, single drainer
            // mutates each player's `DiscoveredTiles`. Sequenced so the
            // projection in the same tick already sees the new entries.
            .add_systems(
                Update,
                (
                    discover_around_players
                        .after(process_game_commands)
                        .after(update_roaming_npcs)
                        .run_if(simulation_active),
                    apply_pending_discovery
                        .after(discover_around_players)
                        .before(crate::network::sets::NetServerSend)
                        .run_if(simulation_active),
                ),
            )
            // Unconditional — mirrors GameClientPlugin so that WorldClientPlugin's
            // .after(apply_game_events_to_client_state) ordering resolves identically
            // in EmbeddedClient mode and TcpClient mode. The `NetClientReceive`
            // edge places the fold after the client-side transport poll so
            // loopback traffic lands in `ClientGameState` within the frame.
            .add_systems(
                Update,
                apply_game_events_to_client_state.after(crate::network::sets::NetClientReceive),
            );
    }
}

impl Plugin for GameClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PendingGameCommands::default())
            .insert_resource(crate::game::resources::ClientPendingCommands::default())
            .insert_resource(PendingGameEvents::default())
            .insert_resource(PendingGameUiEvents::default())
            .insert_resource(ClientGameState::default())
            .insert_resource(ClientStateRevisions::default())
            .add_systems(
                Update,
                apply_game_events_to_client_state.after(crate::network::sets::NetClientReceive),
            );
    }
}
