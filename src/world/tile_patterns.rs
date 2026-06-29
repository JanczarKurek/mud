//! Pure tile-pattern geometry for patterned AoE spells.
//!
//! These helpers turn a center tile + radius into ordered tile lists that the
//! spell system maps to staggered impact delays:
//! - [`chebyshev_ring_tiles`] drives **spread** patterns (fire blooming
//!   ring-by-ring outward from the center).
//! - [`spiral_tiles`] drives **spiral** patterns (electricity arcing through
//!   tiles one at a time).
//!
//! All output stays on the center's `z` (planar at the target floor) and uses
//! Chebyshev (chessboard) geometry to match the rest of the grid model.

use crate::world::components::TilePosition;

/// The tiles on the Chebyshev ring at distance `r` from `center`, i.e. every
/// tile where `max(|dx|, |dy|) == r`.
///
/// - `r == 0` returns just the center (1 tile).
/// - `r > 0` returns the `8 * r`-tile square perimeter, walked clockwise
///   starting from the top-left corner. Ordering within a ring is unspecified
///   by callers (spread fires a whole ring at once), but is deterministic.
///
/// The full filled disk of radius `R` is the union of rings `0..=R`, with the
/// center appearing exactly once.
pub fn chebyshev_ring_tiles(center: TilePosition, r: i32) -> Vec<TilePosition> {
    let r = r.max(0);
    if r == 0 {
        return vec![center];
    }
    let mut tiles = Vec::with_capacity((8 * r) as usize);
    // Top and bottom rows (full width).
    for dx in -r..=r {
        tiles.push(TilePosition::new(center.x + dx, center.y + r, center.z));
        tiles.push(TilePosition::new(center.x + dx, center.y - r, center.z));
    }
    // Left and right columns (excluding the corners already emitted above).
    for dy in (-r + 1)..r {
        tiles.push(TilePosition::new(center.x - r, center.y + dy, center.z));
        tiles.push(TilePosition::new(center.x + r, center.y + dy, center.z));
    }
    tiles
}

/// Tiles of the filled Chebyshev disk of `radius` around `center`, ordered as a
/// square (Ulam) spiral starting at the center and walking outward.
///
/// Produces exactly `(2 * radius + 1)^2` tiles with no overshoot — a square
/// spiral of side `2 * radius + 1` fills the disk precisely. `radius == 0`
/// yields a single tile (the center). `clockwise` flips the turn handedness.
pub fn spiral_tiles(center: TilePosition, radius: i32, clockwise: bool) -> Vec<TilePosition> {
    let radius = radius.max(0);
    let side = 2 * radius + 1;
    let total = (side * side) as usize;
    let mut tiles = Vec::with_capacity(total);

    // Walk a spiral on the (dx, dy) offset lattice. Step lengths grow as
    // 1,1,2,2,3,3,... and we turn after each run. Counter-clockwise turn order
    // is E, N, W, S; clockwise mirrors the y component (E, S, W, N).
    let dirs: [(i32, i32); 4] = if clockwise {
        [(1, 0), (0, -1), (-1, 0), (0, 1)]
    } else {
        [(1, 0), (0, 1), (-1, 0), (0, -1)]
    };

    let (mut dx, mut dy) = (0, 0);
    tiles.push(TilePosition::new(center.x, center.y, center.z));

    let mut dir_idx = 0usize;
    let mut run_len = 1i32;
    while tiles.len() < total {
        // Two runs of the same length (the classic spiral cadence), then grow.
        for _ in 0..2 {
            let (sx, sy) = dirs[dir_idx % 4];
            for _ in 0..run_len {
                dx += sx;
                dy += sy;
                // The growing spiral steps onto the bounding square's edge
                // before the disk is full; clamp emission to the radius so we
                // never return out-of-disk tiles.
                if dx.abs() <= radius && dy.abs() <= radius {
                    tiles.push(TilePosition::new(center.x + dx, center.y + dy, center.z));
                    if tiles.len() == total {
                        return tiles;
                    }
                }
            }
            dir_idx += 1;
        }
        run_len += 1;
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::components::tile_distance_xy;
    use std::collections::HashSet;

    fn center() -> TilePosition {
        TilePosition::new(10, 20, 4)
    }

    #[test]
    fn ring_zero_is_center_only() {
        assert_eq!(chebyshev_ring_tiles(center(), 0), vec![center()]);
    }

    #[test]
    fn ring_r_has_8r_tiles_all_at_distance_r() {
        let c = center();
        for r in 1..=5 {
            let ring = chebyshev_ring_tiles(c, r);
            assert_eq!(ring.len() as i32, 8 * r, "ring {r} tile count");
            // No duplicates.
            let unique: HashSet<_> = ring.iter().copied().collect();
            assert_eq!(unique.len(), ring.len(), "ring {r} has duplicates");
            for tile in &ring {
                assert_eq!(tile.z, c.z, "ring stays planar");
                assert_eq!(
                    tile_distance_xy(*tile, c),
                    r,
                    "tile {tile:?} not on ring {r}"
                );
            }
        }
    }

    #[test]
    fn rings_union_to_filled_disk() {
        let c = center();
        let radius = 4;
        let mut union: HashSet<TilePosition> = HashSet::new();
        for r in 0..=radius {
            for tile in chebyshev_ring_tiles(c, r) {
                assert!(union.insert(tile), "tile {tile:?} appeared in two rings");
            }
        }
        let side = 2 * radius + 1;
        assert_eq!(union.len() as i32, side * side);
    }

    #[test]
    fn spiral_radius_zero_is_center_only() {
        assert_eq!(spiral_tiles(center(), 0, false), vec![center()]);
    }

    #[test]
    fn spiral_fills_disk_exactly_starting_at_center() {
        let c = center();
        for radius in 1..=5 {
            let spiral = spiral_tiles(c, radius, false);
            let side = 2 * radius + 1;
            assert_eq!(spiral.len() as i32, side * side, "spiral {radius} count");
            assert_eq!(spiral[0], c, "spiral starts at center");
            // Every tile is unique and within the Chebyshev disk.
            let unique: HashSet<_> = spiral.iter().copied().collect();
            assert_eq!(unique.len(), spiral.len(), "spiral {radius} duplicates");
            for tile in &spiral {
                assert!(
                    tile_distance_xy(*tile, c) <= radius,
                    "tile {tile:?} outside disk {radius}"
                );
                assert_eq!(tile.z, c.z, "spiral stays planar");
            }
        }
    }

    #[test]
    fn spiral_clockwise_reverses_handedness() {
        let c = center();
        let ccw = spiral_tiles(c, 2, false);
        let cw = spiral_tiles(c, 2, true);
        // Same footprint, same center, same first step (East). Handedness
        // diverges at the second step: counter-clockwise turns north, clockwise
        // turns south.
        let ccw_set: HashSet<_> = ccw.iter().copied().collect();
        let cw_set: HashSet<_> = cw.iter().copied().collect();
        assert_eq!(ccw_set, cw_set, "same footprint");
        assert_eq!(ccw[1], cw[1], "first step is East for both");
        assert_ne!(ccw[2], cw[2], "handedness differs at the second step");
        assert_ne!(ccw, cw, "overall ordering differs");
    }
}
