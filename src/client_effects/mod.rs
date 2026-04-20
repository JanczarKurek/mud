pub mod projectile;

use bevy::prelude::*;

use crate::client_effects::projectile::{advance_projectiles, consume_projectile_events};

pub struct ClientEffectsPlugin;

impl Plugin for ClientEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (consume_projectile_events, advance_projectiles));
    }
}
