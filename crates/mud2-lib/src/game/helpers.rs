use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::game::resources::{ChatLogState, InventoryState};
use crate::player::components::{MovementCooldown, Player, PlayerId, PlayerIdentity, VitalStats};
use crate::world::components::{
    Collider, Movable, OverworldObject, SpaceId, SpaceResident, TilePosition,
};

/// The full mutable view of a player acting on a server command — identity,
/// inventory, chat log, position and vitals. One shared alias instead of
/// re-spelling the nine-field tuple in every command handler.
pub type PlayerActorQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PlayerIdentity,
        &'static mut InventoryState,
        &'static mut ChatLogState,
        &'static mut SpaceResident,
        &'static mut TilePosition,
        &'static mut MovementCooldown,
        &'static mut VitalStats,
        Option<&'static CombatTarget>,
    ),
    With<Player>,
>;

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

/// Every non-player world object with its location — the standard lookup query
/// for command handlers resolving an `object_id`.
pub type WorldObjectQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static SpaceResident,
        &'static TilePosition,
        &'static OverworldObject,
    ),
    Without<Player>,
>;

/// Same view restricted to `Movable` objects (push/pull/pick-up targets).
pub type MovableObjectQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static SpaceResident,
        &'static TilePosition,
        &'static OverworldObject,
    ),
    (With<Movable>, Without<Player>),
>;

pub type ColliderQuery<'w, 's> =
    Query<'w, 's, (&'static SpaceResident, &'static TilePosition), With<Collider>>;

/// The `(BaseStats, SkillSheet)` lookup used by every Athletics-gated action
/// (jump, shove, climb).
pub type AthleticsQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static crate::player::components::BaseStats,
        &'static crate::player::skills::SkillSheet,
    ),
    With<Player>,
>;

/// Adapt the world-object query's rows into `ColumnMember`s for
/// `Column::from_world` / stack settling.
pub fn column_members<'a>(
    object_query: &'a WorldObjectQuery,
) -> impl Iterator<Item = crate::world::stacks::ColumnMember<'a>> + 'a {
    object_query.iter().map(
        |(entity, resident, tile, object)| crate::world::stacks::ColumnMember {
            entity,
            resident,
            tile,
            object,
        },
    )
}

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

/// Resolve the acting player's query row for a queued command: `Some(id)`
/// finds the matching player, `None` falls back to the first row — embedded
/// mode's single local player. Generic over the row tuple; `id_of` extracts
/// the `PlayerId` from a row.
pub fn resolve_acting_player<I: Iterator>(
    mut rows: I,
    player_id: Option<PlayerId>,
    id_of: impl Fn(&I::Item) -> PlayerId,
) -> Option<I::Item> {
    match player_id {
        Some(id) => rows.find(|row| id_of(row) == id),
        None => rows.next(),
    }
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
