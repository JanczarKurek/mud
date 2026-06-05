//! Server-side spatial indices shared by NPC pathing and combat resolution.
//!
//! Two HashSets are surfaced here:
//! * `MovementBlockers` — what an NPC can't *stand in*. Includes every collider
//!   tile expanded over its `block_size` (so a 2-half-block wall blocks both
//!   `z` and `z+1`) plus a pseudo-blocker at `z = floor_idx * 2 - 1` for any
//!   painted floor with `walkable_surface`. The pseudo-blocker is what makes
//!   the existing climb/descent logic in `npc::systems::resolve_npc_step` land
//!   an NPC at the upper-floor surface (`z = floor_idx * 2`) instead of
//!   falling through to z=0.
//! * `LosBlockers` — what a vision ray can't *pass through*. Strict superset
//!   of `MovementBlockers`: it adds the floor surface itself
//!   (`z = floor_idx * 2`) for any painted floor with `occludes_floor_above`.
//!   Without this, a player on floor 2 can shoot an enemy on floor 0 because
//!   the only blockers between them are walls (which are interior to the
//!   building footprint), not the ceiling tile.
//!
//! Both indices are rebuilt every server tick. The HashSet rebuild is O(N) in
//! the number of colliders + painted-floor cells — much cheaper than the
//! per-call linear scans the NPC system used to do.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::npc::components::Npc;
use crate::player::components::Player;
use crate::world::components::{Collider, OverworldObject, SpaceId, SpaceResident, TilePosition};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::floors::{floormap_tile_occludes, floormap_tile_walkable};
use crate::world::object_definitions::OverworldObjectDefinitions;

pub type BlockerIndex = HashSet<(SpaceId, TilePosition)>;

/// Collider query shared by all spatial-index builders.
pub type SpatialColliderQuery<'w, 's> = Query<
    'w,
    's,
    (&'static SpaceResident, &'static TilePosition, Option<&'static OverworldObject>),
    (With<Collider>, Without<Npc>),
>;

/// Same shape as `SpatialColliderQuery` but *without* the NPC exclusion — used
/// by combat, where we want every collider in the world to block sight,
/// including NPCs blocking each other.
pub type CombatColliderQuery<'w, 's> = Query<
    'w,
    's,
    (&'static SpaceResident, &'static TilePosition, Option<&'static OverworldObject>),
    (With<Collider>, Without<Player>),
>;

/// Inflate each collider over its definition's `block_size`. A wall with
/// `block_size: 2` at `z` occupies `(z, z+1)` and must show up in the index at
/// both half-blocks, otherwise NPCs treat the wall as 1 half-block tall.
fn inflate_blockers<'a, I>(colliders: I, definitions: Option<&OverworldObjectDefinitions>) -> BlockerIndex
where
    I: Iterator<Item = (&'a SpaceResident, &'a TilePosition, Option<&'a OverworldObject>)>,
{
    colliders
        .flat_map(|(resident, position, overworld_object)| {
            let extent = overworld_object
                .zip(definitions)
                .and_then(|(object, defs)| defs.get(&object.definition_id))
                .map(|def| def.render.block_size as i32)
                .unwrap_or(0)
                .max(1);
            let space = resident.space_id;
            let base = *position;
            (0..extent).map(move |dz| {
                (space, TilePosition::new(base.x, base.y, base.z + dz))
            })
        })
        .collect()
}

/// Walk every painted floor cell and push pseudo-blockers / occluders into the
/// two indices in one sweep. Shared by `build_indices` so the indices stay in
/// sync (a tile flipped to `walkable_surface` gains movement support; a tile
/// flipped to `occludes_floor_above` gains a vision occluder).
fn apply_floor_layer(
    blockers: &mut BlockerIndex,
    los_blockers: &mut BlockerIndex,
    floor_maps: &FloorMaps,
    floor_defs: &FloorTilesetDefinitions,
) {
    for (space_id, floor_idx, grid) in floor_maps.iter() {
        if floor_idx <= 0 {
            continue;
        }
        let surface_z = floor_idx * 2;
        let support_z = surface_z - 1;
        let (width, height) = grid.dimensions();
        for y in 0..height {
            for x in 0..width {
                if floormap_tile_walkable(Some(grid), floor_defs, x, y) {
                    blockers.insert((space_id, TilePosition::new(x, y, support_z)));
                    los_blockers.insert((space_id, TilePosition::new(x, y, support_z)));
                }
                if floormap_tile_occludes(Some(grid), floor_defs, x, y) {
                    los_blockers.insert((space_id, TilePosition::new(x, y, surface_z)));
                }
            }
        }
    }
}

/// Build both indices from world queries. NPC pathing uses both; combat-side
/// callers that only care about LoS can use [`build_los_blockers`] instead.
pub fn build_indices<'a, I>(
    colliders: I,
    definitions: Option<&OverworldObjectDefinitions>,
    floor_maps: Option<&FloorMaps>,
    floor_defs: Option<&FloorTilesetDefinitions>,
) -> (BlockerIndex, BlockerIndex)
where
    I: Iterator<Item = (&'a SpaceResident, &'a TilePosition, Option<&'a OverworldObject>)>,
{
    let blockers = inflate_blockers(colliders, definitions);
    let mut movement = blockers.clone();
    let mut los = blockers;
    if let (Some(maps), Some(defs)) = (floor_maps, floor_defs) {
        apply_floor_layer(&mut movement, &mut los, maps, defs);
    }
    (movement, los)
}

/// LoS-only variant. Combat doesn't care about movement support; it just needs
/// "can a vision ray reach this tile?". Building only the LoS index sidesteps
/// the extra `HashSet::clone` that `build_indices` does for the movement side.
pub fn build_los_blockers<'a, I>(
    colliders: I,
    definitions: Option<&OverworldObjectDefinitions>,
    floor_maps: Option<&FloorMaps>,
    floor_defs: Option<&FloorTilesetDefinitions>,
) -> BlockerIndex
where
    I: Iterator<Item = (&'a SpaceResident, &'a TilePosition, Option<&'a OverworldObject>)>,
{
    let mut blockers = inflate_blockers(colliders, definitions);
    if let (Some(maps), Some(defs)) = (floor_maps, floor_defs) {
        // Discard the movement-support tiles by routing them into a throwaway
        // set; only the LoS occluders need to land in `blockers`.
        let mut discard = BlockerIndex::new();
        apply_floor_layer(&mut discard, &mut blockers, maps, defs);
    }
    blockers
}

/// 3D line of sight across the voxel grid. Walks a parametric line from `from`
/// (exclusive) to `to` (exclusive) using the largest of `|dx|`, `|dy|`, and
/// `|dz|` (half-block units) as the step count, and tests each interpolated
/// (x, y, z) cell against `los_blockers`. Source and destination are treated
/// as non-blocking by themselves — only *strictly between* tiles block the
/// line.
pub fn has_line_of_sight(
    from: TilePosition,
    to: TilePosition,
    space_id: SpaceId,
    los_blockers: &BlockerIndex,
) -> bool {
    if from == to {
        return true;
    }
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dz = to.z - from.z;
    let steps = dx.abs().max(dy.abs()).max(dz.abs());
    if steps <= 1 {
        return true;
    }
    for step in 1..steps {
        let t = step as f64 / steps as f64;
        let x = from.x + (dx as f64 * t).round() as i32;
        let y = from.y + (dy as f64 * t).round() as i32;
        let z = from.z + (dz as f64 * t).round() as i32;
        let here = TilePosition::new(x, y, z);
        if los_blockers.contains(&(space_id, here)) {
            return false;
        }
    }
    true
}
