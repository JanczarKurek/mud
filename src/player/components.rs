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

impl VitalStats {
    pub const fn new(health: f32, max_health: f32, mana: f32, max_mana: f32) -> Self {
        Self {
            health,
            max_health,
            mana,
            max_mana,
        }
    }
}

#[derive(Component)]
pub struct BaseStats {
    pub max_health: i32,
    pub max_mana: i32,
    pub storage_slots: usize,
}

impl Default for BaseStats {
    fn default() -> Self {
        Self {
            max_health: 100,
            max_mana: 100,
            storage_slots: 8,
        }
    }
}

#[derive(Component)]
pub struct DerivedStats {
    pub max_health: i32,
    pub max_mana: i32,
    pub storage_slots: usize,
}

impl Default for DerivedStats {
    fn default() -> Self {
        let base = BaseStats::default();
        Self {
            max_health: base.max_health,
            max_mana: base.max_mana,
            storage_slots: base.storage_slots,
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
