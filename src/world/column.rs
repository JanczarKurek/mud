//! The voxel model of a single tile column — the one place that answers
//! "what occupies `(space, x, y)` at height `z`?".
//!
//! # Occupancy model
//!
//! A column is a stack of half-block cells at integer `z` (see
//! [`crate::world::components::floor_index`] for the raw-z ↔ floor mapping):
//!
//! * An object with `render.block_size = b` at `z0` occupies cells
//!   `[z0, z0 + b)` and presents a surface at `z0 + b`. `b == 0` (decals,
//!   loose items) occupies nothing and presents no surface.
//! * A painted `FloorMap` tile on floor `fi >= 1` is a **slab**: it occupies
//!   exactly the cell [`slab_cell_z`]`(fi)` = `fi*2 - 1` and presents a
//!   standable surface at [`surface_z`]`(fi)` = `fi*2`. This is the same
//!   placement [`crate::world::spatial::build_indices`] uses for its movement
//!   and line-of-sight blockers, so the indices cannot drift apart.
//! * Ground (`z = 0`) is an implicit standable surface with nothing below it.
//!
//! # Why this module exists
//!
//! Three subsystems used to each carry their own partial answer: `stacks.rs`
//! (placement) knew floors only as "a surface at `fi*2`", `spatial.rs`
//! (pathing / LoS) knew them as "material at `fi*2 - 1`", and `floors.rs`
//! (rendering) knew them as "is the floor above me covering this tile". The
//! placement path consequently never learned what the LoS path already knew,
//! and would happily resolve a ground-floor drop onto the floor above.
//!
//! Everything derives from the model here now. `stacks.rs` keeps its public
//! functions as thin wrappers so existing call sites are untouched.
//!
//! # Client / server sharing
//!
//! The *model* is shared; only the *sources* differ. [`Column::from_world`]
//! reads ECS components plus [`FloorMaps`]; [`Column::from_client_state`]
//! reads the replicated `ClientGameState` projection. Both funnel into the
//! same private constructor, so client prediction and server authority can
//! never disagree about floor semantics.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::world::components::{OverworldObject, SpaceId, SpaceResident, TilePosition};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::{FloorMap, FloorMaps};
use crate::world::object_definitions::OverworldObjectDefinitions;

/// How many floors above ground a column scan considers. Mirrors
/// [`crate::world::floors::MAX_FLOORS_ABOVE`]; kept as a separate constant so
/// this module has no dependency on the render-side floor code.
pub const MAX_FLOORS_SCANNED: i32 = 16;

/// Raw `z` of the *material* of floor `floor_idx` — the slab under its walking
/// surface. A vertical ray between two floors always passes through this odd
/// half-block, while two entities standing *on* the floor (both at the even
/// [`surface_z`]) are never separated by it.
pub const fn slab_cell_z(floor_idx: i32) -> i32 {
    floor_idx * 2 - 1
}

/// Raw `z` of the walking surface of floor `floor_idx`.
pub const fn surface_z(floor_idx: i32) -> i32 {
    floor_idx * 2
}

/// Borrowed view of where the world's floors physically are.
///
/// One type over both the authoritative [`FloorMaps`] and the replicated
/// client projection, so the reach and placement rules are *literally the same
/// code* on both sides rather than two implementations that can drift.
///
/// Pass it around by value — it's two references. Systems that need it as a
/// Bevy parameter should take [`FloorGeometryParam`] and call
/// [`FloorGeometryParam::geometry`], which keeps the gate a one-parameter
/// change for any system that grows a reach check.
#[derive(Clone, Copy)]
pub struct FloorGeometry<'a> {
    source: FloorSource<'a>,
    defs: &'a FloorTilesetDefinitions,
}

#[derive(Clone, Copy)]
enum FloorSource<'a> {
    Authoritative(&'a FloorMaps),
    Replicated(&'a HashMap<(SpaceId, i32), FloorMap>),
}

impl<'a> FloorGeometry<'a> {
    /// Server-side view over the authoritative floor maps.
    pub fn server(maps: &'a FloorMaps, defs: &'a FloorTilesetDefinitions) -> Self {
        Self {
            source: FloorSource::Authoritative(maps),
            defs,
        }
    }

    /// Client-side view over `ClientGameState.floor_maps`.
    pub fn client(
        maps: &'a HashMap<(SpaceId, i32), FloorMap>,
        defs: &'a FloorTilesetDefinitions,
    ) -> Self {
        Self {
            source: FloorSource::Replicated(maps),
            defs,
        }
    }

    pub fn definitions(&self) -> &'a FloorTilesetDefinitions {
        self.defs
    }

    fn floor(&self, space: SpaceId, floor_idx: i32) -> Option<&'a FloorMap> {
        match self.source {
            FloorSource::Authoritative(maps) => maps.get(space, floor_idx),
            FloorSource::Replicated(maps) => maps.get(&(space, floor_idx)),
        }
    }

    /// True iff floor material lies between `a_z` and `b_z` in column
    /// `(x, y)` — some slab cell falls in `[min, max)`.
    pub fn slab_between(&self, space: SpaceId, x: i32, y: i32, a_z: i32, b_z: i32) -> bool {
        let (lo, hi) = (a_z.min(b_z), a_z.max(b_z));
        if lo == hi {
            return false;
        }
        (1..=MAX_FLOORS_SCANNED)
            .filter(|fi| (lo..hi).contains(&slab_cell_z(*fi)))
            .any(|fi| {
                self.floor(space, fi)
                    .and_then(|grid| grid.get(x, y))
                    .and_then(|id| self.defs.get(id))
                    .is_some_and(|def| def.walkable_surface || def.occludes_floor_above)
            })
    }

    /// **The reach gate.** Every "do something to a nearby world object"
    /// command funnels through here: [`is_near_player`] adjacency AND no floor
    /// slab between the actor's feet and the target.
    ///
    /// [`is_near_player`]'s `|dz| <= 2` window is *exactly one whole floor*
    /// (`z` is in half-blocks), which is what let a player pick up, open,
    /// rotate, engrave, craft with, hide behind, or trade through an object on
    /// the storey above. The slab test is taken in the **actor's own column**:
    /// a painted floor is a horizontal plane spanning the room, so it
    /// necessarily covers the actor's tile, while free-standing objects are
    /// never slabs. That blocks every through-the-floor reach yet leaves "take
    /// the item off the adjacent barrel" (a full-block object, no slab
    /// involved) working, and lets an actor standing in a stairwell opening
    /// reach up onto the floor above.
    ///
    /// [`is_near_player`]: crate::game::helpers::is_near_player
    pub fn reachable(&self, actor: &TilePosition, target: &TilePosition, space: SpaceId) -> bool {
        crate::game::helpers::is_near_player(actor, target)
            && !self.slab_between(space, actor.x, actor.y, actor.z, target.z)
    }
}

/// Bevy parameter form of [`FloorGeometry`]. Systems that need a reach check
/// take this single parameter instead of two `Res`es.
///
/// Not usable in a system that already holds `ResMut<FloorMaps>` (Bevy rejects
/// the conflicting access) — `process_game_commands` is the notable case, and
/// it builds a [`FloorGeometry`] from its existing `SpaceAuthority` bundle
/// instead.
#[derive(bevy::ecs::system::SystemParam)]
pub struct FloorGeometryParam<'w> {
    maps: Res<'w, FloorMaps>,
    defs: Res<'w, FloorTilesetDefinitions>,
}

impl FloorGeometryParam<'_> {
    pub fn geometry(&self) -> FloorGeometry<'_> {
        FloorGeometry::server(&self.maps, &self.defs)
    }

    /// Passthrough for [`FloorGeometry::reachable`].
    pub fn reachable(&self, actor: &TilePosition, target: &TilePosition, space: SpaceId) -> bool {
        self.geometry().reachable(actor, target, space)
    }
}

/// One member of a tile column as seen by the column builder. Callers pass
/// their own filtered iterator so the builder stays agnostic to Bevy `Query`
/// filter types (`With<...>`, `Without<...>`).
pub struct ColumnMember<'a> {
    pub entity: Entity,
    pub resident: &'a SpaceResident,
    pub tile: &'a TilePosition,
    pub object: &'a OverworldObject,
}

/// A resolved tile column. Build once per `(x, y)` and ask it every question;
/// each constructor walks the world exactly once.
#[derive(Clone, Debug, Default)]
pub struct Column {
    /// `(surface_z, walkable)` for every standable candidate in the column,
    /// including the implicit ground surface at `(0, true)`.
    surfaces: Vec<(i32, bool)>,
    /// Floor indices (`>= 1`) whose painted tile contributes solid material —
    /// i.e. is `walkable_surface` or `occludes_floor_above`. Used by
    /// [`Column::slab_between`].
    slab_floors: Vec<i32>,
}

impl Column {
    /// Server adapter: ECS column members plus the world's [`FloorGeometry`].
    /// `exclude` drops one entity from the column — used by the
    /// drag-an-object-onto-its-own-tile path so an object doesn't stack on
    /// itself. Pass [`Entity::PLACEHOLDER`] to exclude nothing.
    pub fn from_world<'a, I>(
        space: SpaceId,
        x: i32,
        y: i32,
        exclude: Entity,
        members: I,
        definitions: &OverworldObjectDefinitions,
        geometry: FloorGeometry<'_>,
    ) -> Self
    where
        I: IntoIterator<Item = ColumnMember<'a>>,
    {
        let objects = members
            .into_iter()
            .filter(|m| {
                m.entity != exclude
                    && m.resident.space_id == space
                    && m.tile.x == x
                    && m.tile.y == y
            })
            .filter_map(|m| {
                let def = definitions.get(&m.object.definition_id)?;
                Some((m.tile.z, def.render.block_size, def.render.walkable_surface))
            });
        Self::new(objects, geometry, space, x, y)
    }

    /// Client adapter: the replicated `ClientGameState` projection.
    pub fn from_client_state(
        client_state: &crate::game::resources::ClientGameState,
        definitions: &OverworldObjectDefinitions,
        floor_defs: &FloorTilesetDefinitions,
        space: SpaceId,
        x: i32,
        y: i32,
    ) -> Self {
        let objects = client_state
            .world_objects
            .values()
            .filter(|o| {
                o.position.space_id == space && o.tile_position.x == x && o.tile_position.y == y
            })
            .filter_map(|o| {
                let def = definitions.get(&o.definition_id)?;
                Some((
                    o.tile_position.z,
                    def.render.block_size,
                    def.render.walkable_surface,
                ))
            });
        Self::new(
            objects,
            FloorGeometry::client(&client_state.floor_maps, floor_defs),
            space,
            x,
            y,
        )
    }

    /// Shared constructor. `objects` yields `(feet_z, block_size, walkable_top)`
    /// for every member already filtered to this column.
    fn new<O>(objects: O, geometry: FloorGeometry<'_>, space: SpaceId, x: i32, y: i32) -> Self
    where
        O: Iterator<Item = (i32, u8, bool)>,
    {
        // Ground is always a standable candidate; everything else stacks on it.
        let mut surfaces = vec![(0, true)];
        for (feet_z, block_size, walkable) in objects {
            if block_size == 0 {
                continue;
            }
            surfaces.push((feet_z + block_size as i32, walkable));
        }

        let mut slab_floors = Vec::new();
        for fi in 1..=MAX_FLOORS_SCANNED {
            let Some(def) = geometry
                .floor(space, fi)
                .and_then(|grid| grid.get(x, y))
                .and_then(|id| geometry.definitions().get(id))
            else {
                continue;
            };
            if def.walkable_surface {
                surfaces.push((surface_z(fi), true));
            }
            // Material either way: a floor you can walk on and a floor that
            // merely blocks the sky both occupy their slab cell.
            if def.walkable_surface || def.occludes_floor_above {
                slab_floors.push(fi);
            }
        }

        Self {
            surfaces,
            slab_floors,
        }
    }

    /// Highest standable surface in the column, ignoring who is asking.
    /// Returns `0` for an empty column.
    pub fn top_surface(&self) -> i32 {
        self.surfaces.iter().map(|(z, _)| *z).max().unwrap_or(0)
    }

    /// Whether [`Column::top_surface`] can be stood on. A column topped by a
    /// wall is not walkable; a column with only flat items, or nothing at all,
    /// is (you're dropping onto the ground). When an object top and a painted
    /// floor tie on `z`, the object's flag wins — a wall flush with the floor
    /// above is still a wall.
    pub fn top_is_walkable(&self) -> bool {
        self.walkable_at(self.top_surface())
    }

    /// Highest standable surface an actor at `from_z` can actually put
    /// something on: the same set as [`Column::top_surface`] minus every
    /// surface separated from `from_z` by a floor slab.
    ///
    /// This is what keeps a drop made on the ground floor of a roofed room at
    /// `z = 0` instead of teleporting it onto the floor above — while leaving
    /// "set it on top of the adjacent barrel" (a free-standing full block, not
    /// a slab) working exactly as before.
    pub fn surface_from(&self, from_z: i32) -> i32 {
        self.surfaces
            .iter()
            .map(|(z, _)| *z)
            .filter(|z| !self.slab_between(from_z, *z))
            .max()
            .unwrap_or(0)
    }

    /// [`Column::top_is_walkable`] for the surface [`Column::surface_from`]
    /// picks.
    pub fn surface_from_is_walkable(&self, from_z: i32) -> bool {
        self.walkable_at(self.surface_from(from_z))
    }

    /// True iff floor material lies between `a` and `b`, i.e. some slab cell
    /// falls in `[min(a, b), max(a, b))`.
    ///
    /// Half-open on purpose: `(0, 2)` with a floor above is blocked (the slab
    /// at `z = 1` is in the way), `(2, 2)` on that same floor is free, and
    /// `(0, 1)` — stepping onto a half-block chest — is free.
    pub fn slab_between(&self, a: i32, b: i32) -> bool {
        let (lo, hi) = (a.min(b), a.max(b));
        self.slab_floors
            .iter()
            .any(|fi| (lo..hi).contains(&slab_cell_z(*fi)))
    }

    /// Walkability of the candidates sharing surface `z`. Every candidate at
    /// that height must agree, so a wall flush with a painted floor reads as
    /// not walkable.
    fn walkable_at(&self, z: i32) -> bool {
        self.surfaces
            .iter()
            .filter(|(sz, _)| *sz == z)
            .all(|(_, walkable)| *walkable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::floor_definitions::FloorTilesetDefinition;

    /// A `wooden_floor`-alike: walkable and sky-blocking.
    fn floor_defs_with_wood() -> FloorTilesetDefinitions {
        let mut by_id = HashMap::new();
        by_id.insert(
            "wood".to_string(),
            FloorTilesetDefinition {
                id: "wood".to_string(),
                name: "Wood".to_string(),
                priority: 100,
                tile_size_px: 16,
                atlas_path: None,
                debug_color: [0, 0, 0],
                occludes_floor_above: true,
                walkable_surface: true,
                variants: HashMap::new(),
                ripple: None,
            },
        );
        FloorTilesetDefinitions::for_test(by_id, HashMap::new())
    }

    const SPACE: SpaceId = SpaceId(1);

    /// Replicated floor maps painting `wood` at (0, 0) on each of
    /// `painted_floors`. Goes through the client `FloorGeometry` variant so the
    /// tests exercise the same code path the client does.
    fn floor_maps(painted_floors: &[i32]) -> HashMap<(SpaceId, i32), FloorMap> {
        painted_floors
            .iter()
            .map(|fi| {
                (
                    (SPACE, *fi),
                    FloorMap::new_filled(1, 1, Some("wood".to_string())),
                )
            })
            .collect()
    }

    /// Column at (0, 0) with a painted floor on each of `painted_floors` and
    /// the given block-sized objects.
    fn column(objects: Vec<(i32, u8, bool)>, painted_floors: Vec<i32>) -> Column {
        let defs = floor_defs_with_wood();
        let maps = floor_maps(&painted_floors);
        Column::new(
            objects.into_iter(),
            FloorGeometry::client(&maps, &defs),
            SPACE,
            0,
            0,
        )
    }

    #[test]
    fn slab_between_truth_table() {
        let col = column(vec![], vec![1]);
        // Ground floor reaching up to the floor above: the slab at z=1 is in
        // the way.
        assert!(col.slab_between(0, 2));
        // Two points on the same upper floor: free.
        assert!(!col.slab_between(2, 2));
        // Reaching down through your own floorboards: blocked (symmetric).
        assert!(col.slab_between(2, 0));
        // Stepping onto a half-block chest on the ground floor: free.
        assert!(!col.slab_between(0, 1));
        // Chest on top of the upper floor: free.
        assert!(!col.slab_between(2, 3));
    }

    #[test]
    fn surface_from_keeps_ground_drops_on_the_ground() {
        // Empty ground-floor tile in a roofed room.
        let col = column(vec![], vec![1]);
        assert_eq!(col.top_surface(), 2, "raw top is still the floor above");
        assert_eq!(
            col.surface_from(0),
            0,
            "a player at z=0 places on the ground, not through the ceiling"
        );
        assert_eq!(
            col.surface_from(2),
            2,
            "a player on the upper floor places on the upper floor"
        );
    }

    #[test]
    fn surface_from_still_stacks_onto_adjacent_full_blocks() {
        // Outdoor tile holding one barrel (`block_size: 2`, walkable top).
        let col = column(vec![(0, 2, true)], vec![]);
        assert_eq!(
            col.surface_from(0),
            2,
            "no slab involved — setting an item on a barrel top must still work"
        );
        assert!(col.surface_from_is_walkable(0));
    }

    #[test]
    fn wall_top_is_not_walkable_even_flush_with_a_floor() {
        // A `block_size: 2` wall at z=0 in a column that also has a painted
        // floor on floor 1: both present a surface at z=2.
        let col = column(vec![(0, 2, false)], vec![1]);
        assert_eq!(col.top_surface(), 2);
        assert!(!col.top_is_walkable(), "the wall's flag must win the tie");
    }

    #[test]
    fn empty_column_is_ground_and_walkable() {
        let col = column(vec![], vec![]);
        assert_eq!(col.top_surface(), 0);
        assert!(col.top_is_walkable());
        assert_eq!(col.surface_from(0), 0);
    }

    #[test]
    fn flat_objects_do_not_raise_the_column() {
        let col = column(vec![(0, 0, true)], vec![]);
        assert_eq!(col.top_surface(), 0);
    }

    /// The gate every "act on a nearby object" command now funnels through.
    #[test]
    fn reachable_blocks_through_the_floor_but_not_over_a_barrel() {
        let defs = floor_defs_with_wood();
        let roofed = floor_maps(&[1]);
        let geometry = FloorGeometry::client(&roofed, &defs);

        let ground = TilePosition::new(0, 0, 0);
        // Same floor, adjacent tile: reachable.
        assert!(geometry.reachable(&ground, &TilePosition::new(1, 0, 0), SPACE));
        // Onto a half-block chest beside you: reachable.
        assert!(geometry.reachable(&ground, &TilePosition::new(1, 0, 1), SPACE));
        // Up onto the storey above through the ceiling: blocked, even though
        // `is_near_player`'s |dz| <= 2 window accepts it.
        assert!(crate::game::helpers::is_near_player(
            &ground,
            &TilePosition::new(1, 0, 2)
        ));
        assert!(!geometry.reachable(&ground, &TilePosition::new(1, 0, 2), SPACE));
        // And symmetrically, down through your own floorboards.
        let upstairs = TilePosition::new(0, 0, 2);
        assert!(!geometry.reachable(&upstairs, &TilePosition::new(1, 0, 0), SPACE));
        assert!(geometry.reachable(&upstairs, &TilePosition::new(1, 0, 2), SPACE));

        // With no floor painted overhead, the top of an adjacent full-block
        // barrel (z=2) stays reachable — the gate keys on floor slabs, not on
        // raw height.
        let open = floor_maps(&[]);
        let outdoors = FloorGeometry::client(&open, &defs);
        assert!(outdoors.reachable(&ground, &TilePosition::new(1, 0, 2), SPACE));

        // Out of XY range is still out of range.
        assert!(!outdoors.reachable(&ground, &TilePosition::new(3, 0, 0), SPACE));
    }

    #[test]
    fn slab_cell_sits_below_the_walking_surface() {
        assert_eq!(slab_cell_z(1), 1);
        assert_eq!(surface_z(1), 2);
        assert_eq!(slab_cell_z(2), 3);
        assert_eq!(surface_z(2), 4);
    }
}
