use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct TilePosition {
    pub x: i32,
    pub y: i32,
}

impl TilePosition {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Component)]
#[allow(dead_code)]
pub struct OverworldObject {
    pub definition_id: String,
}

#[derive(Component)]
pub struct Collider;

#[derive(Component)]
pub struct WorldVisual {
    pub z_index: f32,
}

#[derive(Component)]
pub struct Collectible;

#[derive(Component)]
pub struct Container {
    pub slots: Vec<Option<String>>,
}
