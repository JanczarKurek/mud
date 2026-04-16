pub mod components;
pub mod systems;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::npc::systems::update_roaming_npcs;

pub struct NpcPlugin;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_roaming_npcs.run_if(simulation_active));
    }
}
