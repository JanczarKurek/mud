pub mod awareness;
pub mod projectile;
pub mod speech_bubble;
pub mod vfx;
pub mod vfx_attachment;

use bevy::prelude::*;

use crate::client_effects::awareness::{sync_awareness_markers, AwarenessMarkers};
use crate::client_effects::projectile::{advance_projectiles, consume_projectile_events};
use crate::client_effects::speech_bubble::{
    consume_speech_bubble_events, resize_speech_bubble_backdrops,
};
use crate::client_effects::vfx::consume_vfx_events;
use crate::client_effects::vfx_attachment::project_active_effects_to_attachments;

pub struct ClientEffectsPlugin;

impl Plugin for ClientEffectsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AwarenessMarkers>().add_systems(
            Update,
            (
                consume_projectile_events,
                consume_vfx_events,
                consume_speech_bubble_events,
                resize_speech_bubble_backdrops,
                project_active_effects_to_attachments,
                advance_projectiles,
                sync_awareness_markers,
            ),
        );
    }
}
