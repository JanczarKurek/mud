//! Noise propagation — a shared world signal for the stealth/detection web.
//!
//! Loud actions (walking, opening or forcing doors, mining, combat) push a
//! [`NoiseEvent`] into [`PendingNoiseEvents`]. Once per frame `update_noise_field`
//! decays the lingering [`NoiseField`] and folds in the new events. NPCs sample
//! the field: a noise they can hear pulls them into `AiState::Alert` at the noise
//! tile even with no line of sight (see `src/npc/systems.rs`).
//!
//! Server-authoritative and **never replicated** — this is pure simulation state.
//! It must not appear in `ClientGameState` or any `GameEvent` (CLAUDE.md
//! EmbeddedClient invariant). The field *lingers* (rather than being a one-frame
//! `Vec`) because NPC AI ticks on a per-NPC timer, so a noise made between an
//! NPC's ticks would otherwise be missed.
//!
//! `loudness` is the noise's audible radius in tiles: an NPC at Chebyshev
//! distance `d` from the noise tile hears it iff `d <= loudness`.

use bevy::prelude::*;

use crate::world::components::{SpaceId, TilePosition};

/// How long a noise lingers in the field before fully decaying. Sized so a
/// noise outlives the gap between an NPC's AI ticks. `[tunable]` —
/// `docs/utility_systems.md` §7.
pub const NOISE_LIFETIME_SECONDS: f32 = 1.5;

// Per-source loudness (audible radius in tiles). `[tunable]` — §7.
/// A normal footstep.
pub const WALK_NOISE: i32 = 6;
/// A sneaking footstep — faint, short radius.
pub const SNEAK_NOISE: i32 = 2;
/// Opening or closing a door.
pub const DOOR_NOISE: i32 = 5;
/// Forcing a lock (shoulder-barging) — loud.
pub const FORCE_LOCK_NOISE: i32 = 9;
/// Picking a lock — quiet, deliberate.
pub const PICK_LOCK_NOISE: i32 = 3;
/// Mining / chopping a resource node.
pub const MINE_NOISE: i32 = 7;
/// Shoving a heavy object across the floor — grinding and loud.
pub const PUSH_NOISE: i32 = 7;
/// A landed attack in melee/ranged combat — loudest.
pub const ATTACK_NOISE: i32 = 10;

/// A single noise emission. `loudness` is the audible radius in tiles.
#[derive(Clone, Copy, Debug)]
pub struct NoiseEvent {
    pub space_id: SpaceId,
    pub tile: TilePosition,
    pub loudness: i32,
}

/// Queue of noise emissions for this frame. Pushed by movement / interaction /
/// combat systems, drained into [`NoiseField`] by `update_noise_field`.
#[derive(Resource, Default)]
pub struct PendingNoiseEvents {
    pub events: Vec<NoiseEvent>,
}

impl PendingNoiseEvents {
    pub fn push(&mut self, space_id: SpaceId, tile: TilePosition, loudness: i32) {
        if loudness <= 0 {
            return;
        }
        self.events.push(NoiseEvent {
            space_id,
            tile,
            loudness,
        });
    }
}

/// A lingering noise in the world. `remaining_seconds` counts down to zero.
#[derive(Clone, Copy, Debug)]
struct ActiveNoise {
    space_id: SpaceId,
    tile: TilePosition,
    loudness: i32,
    remaining_seconds: f32,
}

/// Decaying set of recent noises that NPCs sample. Small (a handful of entries
/// at most), so a linear `Vec` is cheaper than a hashmap and dodges needing
/// `Hash`/`Eq` on `SpaceId`.
#[derive(Resource, Default)]
pub struct NoiseField {
    active: Vec<ActiveNoise>,
}

impl NoiseField {
    /// The loudest noise audible at `(space_id, tile)`, i.e. within its own
    /// `loudness` radius (Chebyshev). Returns the noise tile so the NPC can walk
    /// toward it. `None` if nothing is audible.
    pub fn loudest_audible(&self, space_id: SpaceId, tile: TilePosition) -> Option<TilePosition> {
        self.active
            .iter()
            .filter(|n| n.space_id == space_id)
            .filter(|n| chebyshev(n.tile, tile) <= n.loudness)
            .max_by_key(|n| n.loudness)
            .map(|n| n.tile)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.active.len()
    }
}

/// Chebyshev (king-move) distance between two tiles, ignoring `z`.
fn chebyshev(a: TilePosition, b: TilePosition) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// Decays the lingering [`NoiseField`] and folds in this frame's
/// [`PendingNoiseEvents`]. Server-only; gated on `simulation_active` at
/// registration so the map editor produces/keeps no noise.
pub fn update_noise_field(
    time: Res<Time>,
    mut field: ResMut<NoiseField>,
    mut pending: ResMut<PendingNoiseEvents>,
) {
    let dt = time.delta_secs();
    // Decay first so freshly-added noise keeps its full lifetime.
    if dt > 0.0 {
        for noise in &mut field.active {
            noise.remaining_seconds -= dt;
        }
        field.active.retain(|n| n.remaining_seconds > 0.0);
    }

    for event in pending.events.drain(..) {
        // Merge onto a colocated entry: keep the louder radius, refresh the timer.
        if let Some(existing) = field
            .active
            .iter_mut()
            .find(|n| n.space_id == event.space_id && n.tile == event.tile)
        {
            existing.loudness = existing.loudness.max(event.loudness);
            existing.remaining_seconds = NOISE_LIFETIME_SECONDS;
        } else {
            field.active.push(ActiveNoise {
                space_id: event.space_id,
                tile: event.tile,
                loudness: event.loudness,
                remaining_seconds: NOISE_LIFETIME_SECONDS,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(x: i32, y: i32) -> TilePosition {
        TilePosition { x, y, z: 0 }
    }

    #[test]
    fn audible_within_loudness_radius() {
        let mut field = NoiseField::default();
        field.active.push(ActiveNoise {
            space_id: SpaceId(1),
            tile: tile(0, 0),
            loudness: 5,
            remaining_seconds: 1.0,
        });
        // Within radius.
        assert_eq!(
            field.loudest_audible(SpaceId(1), tile(4, 0)),
            Some(tile(0, 0))
        );
        // Just outside radius.
        assert_eq!(field.loudest_audible(SpaceId(1), tile(6, 0)), None);
        // Wrong space.
        assert_eq!(field.loudest_audible(SpaceId(2), tile(1, 0)), None);
    }

    #[test]
    fn loudest_wins_when_multiple_audible() {
        let mut field = NoiseField::default();
        field.active.push(ActiveNoise {
            space_id: SpaceId(1),
            tile: tile(1, 0),
            loudness: 4,
            remaining_seconds: 1.0,
        });
        field.active.push(ActiveNoise {
            space_id: SpaceId(1),
            tile: tile(2, 0),
            loudness: 9,
            remaining_seconds: 1.0,
        });
        assert_eq!(
            field.loudest_audible(SpaceId(1), tile(0, 0)),
            Some(tile(2, 0))
        );
    }

    #[test]
    fn push_ignores_zero_loudness() {
        let mut pending = PendingNoiseEvents::default();
        pending.push(SpaceId(1), tile(0, 0), 0);
        pending.push(SpaceId(1), tile(0, 0), -3);
        assert!(pending.events.is_empty());
        pending.push(SpaceId(1), tile(0, 0), 5);
        assert_eq!(pending.events.len(), 1);
    }

    #[test]
    fn merge_keeps_louder_radius() {
        let mut field = NoiseField::default();
        field.active.push(ActiveNoise {
            space_id: SpaceId(1),
            tile: tile(0, 0),
            loudness: 3,
            remaining_seconds: 0.5,
        });
        // Simulate the merge branch of update_noise_field.
        let event = NoiseEvent {
            space_id: SpaceId(1),
            tile: tile(0, 0),
            loudness: 7,
        };
        let existing = field
            .active
            .iter_mut()
            .find(|n| n.space_id == event.space_id && n.tile == event.tile)
            .unwrap();
        existing.loudness = existing.loudness.max(event.loudness);
        existing.remaining_seconds = NOISE_LIFETIME_SECONDS;
        assert_eq!(field.len(), 1);
        assert_eq!(field.active[0].loudness, 7);
    }
}
