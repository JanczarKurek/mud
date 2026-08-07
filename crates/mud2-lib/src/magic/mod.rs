pub mod effects;
pub mod glimmer;
pub mod resources;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::magic::effects::{tick_dot_effects, tick_magic_effects};
use crate::magic::glimmer::sync_player_glimmer_light;
use crate::magic::resources::SpellDefinitions;

/// Server-side magic systems: tick effect durations, accumulate DoT damage.
/// `tick_dot_effects` writes to `PendingDamageEvents`, which is only inserted
/// by `GameServerPlugin` — so this plugin must NOT be added in TcpClient mode.
pub struct MagicServerPlugin;

impl Plugin for MagicServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpellDefinitions::load_from_disk())
            .add_systems(
                Update,
                (tick_magic_effects, tick_dot_effects)
                    .chain()
                    .run_if(simulation_active),
            );
    }
}

/// Client-side magic presentation: SpellDefinitions for UI lookups and the
/// Glimmer halo override.
pub struct MagicClientPlugin;

impl Plugin for MagicClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpellDefinitions::load_from_disk())
            // Runs unconditionally so the buff override is visible in both
            // EmbeddedClient (where the player entity carries `Player`) and
            // TcpClient mode.
            .add_systems(Update, sync_player_glimmer_light);
    }
}
