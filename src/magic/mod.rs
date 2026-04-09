pub mod resources;

use bevy::prelude::*;

use crate::magic::resources::SpellDefinitions;

pub struct MagicPlugin;

impl Plugin for MagicPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpellDefinitions::load_from_disk());
    }
}
