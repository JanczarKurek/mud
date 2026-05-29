use bevy::prelude::*;

use crate::world::components::{OverworldObject, SpaceId, SpaceResident, TilePosition};
use crate::world::object_definitions::OverworldObjectDefinitions;

/// One member of a tile column as seen by the stack helpers. The helpers
/// take an iterator of these so they're agnostic to Bevy `Query` filter
/// types (`With<...>`, `Without<...>`) — callers pass their own filtered
/// iterator built from whatever query they happen to hold.
pub struct ColumnMember<'a> {
    pub entity: Entity,
    pub resident: &'a SpaceResident,
    pub tile: &'a TilePosition,
    pub object: &'a OverworldObject,
}

/// `z` of the surface above the topmost block at column `(space, x, y)` —
/// i.e. the `z` a newly placed object's *feet* would occupy. Returns `0`
/// (ground) when no block-sized objects sit on the column. Half-blocks
/// contribute `+1` z, full blocks `+2`; flat (`block_size == 0`) objects
/// don't change the stack top.
pub fn stack_top_z<'a, I>(
    space: SpaceId,
    x: i32,
    y: i32,
    members: I,
    definitions: &OverworldObjectDefinitions,
) -> i32
where
    I: IntoIterator<Item = ColumnMember<'a>>,
{
    stack_top_z_excluding(space, x, y, Entity::PLACEHOLDER, members, definitions)
}

/// Same as [`stack_top_z`] but excludes `exclude` from the column. Used by
/// the drag-an-existing-object-onto-its-own-tile path so the moved entity
/// doesn't stack on top of itself.
pub fn stack_top_z_excluding<'a, I>(
    space: SpaceId,
    x: i32,
    y: i32,
    exclude: Entity,
    members: I,
    definitions: &OverworldObjectDefinitions,
) -> i32
where
    I: IntoIterator<Item = ColumnMember<'a>>,
{
    members
        .into_iter()
        .filter(|m| {
            m.entity != exclude && m.resident.space_id == space && m.tile.x == x && m.tile.y == y
        })
        .filter_map(|m| {
            let def = definitions.get(&m.object.definition_id)?;
            if def.render.block_size == 0 {
                return None;
            }
            Some(m.tile.z + def.render.block_size as i32)
        })
        .max()
        .unwrap_or(0)
}

/// True iff the topmost block at column `(space, x, y)` (excluding
/// `exclude`) has a walkable top — i.e. it's safe to stack onto. A column
/// with only flat objects, or no objects at all, returns true (drop onto
/// ground). A column whose top object is a wall returns false.
pub fn stack_top_is_walkable<'a, I>(
    space: SpaceId,
    x: i32,
    y: i32,
    exclude: Entity,
    members: I,
    definitions: &OverworldObjectDefinitions,
) -> bool
where
    I: IntoIterator<Item = ColumnMember<'a>>,
{
    let topmost = members
        .into_iter()
        .filter(|m| {
            m.entity != exclude && m.resident.space_id == space && m.tile.x == x && m.tile.y == y
        })
        .filter_map(|m| {
            let def = definitions.get(&m.object.definition_id)?;
            if def.render.block_size == 0 {
                return None;
            }
            Some((
                m.tile.z + def.render.block_size as i32,
                def.render.walkable_surface,
            ))
        })
        .max_by_key(|(top, _)| *top);
    topmost.map(|(_, walkable)| walkable).unwrap_or(true)
}

/// True iff a player at z=`player_z` can place a `placed_block_size`-tall
/// object onto a column whose current top is `current_stack_top_z`.
///
/// The single rule: the existing stack top must be within ±2 half-blocks
/// of the player's feet — i.e. the same reach window as auto-climb. If you
/// could step onto the top, you can place onto it. There is no extra cap
/// on the resulting stack height; to keep building higher, climb the stack
/// first (which raises your `player_z`, which in turn extends your reach).
pub fn can_place_on_stack(player_z: i32, current_stack_top_z: i32, _placed_block_size: u8) -> bool {
    (current_stack_top_z - player_z).abs() <= 2
}

/// Settle request: re-stack the block-sized objects at column `(space, x, y)`
/// so each rests on the one below with no gaps. `removed_entity` lets callers
/// exclude an entity that's about to despawn (or has already had its position
/// changed) in the *same* frame — Bevy commands haven't flushed yet, so the
/// query would still see it without an explicit filter.
#[derive(Clone, Copy, Debug)]
pub struct SettleStackEvent {
    pub space_id: SpaceId,
    pub x: i32,
    pub y: i32,
    pub removed_entity: Option<Entity>,
}

#[derive(Resource, Default)]
pub struct PendingStackSettleEvents {
    pub events: Vec<SettleStackEvent>,
}

impl PendingStackSettleEvents {
    pub fn push(&mut self, event: SettleStackEvent) {
        self.events.push(event);
    }
}

/// Drains [`PendingStackSettleEvents`] and compacts each affected column,
/// then drops anything floating above the new top.
///
/// Two passes per column:
///   1. **Compact block-sized members.** Collect block-sized objects
///      (excluding `removed_entity`), sort by current `z`, and re-assign `z`
///      from `0` upward by cumulative `block_size`. Yields the new
///      `stack_top` for the column.
///   2. **Drop floaters.** Anything *else* in the column that sat above the
///      new `stack_top` — players, NPCs, flat decals on the chest you just
///      picked up — gets snapped down to `stack_top`. This is what makes
///      picking the chest out from under your own feet leave you on the
///      ground instead of floating.
///
/// Both passes mutate `TilePosition` directly; the state-diff pipeline
/// (`collect_game_events_from_authority`) replicates the changes to clients.
pub fn settle_pending_stacks(
    mut pending: ResMut<PendingStackSettleEvents>,
    mut object_query: Query<(Entity, &SpaceResident, &mut TilePosition, &OverworldObject)>,
    definitions: Res<OverworldObjectDefinitions>,
) {
    if pending.events.is_empty() {
        return;
    }
    for event in pending.events.drain(..) {
        // Pass 1: collect block-sized stack members in the column.
        let mut members: Vec<(Entity, i32, u8)> = object_query
            .iter()
            .filter(|(entity, resident, tile, _)| {
                Some(*entity) != event.removed_entity
                    && resident.space_id == event.space_id
                    && tile.x == event.x
                    && tile.y == event.y
            })
            .filter_map(|(entity, _, tile, object)| {
                let def = definitions.get(&object.definition_id)?;
                if def.render.block_size == 0 {
                    return None;
                }
                Some((entity, tile.z, def.render.block_size))
            })
            .collect();
        members.sort_by_key(|(_, z, _)| *z);

        let member_entities: std::collections::HashSet<Entity> =
            members.iter().map(|(e, _, _)| *e).collect();

        // Compact block members and record the new top.
        let mut next_z = 0i32;
        for (entity, _, bs) in members {
            if let Ok((_, _, mut tile, _)) = object_query.get_mut(entity) {
                if tile.z != next_z {
                    tile.z = next_z;
                }
            }
            next_z += bs as i32;
        }
        let new_stack_top = next_z;

        // Pass 2: drop any other entity in the column that's floating above
        // the new top. Collect first to avoid holding an iter borrow while
        // mutating via `get_mut`.
        let floaters: Vec<Entity> = object_query
            .iter()
            .filter(|(entity, resident, tile, _)| {
                Some(*entity) != event.removed_entity
                    && !member_entities.contains(entity)
                    && resident.space_id == event.space_id
                    && tile.x == event.x
                    && tile.y == event.y
                    && tile.z > new_stack_top
            })
            .map(|(entity, _, _, _)| entity)
            .collect();
        for entity in floaters {
            if let Ok((_, _, mut tile, _)) = object_query.get_mut(entity) {
                tile.z = new_stack_top;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_place_respects_reach() {
        // Player at z=0 placing onto empty column (top=0): reach 0 ≤ 2 ✓.
        assert!(can_place_on_stack(0, 0, 2));
        // Chest on a barrel from the ground: barrel top = 2, reach = 2 ≤ 2 ✓.
        assert!(can_place_on_stack(0, 2, 1));
        // Another barrel on the barrel from the ground: top=2, reach 2 ≤ 2 ✓.
        assert!(can_place_on_stack(0, 2, 2));
        // Stack of barrel+chest (top=3) from the ground: reach 3 > 2 → rejected.
        assert!(!can_place_on_stack(0, 3, 1));
        // Once you climb onto the barrel (player_z=2), the same top=3 is in
        // reach again — that's how you keep stacking higher.
        assert!(can_place_on_stack(2, 3, 1));
        // Dropping into a small pit is fine; into a deep one is not.
        assert!(can_place_on_stack(2, 0, 1));
        assert!(!can_place_on_stack(4, 0, 1));
    }
}
