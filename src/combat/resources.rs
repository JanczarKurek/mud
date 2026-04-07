use bevy::prelude::*;

#[derive(Resource)]
pub struct BattleTurnTimer {
    pub remaining_seconds: f32,
    pub interval_seconds: f32,
    pub disengage_distance_tiles: i32,
}

impl Default for BattleTurnTimer {
    fn default() -> Self {
        Self {
            remaining_seconds: 1.0,
            interval_seconds: 1.0,
            disengage_distance_tiles: 3,
        }
    }
}
