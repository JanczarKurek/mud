use bevy::prelude::*;

use crate::player::components::{Player, PlayerIdentity};
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

/// Adjacency check used by every "do something to a nearby object" command
/// (move-item, open container, rotate, interact). Chebyshev-1 on the same
/// floor.
pub fn is_near_player(player_position: &TilePosition, target_position: &TilePosition) -> bool {
    player_position.z == target_position.z
        && (player_position.x - target_position.x).abs() <= 1
        && (player_position.y - target_position.y).abs() <= 1
}
