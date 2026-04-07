use bevy::prelude::*;

#[derive(Component)]
pub struct Npc;

#[derive(Component)]
pub struct RoamingBehavior {
    pub bounds: RoamBounds,
    pub step_interval_seconds: f32,
}

#[derive(Component)]
pub struct RoamingStepTimer {
    pub remaining_seconds: f32,
}

#[derive(Component)]
pub struct RoamingRandomState {
    pub seed: u64,
}

#[derive(Clone, Copy, Debug)]
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
