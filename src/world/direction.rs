use std::f32::consts::FRAC_PI_2;

use bevy::math::IVec2;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
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
    fn from_yaml_parses_common_forms() {
        assert_eq!(Direction::from_yaml("north"), Some(Direction::North));
        assert_eq!(Direction::from_yaml("N"), Some(Direction::North));
        assert_eq!(Direction::from_yaml(" east "), Some(Direction::East));
        assert_eq!(Direction::from_yaml("left"), Some(Direction::West));
        assert_eq!(Direction::from_yaml("nope"), None);
    }
}
