use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CombatTarget {
    pub entity: Entity,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CombatLeash {
    pub max_distance_tiles: i32,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttackProfile {
    pub kind: AttackKind,
}

impl AttackProfile {
    pub const fn melee() -> Self {
        Self {
            kind: AttackKind::Melee,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AttackKind {
    Melee,
}
