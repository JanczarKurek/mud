use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::damage_type::DamageType;

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
    pub damage_type: DamageType,
}

impl AttackProfile {
    pub const fn melee() -> Self {
        Self {
            kind: AttackKind::Melee,
            damage_type: DamageType::Blunt,
        }
    }

    pub const fn ranged(range_tiles: i32) -> Self {
        Self {
            kind: AttackKind::Ranged { range_tiles },
            damage_type: DamageType::Pierce,
        }
    }

    pub const fn melee_with(damage_type: DamageType) -> Self {
        Self {
            kind: AttackKind::Melee,
            damage_type,
        }
    }

    pub const fn ranged_with(range_tiles: i32, damage_type: DamageType) -> Self {
        Self {
            kind: AttackKind::Ranged { range_tiles },
            damage_type,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AttackKind {
    Melee,
    Ranged { range_tiles: i32 },
}
