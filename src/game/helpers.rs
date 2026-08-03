use bevy::prelude::*;

use crate::player::components::{Player, PlayerId, PlayerIdentity};
use crate::world::components::{Collider, OverworldObject, SpaceId, SpaceResident, TilePosition};

pub type PlayerLookupQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PlayerIdentity,
        &'static SpaceResident,
        &'static TilePosition,
        &'static OverworldObject,
    ),
    With<Player>,
>;

pub type ColliderQuery<'w, 's> =
    Query<'w, 's, (&'static SpaceResident, &'static TilePosition), With<Collider>>;

pub fn player_space_id(player_entity: Entity, query: &PlayerLookupQuery) -> Option<SpaceId> {
    query.iter().find_map(|(entity, _, resident, _, _)| {
        (entity == player_entity).then_some(resident.space_id)
    })
}

pub fn colliders_in_space(space_id: SpaceId, query: &ColliderQuery) -> Vec<TilePosition> {
    query
        .iter()
        .filter_map(|(resident, tile_position)| {
            (resident.space_id == space_id).then_some(*tile_position)
        })
        .collect()
}

/// Raw XY/Z adjacency window — the *geometric half* of the reach rule.
/// Chebyshev-1 horizontally; `z` is allowed within ±2 (one full block) so the
/// player can reach items sitting on an adjacent barrel, a chest stacked on a
/// chest, or one full block down — i.e. the same vertical window as auto-climb.
///
/// **Call [`FloorGeometry::reachable`] instead** for anything acting on a world
/// object. `|dz| <= 2` is exactly one whole floor (`z` is in half-blocks), so
/// this predicate on its own happily reaches through a ceiling; `reachable`
/// pairs it with the floor-slab test that makes floors solid.
///
/// [`FloorGeometry::reachable`]: crate::world::column::FloorGeometry::reachable
pub fn is_near_player(player_position: &TilePosition, target_position: &TilePosition) -> bool {
    (player_position.z - target_position.z).abs() <= 2
        && (player_position.x - target_position.x).abs() <= 1
        && (player_position.y - target_position.y).abs() <= 1
}

/// How far a conversation carries, in tiles. Wider than the manipulation reach
/// (`is_near_player`) so a player doesn't have to stand nose-to-nose with an
/// NPC to open a dialog; matches the feel of the ambient chatter radius.
pub const TALK_RANGE_TILES: i32 = 3;

/// Geometric half of the *talk* rule — Chebyshev-[`TALK_RANGE_TILES`]
/// horizontally, same ±2 vertical window as [`is_near_player`].
///
/// **Call [`FloorGeometry::talk_reachable`] instead** when deciding whether a
/// dialog may start; it pairs this window with the floor-slab test.
///
/// [`FloorGeometry::talk_reachable`]: crate::world::column::FloorGeometry::talk_reachable
pub fn is_within_talk_range(
    player_position: &TilePosition,
    target_position: &TilePosition,
) -> bool {
    (player_position.z - target_position.z).abs() <= 2
        && (player_position.x - target_position.x).abs() <= TALK_RANGE_TILES
        && (player_position.y - target_position.y).abs() <= TALK_RANGE_TILES
}

/// Single funnel for "the client asked for something the server won't do".
///
/// Refusals are *silent on the wire* where a correct client could never have
/// produced the request (cross-floor reach, out-of-range placement): sending a
/// narrator line would only confuse a player whose client already knows better.
/// They are always logged, though — a steady trickle of `refused` lines means a
/// desynced, stale, or hand-crafted client, which is exactly the thing we want
/// to be able to grep for.
pub fn refuse(player_id: PlayerId, command: &str, reason: &str) {
    bevy::log::warn!(
        "refused player={} command={command} reason={reason}",
        player_id.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talk_range_is_wider_than_reach_but_bounded() {
        let origin = TilePosition::new(0, 0, 0);
        let at = |x, y, z| TilePosition::new(x, y, z);

        assert!(!is_near_player(&origin, &at(3, 0, 0)));
        assert!(is_within_talk_range(&origin, &at(3, 0, 0)));
        assert!(is_within_talk_range(&origin, &at(-3, 3, 0)));
        assert!(!is_within_talk_range(&origin, &at(4, 0, 0)));
        assert!(!is_within_talk_range(&origin, &at(0, -4, 0)));
        // Same vertical window as `is_near_player`.
        assert!(is_within_talk_range(&origin, &at(1, 1, 2)));
        assert!(!is_within_talk_range(&origin, &at(1, 1, 3)));
    }
}
