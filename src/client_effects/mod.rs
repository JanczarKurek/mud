pub mod projectile;
pub mod vfx;
pub mod vfx_attachment;

use bevy::prelude::*;

use crate::client_effects::projectile::{advance_projectiles, consume_projectile_events};
use crate::client_effects::vfx::consume_vfx_events;
use crate::client_effects::vfx_attachment::project_active_effects_to_attachments;

pub struct ClientEffectsPlugin;

impl Plugin for ClientEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                consume_projectile_events,
                consume_vfx_events,
                project_active_effects_to_attachments,
                advance_projectiles,
            ),
        );
    }
}
