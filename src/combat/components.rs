use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct CombatTarget {
    pub entity: Entity,
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct CombatLeash {
    pub max_distance_tiles: i32,
}

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttackKind {
    Melee,
}
