use bevy::prelude::*;

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct VitalStats {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
}

impl Default for VitalStats {
    fn default() -> Self {
        Self {
            health: 100.0,
            max_health: 100.0,
            mana: 65.0,
            max_mana: 100.0,
        }
    }
}

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
