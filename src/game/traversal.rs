//! Athletic traversal: climb / jump / fall mechanics shared between the
//! standard `MovePlayer` handler and the dedicated `JumpTo` handler.
//!
//! All `z` values are in half-block units (see `TilePosition`). A climb of 1
//! half-block is free (Tibia-style auto-step onto a chest). Anything taller
//! triggers an Athletics check whose DC rises with height. Falling more than
//! 2 half-blocks deals quadratic damage, halved by a successful Athletics save.
//! Jumps are issued via `JumpTo`; the effective cost is
//! `ceil(hypot(dx, dy)) + dz_up`, so a diagonal jump scales by true Euclidean
//! distance and leaping onto a tall ledge stacks vertical cost on top.

use bevy::prelude::*;

use crate::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use crate::combat::damage_type::DamageType;
use crate::player::components::{AttributeSet, ChatLog};
use crate::player::skills::{skill_check, Skill, SkillSheet};

/// Auto-climb height with no skill check. Stepping onto a half-block chest
/// or a `stair_*_low` keeps the old Tibia-style auto-step behaviour.
pub const CLIMB_FREE_DZ: i32 = 1;

/// Hard cap on attempted climbs (in half-blocks). Above this, the ledge is
/// unreachable on a single step regardless of Athletics.
pub const CLIMB_MAX_DZ: i32 = 4;

/// Maximum jump cost (`jump_cost` units) the handler will accept. Bounds both
/// XY range and the combined XY+upward-z cost — a 4-tile cardinal jump, a 1+3
/// half-block ledge-leap, and everything in between cap out here.
pub const JUMP_MAX_RANGE: i32 = 4;

/// Minimum jump cost. 0 is rejected (same tile); a 1-tile cardinal jump is
/// now legal — short hops aren't strictly walks since they still roll
/// Athletics and can trigger fall damage on the landing.
pub const JUMP_MIN_RANGE: i32 = 1;

/// Falls of 1 or 2 half-blocks (≤ 1 full block) are free; above this triggers
/// fall damage with an Athletics save to halve.
pub const FALL_THRESHOLD_DZ: i32 = 2;

/// Quadratic multiplier for fall damage: `FALL_DAMAGE_K * dz²` HP.
pub const FALL_DAMAGE_K: f32 = 1.5;

/// DC for an attempted climb of `dz` half-blocks. `dz = 2` (one full block,
/// e.g. a barrel) is DC 10; every additional half-block adds 5 to the DC.
pub const fn climb_dc(dz: i32) -> i32 {
    5 + 5 * (dz - 1)
}

/// Effective cost of a jump in tile-equivalent units. Horizontal distance is
/// Euclidean rounded up (so a 2-tile diagonal costs `ceil(√8) = 3`, harder
/// than a 2-tile cardinal at cost 2); each half-block of upward `z` adds one
/// unit. Downward `z` is free — gravity does the work — but the landing
/// still rolls a fall save.
pub fn jump_cost(dx: i32, dy: i32, dz_half: i32) -> i32 {
    let xy = (dx as f32).hypot(dy as f32).ceil() as i32;
    let up = dz_half.max(0);
    xy + up
}

/// DC for a jump with horizontal displacement `(dx, dy)` and upward `dz_half`
/// (half-blocks; downward z is free). DC scales by raw Euclidean distance:
/// `round((hypot(dx, dy) + max(0, dz_half)) * 5)`. Unlike [`jump_cost`] this
/// doesn't ceil the XY component, so a 1-tile diagonal is DC 7 (not 10) and
/// a 2-tile diagonal is DC 14 (not 15) — finer-grained than `jump_cost`'s
/// integer cap, which is still the right tool for the range/cost gate.
pub fn jump_dc(dx: i32, dy: i32, dz_half: i32) -> i32 {
    let xy = (dx as f32).hypot(dy as f32);
    let up = dz_half.max(0) as f32;
    ((xy + up) * 5.0).round() as i32
}

/// `jump_cost` with an extra obstacle-height term: the highest stack the arc
/// has to clear strictly between source and landing (half-blocks above the
/// source z, clamped to 0). When clearing nothing, this equals [`jump_cost`].
pub fn jump_cost_with_obstacle(dx: i32, dy: i32, dz_half: i32, obstacle_dz_half: i32) -> i32 {
    jump_cost(dx, dy, dz_half) + obstacle_dz_half.max(0)
}

/// `jump_dc` with an extra obstacle-height term — same arc-clearing meaning as
/// [`jump_cost_with_obstacle`], priced at 5 DC per half-block like the rest of
/// the jump cost.
pub fn jump_dc_with_obstacle(dx: i32, dy: i32, dz_half: i32, obstacle_dz_half: i32) -> i32 {
    jump_dc(dx, dy, dz_half) + obstacle_dz_half.max(0) * 5
}

/// DC for the reflexive Athletics save against fall damage. Tougher falls are
/// harder to absorb, scaling more gently than the damage itself.
pub const fn fall_save_dc(dz: i32) -> i32 {
    5 + 2 * dz
}

/// Quadratic fall damage in HP.
pub fn fall_damage(dz: i32) -> f32 {
    FALL_DAMAGE_K * (dz as f32).powi(2)
}

/// Outcome of `resolve_step_with_climb`: the resolved tile plus the climb /
/// fall deltas the caller uses to drive Athletics checks and fall damage.
#[derive(Clone, Copy, Debug)]
pub struct StepResolution {
    pub landed: crate::world::components::TilePosition,
    /// Half-blocks climbed upward (positive). 0 for lateral / falling steps.
    pub dz_climbed: i32,
    /// Half-blocks descended (positive). 0 for lateral / climbing steps.
    pub dz_fell: i32,
}

/// Integer tiles along the straight line from `from` (exclusive) to `to`
/// (inclusive), using Bresenham's algorithm. The straight-line path is what
/// `JumpTo` walks tile-by-tile to find the farthest reachable landing — it
/// degenerates to a single tile for adjacent jumps and to the cardinal /
/// diagonal run for axis-aligned targets. Returns an empty vec when `from == to`.
pub fn bresenham_line(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    if from == to {
        return Vec::new();
    }
    let (mut x, mut y) = from;
    let (tx, ty) = to;
    let dx = (tx - x).abs();
    let dy = -(ty - y).abs();
    let sx = (tx - x).signum();
    let sy = (ty - y).signum();
    let mut err = dx + dy;
    let mut out = Vec::new();
    loop {
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        out.push((x, y));
        if (x, y) == (tx, ty) {
            return out;
        }
    }
}

/// Move `(x, y)` one tile from `target` back toward `from`, used to compute
/// "land short" positions on a failed jump. Returns the same tile when
/// already at `from`.
pub fn step_back_toward(target: (i32, i32), from: (i32, i32)) -> (i32, i32) {
    let dx = (from.0 - target.0).signum();
    let dy = (from.1 - target.1).signum();
    (target.0 + dx, target.1 + dy)
}

/// Roll an Athletics save against `fall_save_dc(dz)`, halve damage on success,
/// and push a `DamageEvent` into the pending queue. Mirrors the standard
/// damage producer pattern — the damage system handles death, VFX, and
/// replication downstream.
pub fn apply_fall_damage(
    pending_damage: &mut PendingDamageEvents,
    chat_log: &mut ChatLog,
    target: Entity,
    sheet: &SkillSheet,
    attrs: &AttributeSet,
    dz: i32,
) {
    let dc = fall_save_dc(dz);
    let save = skill_check(sheet, attrs, Skill::Athletics, dc, 0);
    let mut amount = fall_damage(dz);
    if save.success {
        amount *= 0.5;
        chat_log.push_narrator(format!(
            "You roll with the impact (Athletics {} vs DC {}). You take {:.0} damage.",
            save.total, dc, amount
        ));
    } else {
        chat_log.push_narrator(format!(
            "You hit the ground hard (Athletics {} vs DC {}). You take {:.0} damage.",
            save.total, dc, amount
        ));
    }
    pending_damage.push(DamageEvent {
        target,
        amount,
        source: DamageSource::Environment,
        damage_type: DamageType::Blunt,
        vfx_override: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climb_dc_progression() {
        assert_eq!(climb_dc(2), 10);
        assert_eq!(climb_dc(3), 15);
        assert_eq!(climb_dc(4), 20);
    }

    #[test]
    fn jump_cost_uses_euclidean_xy_plus_upward_z() {
        // 1 tile cardinal: cost 1 (DC 5) — short hops are legal now.
        assert_eq!(jump_cost(1, 0, 0), 1);
        // 1 tile diagonal: ceil(√2) = 2 (DC 10).
        assert_eq!(jump_cost(1, 1, 0), 2);
        // 2 tiles cardinal: cost 2.
        assert_eq!(jump_cost(2, 0, 0), 2);
        // 2 tiles diagonal: ceil(√8) = 3 (was 2 under Chebyshev).
        assert_eq!(jump_cost(2, 2, 0), 3);
        // 1 tile horizontal + 2 half-blocks up: ceil(1) + 2 = 3 (DC 15).
        assert_eq!(jump_cost(1, 0, 2), 3);
        // 2 tiles horizontal + 1 half-block down: dz down is free, cost 2.
        assert_eq!(jump_cost(2, 0, -2), 2);
        // 4-cardinal sits at the cap.
        assert_eq!(jump_cost(4, 0, 0), 4);
        // 3-diagonal exceeds the cap (caller will reject it).
        assert_eq!(jump_cost(3, 3, 0), 5);
    }

    #[test]
    fn jump_dc_scales_continuously_with_euclidean_distance() {
        // Cardinal jumps land on the same DCs as before (every 5).
        assert_eq!(jump_dc(2, 0, 0), 10);
        assert_eq!(jump_dc(3, 0, 0), 15);
        assert_eq!(jump_dc(4, 0, 0), 20);
        // Diagonal jumps fill in between: √2 ≈ 1.41 → DC 7, √8 ≈ 2.83 → DC 14.
        assert_eq!(jump_dc(1, 1, 0), 7);
        assert_eq!(jump_dc(2, 2, 0), 14);
        // Upward z is integer half-blocks of cost; 1 east + 2 up → (1+2)*5 = 15.
        assert_eq!(jump_dc(1, 0, 2), 15);
        // Downward z is free for DC purposes.
        assert_eq!(jump_dc(2, 0, -2), 10);
    }

    #[test]
    fn fall_damage_is_quadratic() {
        assert_eq!(fall_damage(3) as i32, 13); // 1.5 * 9 = 13.5
        assert_eq!(fall_damage(4) as i32, 24); // 1.5 * 16
        assert_eq!(fall_damage(6) as i32, 54); // 1.5 * 36
    }

    #[test]
    fn jump_cost_with_obstacle_adds_obstacle_height() {
        // No obstacle == plain jump_cost.
        assert_eq!(jump_cost_with_obstacle(2, 0, 0, 0), jump_cost(2, 0, 0));
        // 2-tile cardinal jump over a 2-half-block wall: 2 + 2 = 4 (at cap).
        assert_eq!(jump_cost_with_obstacle(2, 0, 0, 2), 4);
        // Negative obstacle height is clamped to 0.
        assert_eq!(jump_cost_with_obstacle(2, 0, 0, -3), 2);
        // Obstacle stacks ON TOP of both XY and dz_up.
        assert_eq!(jump_cost_with_obstacle(1, 0, 1, 2), 4);
    }

    #[test]
    fn jump_dc_with_obstacle_adds_5_per_half_block() {
        assert_eq!(jump_dc_with_obstacle(2, 0, 0, 0), jump_dc(2, 0, 0));
        assert_eq!(jump_dc_with_obstacle(2, 0, 0, 2), 20); // 10 + 2*5
        assert_eq!(jump_dc_with_obstacle(1, 1, 0, 2), 17); // 7 + 10
        assert_eq!(jump_dc_with_obstacle(2, 0, 0, -3), 10);
    }

    #[test]
    fn bresenham_excludes_source_includes_target() {
        // Empty when source == target.
        assert!(bresenham_line((3, 3), (3, 3)).is_empty());
        // Cardinal: pure run of tiles.
        assert_eq!(bresenham_line((0, 0), (3, 0)), vec![(1, 0), (2, 0), (3, 0)]);
        // Diagonal: pure diagonal steps (no staircase).
        assert_eq!(bresenham_line((0, 0), (3, 3)), vec![(1, 1), (2, 2), (3, 3)]);
        // Shallow diagonal (2:1 slope) walks two tiles right per tile up.
        let line = bresenham_line((0, 0), (4, 2));
        assert_eq!(line.last(), Some(&(4, 2)));
        assert_eq!(line.len(), 4);
    }

    #[test]
    fn bresenham_handles_negative_directions() {
        // Going up-left from (3, 3) toward (0, 0).
        assert_eq!(bresenham_line((3, 3), (0, 0)), vec![(2, 2), (1, 1), (0, 0)]);
        // West-only.
        assert_eq!(
            bresenham_line((2, 5), (-1, 5)),
            vec![(1, 5), (0, 5), (-1, 5)]
        );
    }

    #[test]
    fn step_back_walks_one_tile_along_line() {
        // Diagonal jump: stepping back from (3, 3) toward (0, 0) lands at (2, 2).
        assert_eq!(step_back_toward((3, 3), (0, 0)), (2, 2));
        // Pure-x: (3, 0) toward (0, 0) -> (2, 0).
        assert_eq!(step_back_toward((3, 0), (0, 0)), (2, 0));
        // Already collocated: signum 0 means no movement.
        assert_eq!(step_back_toward((1, 1), (1, 1)), (1, 1));
    }
}
