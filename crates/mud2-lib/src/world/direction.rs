use std::f32::consts::FRAC_PI_2;

use bevy::math::IVec2;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum Direction {
    North,
    #[default]
    South,
    East,
    West,
}

impl Direction {
    pub fn from_delta(dx: i32, dy: i32) -> Option<Self> {
        if dx == 0 && dy == 0 {
            return None;
        }
        if dx.abs() > dy.abs() {
            Some(if dx > 0 { Self::East } else { Self::West })
        } else {
            Some(if dy > 0 { Self::North } else { Self::South })
        }
    }

    pub fn to_delta(self) -> IVec2 {
        match self {
            Self::North => IVec2::new(0, 1),
            Self::South => IVec2::new(0, -1),
            Self::East => IVec2::new(1, 0),
            Self::West => IVec2::new(-1, 0),
        }
    }

    pub fn from_yaml(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "north" | "n" | "up" => Some(Self::North),
            "south" | "s" | "down" => Some(Self::South),
            "east" | "e" | "right" => Some(Self::East),
            "west" | "w" | "left" => Some(Self::West),
            _ => None,
        }
    }

    /// Z-axis rotation (radians) to apply to a sprite whose native pose faces south.
    /// South = 0, East = +π/2 (CCW), North = π, West = -π/2.
    pub fn rotation_z_radians(self) -> f32 {
        match self {
            Self::South => 0.0,
            Self::East => FRAC_PI_2,
            Self::North => std::f32::consts::PI,
            Self::West => -FRAC_PI_2,
        }
    }

    /// Rotate 90° clockwise in screen space. Matches the visual effect of
    /// `rotation_z_radians` decreasing by π/2 — i.e. South → West → North → East → South.
    pub fn turn_clockwise(self) -> Self {
        match self {
            Self::South => Self::West,
            Self::West => Self::North,
            Self::North => Self::East,
            Self::East => Self::South,
        }
    }

    /// Rotate 90° counter-clockwise in screen space. Inverse of `turn_clockwise`.
    pub fn turn_counter_clockwise(self) -> Self {
        match self {
            Self::South => Self::East,
            Self::East => Self::North,
            Self::North => Self::West,
            Self::West => Self::South,
        }
    }
}

/// Which corner of a building shell a wall-corner sprite sits on. A straight
/// wall is a single [`Direction`] face, but a corner spans two axes, so it
/// needs its own descriptor to drive the inside-fade and indoor-tint logic in
/// `sync_tile_transforms` (a single `Direction` cannot express both arms).
#[derive(Component, Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum WallCorner {
    Ne,
    Nw,
    Se,
    Sw,
}

impl WallCorner {
    /// Camera-facing (front) corners: their interior lies to the north, so they
    /// sit between the south-east camera and the room and must fade when the
    /// player is inside — exactly like the S/E straight walls they join. The
    /// back corners (NE/NW, interior to the south) stay opaque and tint instead.
    pub fn is_camera_facing(self) -> bool {
        matches!(self, Self::Se | Self::Sw)
    }

    /// Offset from the corner's tile to the interior cell diagonally behind it.
    /// The corner analogue of the perpendicular interior neighbour a straight
    /// wall checks; used to decide indoor tinting for the back (NE/NW) corners.
    pub fn interior_diagonal(self) -> (i32, i32) {
        match self {
            Self::Se => (-1, 1),
            Self::Sw => (1, 1),
            Self::Ne => (-1, -1),
            Self::Nw => (1, -1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_delta_cardinals() {
        assert_eq!(Direction::from_delta(1, 0), Some(Direction::East));
        assert_eq!(Direction::from_delta(-1, 0), Some(Direction::West));
        assert_eq!(Direction::from_delta(0, 1), Some(Direction::North));
        assert_eq!(Direction::from_delta(0, -1), Some(Direction::South));
    }

    #[test]
    fn from_delta_zero_returns_none() {
        assert_eq!(Direction::from_delta(0, 0), None);
    }

    #[test]
    fn from_delta_diagonal_prefers_horizontal_when_abs_dx_greater() {
        assert_eq!(Direction::from_delta(2, 1), Some(Direction::East));
        assert_eq!(Direction::from_delta(-3, 1), Some(Direction::West));
    }

    #[test]
    fn from_delta_diagonal_prefers_vertical_when_abs_dy_greater_or_equal() {
        assert_eq!(Direction::from_delta(1, 2), Some(Direction::North));
        assert_eq!(Direction::from_delta(1, -1), Some(Direction::South));
    }

    #[test]
    fn to_delta_roundtrip() {
        for dir in [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ] {
            let d = dir.to_delta();
            assert_eq!(Direction::from_delta(d.x, d.y), Some(dir));
        }
    }

    #[test]
    fn turn_clockwise_cycles_full_loop() {
        let mut d = Direction::South;
        for _ in 0..4 {
            d = d.turn_clockwise();
        }
        assert_eq!(d, Direction::South);
        assert_eq!(Direction::South.turn_clockwise(), Direction::West);
        assert_eq!(Direction::West.turn_clockwise(), Direction::North);
    }

    #[test]
    fn turn_counter_clockwise_is_inverse_of_clockwise() {
        for dir in [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ] {
            assert_eq!(dir.turn_clockwise().turn_counter_clockwise(), dir);
        }
    }

    #[test]
    fn from_yaml_parses_common_forms() {
        assert_eq!(Direction::from_yaml("north"), Some(Direction::North));
        assert_eq!(Direction::from_yaml("N"), Some(Direction::North));
        assert_eq!(Direction::from_yaml(" east "), Some(Direction::East));
        assert_eq!(Direction::from_yaml("left"), Some(Direction::West));
        assert_eq!(Direction::from_yaml("nope"), None);
    }

    #[test]
    fn wall_corner_front_corners_are_camera_facing() {
        // SE/SW (interior to the north) sit between the camera and the room.
        assert!(WallCorner::Se.is_camera_facing());
        assert!(WallCorner::Sw.is_camera_facing());
        // NE/NW are back corners — they tint instead of fading.
        assert!(!WallCorner::Ne.is_camera_facing());
        assert!(!WallCorner::Nw.is_camera_facing());
    }

    #[test]
    fn wall_corner_interior_diagonal_points_at_the_room() {
        // Each diagonal points to the interior cell behind the corner: SE's room
        // is to its NW, SW's to its NE, NE's to its SW, NW's to its SE.
        assert_eq!(WallCorner::Se.interior_diagonal(), (-1, 1));
        assert_eq!(WallCorner::Sw.interior_diagonal(), (1, 1));
        assert_eq!(WallCorner::Ne.interior_diagonal(), (-1, -1));
        assert_eq!(WallCorner::Nw.interior_diagonal(), (1, -1));
    }

    #[test]
    fn wall_corner_deserializes_from_lowercase_yaml() {
        assert_eq!(
            serde_yaml::from_str::<WallCorner>("se").unwrap(),
            WallCorner::Se
        );
        assert_eq!(
            serde_yaml::from_str::<WallCorner>("nw").unwrap(),
            WallCorner::Nw
        );
    }
}
