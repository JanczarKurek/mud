pub mod effects;
pub mod glimmer;
pub mod resources;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::magic::effects::tick_magic_effects;
use crate::magic::glimmer::sync_player_glimmer_light;
use crate::magic::resources::SpellDefinitions;

pub struct MagicPlugin;

impl Plugin for MagicPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpellDefinitions::load_from_disk())
            .add_systems(Update, tick_magic_effects.run_if(simulation_active))
            // Client-side presentation: runs unconditionally so the buff
            // override is visible in both EmbeddedClient (where the player
            // entity carries `Player`) and TcpClient mode.
            .add_systems(Update, sync_player_glimmer_light);
    }
}
