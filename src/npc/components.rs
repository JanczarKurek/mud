use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Npc;

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamingBehavior {
    pub bounds: RoamBounds,
    pub step_interval_seconds: f32,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct HostileBehavior {
    pub detect_distance_tiles: i32,
    pub disengage_distance_tiles: i32,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamingStepTimer {
    pub remaining_seconds: f32,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamingRandomState {
    pub seed: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct RoamBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl RoamBounds {
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}
