//! Client-side presentation hook: when the local player has an active
//! `Glimmer` magical effect, expand the radius/intensity of their `LightSource`
//! component. When the effect fades, restore the baseline.
//!
//! Server-authoritative inputs (`MagicEffects` → projected
//! `ClientGameState.active_effects`) flow through the standard event pipeline,
//! so this system runs identically in EmbeddedClient and TcpClient modes.

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::magic::resources::EffectKind;
use crate::player::components::Player;
use crate::world::lighting::LightSource;

/// Baseline values mirrored from `spawn_player_visual` so toggling Glimmer
/// off restores the player to exactly the same halo they had at spawn.
const BASELINE_COLOR: [f32; 3] = [1.0, 0.92, 0.78];
const BASELINE_RADIUS: f32 = 1.5;
const BASELINE_INTENSITY: f32 = 0.18;

/// While a `Glimmer` effect is active, raise the player's halo to its
/// magnitude (tile radius) at a much brighter intensity. Otherwise, restore
/// the spawn-time baseline.
pub fn sync_player_glimmer_light(
    client_state: Res<ClientGameState>,
    mut player_query: Query<&mut LightSource, With<Player>>,
) {
    let Ok(mut light) = player_query.single_mut() else {
        return;
    };

    let glimmer = client_state
        .active_effects
        .iter()
        .find(|e| e.kind == EffectKind::Glimmer && e.remaining_seconds > 0.0);

    let (radius, intensity) = match glimmer {
        Some(effect) => (effect.magnitude.max(BASELINE_RADIUS), 1.0),
        None => (BASELINE_RADIUS, BASELINE_INTENSITY),
    };

    if (light.radius - radius).abs() > 1e-3 || (light.intensity - intensity).abs() > 1e-3 {
        light.radius = radius;
        light.intensity = intensity;
        light.color = BASELINE_COLOR;
    }
}
