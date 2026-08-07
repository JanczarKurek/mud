//! Generic time-to-live for transient world entities.
//!
//! Anything that should auto-despawn after a fixed duration (corpses,
//! spell-summoned objects, future timed pickups, ...) carries a `Ttl`. The
//! single `tick_ttl` system decrements `remaining_seconds` and despawns when
//! it hits zero. Persistence reads/writes it as the `remaining_ttl` field on
//! `WorldObjectStateDump`.

use bevy::prelude::*;

use crate::app::state::simulation_active;

#[derive(Component, Clone, Copy, Debug)]
pub struct Ttl {
    pub remaining_seconds: f32,
}

pub fn tick_ttl(mut commands: Commands, time: Res<Time>, mut query: Query<(Entity, &mut Ttl)>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (entity, mut ttl) in query.iter_mut() {
        ttl.remaining_seconds -= dt;
        if ttl.remaining_seconds <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

pub struct TtlPlugin;

impl Plugin for TtlPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, tick_ttl.run_if(simulation_active));
    }
}
