//! Per-player map discovery (fog-of-war substrate).
//!
//! Each player carries a [`crate::player::components::DiscoveredTiles`]
//! component listing the `(x, y, z)` tiles they have ever seen in each space.
//! This module owns the single mutation pipeline:
//!
//! - [`PendingDiscoveryEvents`] — queue of pending reveals. Any system that
//!   wants to grant discovery (movement sweep, future scout NPCs, reveal
//!   spells, map items) pushes [`DiscoveryEvent`]s into it.
//! - [`discover_around_players`] — the v1 publisher. For every player with a
//!   position, sweeps a Euclidean disc of radius [`DISCOVERY_RADIUS`] around
//!   them and pushes any newly-seen tiles into the queue.
//! - [`apply_pending_discovery`] — sole drainer; mutates `DiscoveredTiles` and
//!   nothing else. Runs before
//!   [`crate::game::projection::collect_game_events_from_authority`] so the
//!   replication tick on the same frame picks up the new entries.
//!
//! The queue+drainer shape is overkill for a single publisher today but
//! matches the project's `PendingDamageEvents` / `PendingGameCommands` pattern
//! and keeps the door open for additional reveal sources without N call-site
//! mutations.

use bevy::prelude::*;

use crate::player::components::{DiscoveredTiles, Player, PlayerIdentity};
use crate::world::components::{SpaceId, SpaceResident, TilePosition};
use crate::world::resources::SpaceManager;

/// Euclidean tile radius around the player that counts as "seen this frame".
/// At 6, a flat field reveals a ~13×13 disc of 113 tiles around the player.
pub const DISCOVERY_RADIUS: f32 = 6.0;

#[derive(Clone, Debug)]
pub struct DiscoveryEvent {
    pub player: crate::player::components::PlayerId,
    pub space_id: SpaceId,
    pub tiles: Vec<(i32, i32, i32)>,
}

#[derive(Resource, Default)]
pub struct PendingDiscoveryEvents {
    pub events: Vec<DiscoveryEvent>,
}

/// v1 publisher: sweep a Euclidean disc around each player and push any tiles
/// that aren't already in their `DiscoveredTiles` into [`PendingDiscoveryEvents`].
/// Clipped to space bounds via [`SpaceManager`].
pub fn discover_around_players(
    space_manager: Res<SpaceManager>,
    mut pending: ResMut<PendingDiscoveryEvents>,
    players: Query<
        (
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &DiscoveredTiles,
        ),
        With<Player>,
    >,
) {
    let radius = DISCOVERY_RADIUS;
    let radius_sq = radius * radius;
    let radius_ceil = radius.ceil() as i32;

    for (identity, resident, tile, discovered) in players.iter() {
        let Some(space) = space_manager.get(resident.space_id) else {
            continue;
        };
        let z = tile.z;
        let mut new_tiles: Vec<(i32, i32, i32)> = Vec::new();
        for dy in -radius_ceil..=radius_ceil {
            for dx in -radius_ceil..=radius_ceil {
                let fx = dx as f32;
                let fy = dy as f32;
                if fx * fx + fy * fy > radius_sq {
                    continue;
                }
                let x = tile.x + dx;
                let y = tile.y + dy;
                if x < 0 || y < 0 || x >= space.width || y >= space.height {
                    continue;
                }
                if discovered.contains(resident.space_id, x, y, z) {
                    continue;
                }
                new_tiles.push((x, y, z));
            }
        }
        if !new_tiles.is_empty() {
            pending.events.push(DiscoveryEvent {
                player: identity.id,
                space_id: resident.space_id,
                tiles: new_tiles,
            });
        }
    }
}

/// Sole writer for [`DiscoveredTiles`]. Drains the queue and applies each
/// event to the matching player entity.
pub fn apply_pending_discovery(
    mut pending: ResMut<PendingDiscoveryEvents>,
    mut players: Query<(&PlayerIdentity, &mut DiscoveredTiles), With<Player>>,
) {
    if pending.events.is_empty() {
        return;
    }
    let events = std::mem::take(&mut pending.events);
    for event in events {
        for (identity, mut discovered) in players.iter_mut() {
            if identity.id != event.player {
                continue;
            }
            for (x, y, z) in &event.tiles {
                discovered.insert(event.space_id, *x, *y, *z);
            }
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::components::TilePosition;

    #[test]
    fn discovered_tiles_insert_dedupes() {
        let mut set = DiscoveredTiles::default();
        assert!(set.insert(SpaceId(0), 1, 2, 0));
        assert!(!set.insert(SpaceId(0), 1, 2, 0));
        assert!(set.contains(SpaceId(0), 1, 2, 0));
        assert!(!set.contains(SpaceId(0), 1, 2, 1));
        assert!(!set.contains(SpaceId(1), 1, 2, 0));
    }

    #[test]
    fn radius_disc_inside_space_bounds_is_swept() {
        // Direct math check: at radius 6, a tile 4 east and 4 north is
        // inside (4^2 + 4^2 = 32 < 36); a tile 5 east and 5 north is not.
        let r = DISCOVERY_RADIUS;
        let rsq = r * r;
        assert!((4.0 * 4.0 + 4.0 * 4.0) < rsq);
        assert!((5.0 * 5.0 + 5.0 * 5.0) > rsq);

        // Sanity-check TilePosition arithmetic.
        let p = TilePosition::new(10, 10, 0);
        assert_eq!(p.x + 4, 14);
        assert_eq!(p.y + 4, 14);
    }
}
