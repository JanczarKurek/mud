use bevy::prelude::*;

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct MovementCooldown {
    pub remaining_seconds: f32,
    pub step_interval_seconds: f32,
}

impl Default for MovementCooldown {
    fn default() -> Self {
        Self {
            remaining_seconds: 0.0,
            step_interval_seconds: 0.18,
        }
    }
}
