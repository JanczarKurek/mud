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

    pub const fn ranged(range_tiles: i32) -> Self {
        Self {
            kind: AttackKind::Ranged { range_tiles },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AttackKind {
    Melee,
    Ranged { range_tiles: i32 },
}
