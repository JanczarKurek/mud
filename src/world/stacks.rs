use bevy::prelude::*;

use crate::game::resources::PlacementSeqCounter;
use crate::world::components::{
    OverworldObject, RenderStackOrder, SpaceId, SpaceResident, TilePosition,
};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::floors::{floormap_tile_walkable, MAX_FLOORS_ABOVE};
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Raw z (half-block units) of every painted FloorMap walkable surface in
/// column `(space, x, y)`. Returns an empty Vec when no upper floors are
/// painted there. Ground (z=0) is *not* included — callers add it implicitly.
fn floormap_supports_in_column(
    floor_maps: &FloorMaps,
    floor_defs: &FloorTilesetDefinitions,
    space: SpaceId,
    x: i32,
    y: i32,
) -> Vec<i32> {
    (1..=MAX_FLOORS_ABOVE)
        .filter(|fi| floormap_tile_walkable(floor_maps.get(space, *fi), floor_defs, x, y))
        .map(|fi| fi * 2)
        .collect()
}

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

/// `z` of the surface above the topmost block or painted upper floor at
/// column `(space, x, y)` — i.e. the `z` a newly placed object's *feet*
/// would occupy. Returns `0` (ground) when the column is empty and no upper
/// FloorMap tile covers it. Half-blocks contribute `+1` z, full blocks `+2`;
/// flat (`block_size == 0`) objects don't change the stack top, but painted
/// FloorMap tiles do — a walkable FloorMap tile on floor N raises the stack
/// top to `N * 2`.
pub fn stack_top_z<'a, I>(
    space: SpaceId,
    x: i32,
    y: i32,
    members: I,
    definitions: &OverworldObjectDefinitions,
    floor_maps: &FloorMaps,
    floor_defs: &FloorTilesetDefinitions,
) -> i32
where
    I: IntoIterator<Item = ColumnMember<'a>>,
{
    stack_top_z_excluding(
        space,
        x,
        y,
        Entity::PLACEHOLDER,
        members,
        definitions,
        floor_maps,
        floor_defs,
    )
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
    floor_maps: &FloorMaps,
    floor_defs: &FloorTilesetDefinitions,
) -> i32
where
    I: IntoIterator<Item = ColumnMember<'a>>,
{
    let object_top = members
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
        .unwrap_or(0);
    let floor_top = floormap_supports_in_column(floor_maps, floor_defs, space, x, y)
        .into_iter()
        .max()
        .unwrap_or(0);
    object_top.max(floor_top)
}

/// True iff the topmost surface at column `(space, x, y)` (excluding
/// `exclude`) is walkable — i.e. it's safe to stack onto. A column with only
/// flat objects, or no objects at all, returns true (drop onto ground). A
/// column whose top *object* is a wall returns false. A painted FloorMap tile
/// on top wins by raw z and contributes its tileset's `walkable_surface` flag
/// (currently always `true` for any authored upper floor).
pub fn stack_top_is_walkable<'a, I>(
    space: SpaceId,
    x: i32,
    y: i32,
    exclude: Entity,
    members: I,
    definitions: &OverworldObjectDefinitions,
    floor_maps: &FloorMaps,
    floor_defs: &FloorTilesetDefinitions,
) -> bool
where
    I: IntoIterator<Item = ColumnMember<'a>>,
{
    let object_top = members
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
    let floor_top = floormap_supports_in_column(floor_maps, floor_defs, space, x, y)
        .into_iter()
        .max();
    match (object_top, floor_top) {
        (Some((oz, ow)), Some(fz)) => {
            if oz >= fz {
                ow
            } else {
                // Painted FloorMap currently always has walkable_surface = true.
                true
            }
        }
        (Some((_, ow)), None) => ow,
        (None, Some(_)) => true,
        (None, None) => true,
    }
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
    floor_maps: Res<FloorMaps>,
    floor_defs: Res<FloorTilesetDefinitions>,
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

        // Pass 2: drop any floater that isn't sitting on a supported z. Support
        // = ground (0), the new block stack top, or any painted-FloorMap
        // walkable surface in this column. A player standing on an upper-floor
        // wooden_floor at z=2 must stay at z=2 even when no block-sized object
        // sits beneath them; previously they'd snap back to z=0.
        let mut supports: Vec<i32> = vec![0, new_stack_top];
        supports.extend(floormap_supports_in_column(
            &floor_maps,
            &floor_defs,
            event.space_id,
            event.x,
            event.y,
        ));
        let landing_for = |z: i32| -> i32 {
            supports
                .iter()
                .copied()
                .filter(|s| *s <= z)
                .max()
                .unwrap_or(0)
        };
        let floaters: Vec<(Entity, i32)> = object_query
            .iter()
            .filter(|(entity, resident, tile, _)| {
                Some(*entity) != event.removed_entity
                    && !member_entities.contains(entity)
                    && resident.space_id == event.space_id
                    && tile.x == event.x
                    && tile.y == event.y
            })
            .filter_map(|(entity, _, tile, _)| {
                let landing = landing_for(tile.z);
                (landing < tile.z).then_some((entity, landing))
            })
            .collect();
        for (entity, landing) in floaters {
            if let Ok((_, _, mut tile, _)) = object_query.get_mut(entity) {
                tile.z = landing;
            }
        }
    }
}

/// Stamps a fresh `placement_seq` onto every newly-spawned `OverworldObject`.
/// Runs once per Bevy tick; `Added<OverworldObject>` makes it a no-op for
/// entities whose component was attached in a prior tick, so it doesn't fight
/// `settle_pending_stacks` (which only mutates `TilePosition.z`) and doesn't
/// re-stamp moving players/NPCs each step. The single point of seq
/// assignment — production placement paths (world load, NPC corpse drops,
/// inventory→world drops, editor placements) all funnel through fresh
/// `spawn` calls, which `Added` catches uniformly.
pub fn stamp_placement_seq_on_spawn(
    counter: Option<ResMut<PlacementSeqCounter>>,
    mut query: Query<&mut OverworldObject, Added<OverworldObject>>,
) {
    let Some(mut counter) = counter else {
        return;
    };
    for mut object in query.iter_mut() {
        object.placement_seq = counter.next();
    }
}

/// Mirrors `OverworldObject::placement_seq` to a `RenderStackOrder` component
/// on the same entity so `sync_tile_transforms` can use a single
/// `Option<&RenderStackOrder>` query for both authoritative entities (this
/// path) and TcpClient projected entities (handled in
/// `sync_client_world_projection`).
pub fn sync_render_stack_order(
    mut commands: Commands,
    query: Query<(Entity, &OverworldObject, Option<&RenderStackOrder>), Changed<OverworldObject>>,
) {
    for (entity, object, existing) in &query {
        let next = RenderStackOrder(object.placement_seq);
        if existing.copied() != Some(next) {
            commands.entity(entity).insert(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_places_unique_seq_per_spawn() {
        let mut app = App::new();
        app.insert_resource(PlacementSeqCounter::default());
        app.add_systems(Update, stamp_placement_seq_on_spawn);

        let space_id = SpaceId(1);
        let e1 = app
            .world_mut()
            .spawn((
                OverworldObject {
                    object_id: 10,
                    definition_id: "pickaxe".to_string(),
                    placement_seq: 0,
                },
                SpaceResident { space_id },
                TilePosition::ground(5, 5),
            ))
            .id();
        app.update();
        let seq1 = app
            .world()
            .entity(e1)
            .get::<OverworldObject>()
            .unwrap()
            .placement_seq;

        let e2 = app
            .world_mut()
            .spawn((
                OverworldObject {
                    object_id: 11,
                    definition_id: "pen".to_string(),
                    placement_seq: 0,
                },
                SpaceResident { space_id },
                TilePosition::ground(5, 5),
            ))
            .id();
        app.update();
        let seq2 = app
            .world()
            .entity(e2)
            .get::<OverworldObject>()
            .unwrap()
            .placement_seq;

        // Later spawn must get a strictly higher seq so LIFO tiebreak works.
        assert!(seq2 > seq1, "second seq {seq2} should exceed first {seq1}");
    }

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
