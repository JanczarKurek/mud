use bevy::prelude::*;
use std::collections::HashSet;

use crate::game::resources::ClientGameState;
use crate::world::components::{floor_index, SpaceId};
use crate::world::direction::Direction;
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMap;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// How many floors above the player we'll scan upward in search of an
/// `occludes_floor_above` tile. The scan terminates as soon as a covering tile
/// is found, so this only sets the depth for an *uncovered* player (e.g.
/// standing outside in the open).
pub const MAX_FLOORS_ABOVE: i32 = 16;

/// True iff a FloorMap tile (if any) at `(x, y)` in `floor` carries an
/// `occludes_floor_above` flag on its tileset definition. Painted upper-floor
/// tiles act as roofs to the floor below — the same role `floor_plank` objects
/// used to fill before the FloorMap migration.
pub fn floormap_tile_occludes(
    floor: Option<&FloorMap>,
    floor_defs: &FloorTilesetDefinitions,
    x: i32,
    y: i32,
) -> bool {
    floor
        .and_then(|m| m.get(x, y))
        .and_then(|id| floor_defs.get(id))
        .is_some_and(|def| def.occludes_floor_above)
}

/// True iff a FloorMap tile at `(x, y)` in `floor` carries `walkable_surface`.
/// Drives upper-floor walkability and stack-support: a painted floor at z=2
/// makes raw z=2 a valid landing surface even when no walkable object lives
/// there.
pub fn floormap_tile_walkable(
    floor: Option<&FloorMap>,
    floor_defs: &FloorTilesetDefinitions,
    x: i32,
    y: i32,
) -> bool {
    floor
        .and_then(|m| m.get(x, y))
        .and_then(|id| floor_defs.get(id))
        .is_some_and(|def| def.walkable_surface)
}

/// True iff floor `floor_idx` at `(x, y)` is "indoor" — i.e. covered by an
/// occluder on the next floor up (`floor_idx + 1`). An occluder is either an
/// object with `render.occludes_floor_above = true` or a painted FloorMap tile
/// whose tileset has `occludes_floor_above = true`. Callers with a raw `z`
/// should pass `floor_index(z)`. The same predicate powers floor-roof culling
/// (`recompute_visible_floors`) and outdoor-light occlusion in the lighting
/// system.
pub fn is_indoor_tile(
    state: &ClientGameState,
    definitions: &OverworldObjectDefinitions,
    floor_defs: &FloorTilesetDefinitions,
    space_id: SpaceId,
    x: i32,
    y: i32,
    floor_idx: i32,
) -> bool {
    let object_occludes = state.world_objects.values().any(|object| {
        if object.position.space_id != space_id
            || floor_index(object.tile_position.z) != floor_idx + 1
        {
            return false;
        }
        if object.tile_position.x != x || object.tile_position.y != y {
            return false;
        }
        definitions
            .get(&object.definition_id)
            .is_some_and(|def| def.render.occludes_floor_above)
    });
    if object_occludes {
        return true;
    }
    floormap_tile_occludes(
        state.floor_maps.get(&(space_id, floor_idx + 1)),
        floor_defs,
        x,
        y,
    )
}

/// Cached set of indoor tiles for the current frame. Rebuilt in
/// `recompute_indoor_tile_map` from one sweep over `world_objects`, so the
/// per-frame consumers (`sync_tile_transforms`, `sync_floor_render_transforms`)
/// can answer "is `(space, x, y, z)` indoor?" in O(1). Without this cache the
/// floor-cell sync ran 4× O(world_objects) per cell — pathological on the 70×50
/// overworld.
#[derive(Resource, Default, Clone, Debug)]
pub struct IndoorTileMap {
    /// `(space, x, y, floor_idx)` of every covered tile. `floor_idx` is the
    /// covered floor itself (one below the floor that holds the occluder).
    tiles: HashSet<(SpaceId, i32, i32, i32)>,
}

impl IndoorTileMap {
    pub fn contains(&self, space_id: SpaceId, x: i32, y: i32, floor_idx: i32) -> bool {
        self.tiles.contains(&(space_id, x, y, floor_idx))
    }
}

/// Build the per-frame `IndoorTileMap` from one sweep over `world_objects`.
/// Schedules after `apply_game_events_to_client_state` so the set reflects the
/// latest replicated state. Matches the predicate in `is_indoor_tile`: an
/// occluder on floor `floor_index(object.z)` makes `(x, y, floor_index(object.z) - 1)`
/// indoor.
pub fn recompute_indoor_tile_map(
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut map: ResMut<IndoorTileMap>,
) {
    let _t = crate::diagnostics::SystemTimer::new("recompute_indoor_tile_map", 1.0);
    map.tiles.clear();
    for object in client_state.world_objects.values() {
        let occludes = definitions
            .get(&object.definition_id)
            .is_some_and(|def| def.render.occludes_floor_above);
        if !occludes {
            continue;
        }
        let tile = object.tile_position;
        map.tiles.insert((
            object.position.space_id,
            tile.x,
            tile.y,
            floor_index(tile.z) - 1,
        ));
    }
    // FloorMap tiles whose tileset has `occludes_floor_above` cover the floor
    // immediately below them, the same way object occluders do. This is what
    // makes painted upper-storey floors hide the ground floor.
    for ((space_id, floor_idx), grid) in client_state.floor_maps.iter() {
        let target_floor = *floor_idx - 1;
        for y in 0..grid.height {
            for x in 0..grid.width {
                if floormap_tile_occludes(Some(grid), &floor_defs, x, y) {
                    map.tiles.insert((*space_id, x, y, target_floor));
                }
            }
        }
    }
}

/// Whether a sprite anchored at `(x, y, z)` should receive the indoor-ambient
/// color tint. Floors, NPCs, and ground objects (no `hide_when_inside_facing`)
/// tint when *their own tile* is indoor. Wall sprites tint when the *interior*
/// tile they back onto is indoor — i.e. the wall is a "back" wall (N or W) of
/// the building, not the camera-facing front (S or E) which gets alpha-faded
/// instead. The direction encodes the sprite's visible face:
/// - `South` (sprite shows its south face): interior is to its NORTH; tint when
///   `(x, y-1, z)` is indoor (= this wall sits on the NORTH edge, a back wall).
///   If `(x, y+1, z)` is indoor it's the SOUTH edge and stays untinted.
/// - `East` (sprite shows its east face): interior is to its WEST; tint when
///   `(x+1, y, z)` is indoor (= WEST edge, back wall).
/// - `North` / `West` are symmetric for completeness; current assets use only
///   `South` / `East`.
pub fn should_apply_indoor_tint(
    indoor: &IndoorTileMap,
    space_id: SpaceId,
    x: i32,
    y: i32,
    floor_idx: i32,
    hide_when_inside_facing: Option<Direction>,
) -> bool {
    let (sx, sy) = match hide_when_inside_facing {
        None => (x, y),
        Some(Direction::South) => (x, y - 1),
        Some(Direction::East) => (x + 1, y),
        Some(Direction::North) => (x, y + 1),
        Some(Direction::West) => (x - 1, y),
    };
    indoor.contains(space_id, sx, sy, floor_idx)
}

/// Window of floor indices currently rendered on the client. Recomputed each
/// frame from the local player's position plus the covering tiles above them.
/// Consumed by `sync_tile_transforms` to cull/dim by floor.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct VisibleFloorRange {
    /// Floor index of the player (`floor_index(player.z)`). Used for culling
    /// and floor-bucketed checks (visible-range, indoor-tile lookups).
    pub player_floor: i32,
    /// Player `z` in half-block units. Used by `floor_screen_offset` to
    /// produce fractional-floor diagonal shifts on intra-floor climbs (e.g.
    /// standing on a half-block chest = z+1 = half-floor up).
    pub player_z: i32,
    pub lowest_visible: i32,
    pub highest_visible: i32,
}

impl VisibleFloorRange {
    pub fn contains(&self, floor: i32) -> bool {
        floor >= self.lowest_visible && floor <= self.highest_visible
    }
}

pub fn recompute_visible_floors(
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut range: ResMut<VisibleFloorRange>,
) {
    let _t = crate::diagnostics::SystemTimer::new("recompute_visible_floors", 1.0);
    let Some(player_pos) = client_state.player_position else {
        return;
    };
    let player_floor = floor_index(player_pos.tile_position.z);
    let player_x = player_pos.tile_position.x;
    let player_y = player_pos.tile_position.y;
    let space_id = player_pos.space_id;

    // Upper bound: same occlusion-driven scan as before, but with a much
    // larger cap (`MAX_FLOORS_ABOVE`) so that tall buildings render above an
    // uncovered player. The scan still breaks at the first `occludes_floor_above`
    // tile, so a player under a roof only sees up to the roof.
    let mut highest_visible = player_floor;
    for step in 1..=MAX_FLOORS_ABOVE {
        let floor = player_floor + step;
        // Reuses `is_indoor_tile` as the "is the player covered by something
        // on the floor above?" predicate — same semantics as outdoor-light
        // occlusion in the lighting system.
        let covered = is_indoor_tile(
            &client_state,
            &definitions,
            &floor_defs,
            space_id,
            player_x,
            player_y,
            floor - 1,
        );
        if covered {
            break;
        }
        highest_visible = floor;
    }

    // Lower bound: every floor below the player stays visible — no artificial
    // cap. `space_min_z` extends the range downward when the space has authored
    // floors below ground (e.g. a basement); the `min(_, player_floor)`
    // guarantees the player's own floor is always inside the range.
    let space_min_z = floor_min_z(&client_state, space_id, player_floor);

    range.player_floor = player_floor;
    range.player_z = player_pos.tile_position.z;
    range.lowest_visible = space_min_z.min(player_floor);
    range.highest_visible = highest_visible;
}

/// Minimum `z` of all `FloorMap`s in `space_id`, with `player_floor` as a
/// fallback when the space has no maps loaded yet.
fn floor_min_z(state: &ClientGameState, space_id: SpaceId, player_floor: i32) -> i32 {
    state
        .floor_maps
        .keys()
        .filter(|(sid, _)| *sid == space_id)
        .map(|(_, z)| *z)
        .min()
        .unwrap_or(player_floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Indoor at (5, 5, 0) only — every other tile is outdoor.
    fn indoor_55() -> IndoorTileMap {
        let mut map = IndoorTileMap::default();
        map.tiles.insert((SpaceId(1), 5, 5, 0));
        map
    }

    #[test]
    fn no_facing_tints_when_own_tile_is_indoor() {
        let indoor = indoor_55();
        assert!(should_apply_indoor_tint(&indoor, SpaceId(1), 5, 5, 0, None));
        assert!(!should_apply_indoor_tint(
            &indoor,
            SpaceId(1),
            6,
            5,
            0,
            None
        ));
    }

    #[test]
    fn south_facing_wall_tints_only_when_north_neighbor_is_indoor() {
        // South-facing wall at (5, 6, 0) reads (5, 5, 0) → indoor → tint
        // (NORTH edge of the room, back wall). South-facing wall at (5, 4, 0)
        // reads (5, 3, 0) → outdoor → no tint (SOUTH edge, alpha-faded).
        let indoor = indoor_55();
        assert!(should_apply_indoor_tint(
            &indoor,
            SpaceId(1),
            5,
            6,
            0,
            Some(Direction::South)
        ));
        assert!(!should_apply_indoor_tint(
            &indoor,
            SpaceId(1),
            5,
            4,
            0,
            Some(Direction::South)
        ));
    }

    #[test]
    fn east_facing_wall_tints_only_when_west_neighbor_is_indoor() {
        // East-facing wall at (4, 5, 0) reads (5, 5, 0) → indoor → tint
        // (WEST edge). At (6, 5, 0) reads (7, 5, 0) → outdoor → no tint.
        let indoor = indoor_55();
        assert!(should_apply_indoor_tint(
            &indoor,
            SpaceId(1),
            4,
            5,
            0,
            Some(Direction::East)
        ));
        assert!(!should_apply_indoor_tint(
            &indoor,
            SpaceId(1),
            6,
            5,
            0,
            Some(Direction::East)
        ));
    }

    #[test]
    fn north_and_west_facings_are_symmetric() {
        let indoor = indoor_55();
        // North-facing at (5, 4, 0) reads (5, 5, 0) → indoor → tint.
        assert!(should_apply_indoor_tint(
            &indoor,
            SpaceId(1),
            5,
            4,
            0,
            Some(Direction::North)
        ));
        // West-facing at (6, 5, 0) reads (5, 5, 0) → indoor → tint.
        assert!(should_apply_indoor_tint(
            &indoor,
            SpaceId(1),
            6,
            5,
            0,
            Some(Direction::West)
        ));
    }

    #[test]
    fn recompute_populates_indoor_tile_below_each_roof() {
        use crate::game::resources::ClientWorldObjectState;
        use crate::world::components::{SpacePosition, TilePosition};
        use crate::world::object_definitions::OverworldObjectDefinition;
        use std::collections::HashMap;

        let yaml = r#"
name: Roof
description: ""
colliding: true
movable: false
storable: false
render:
  z_index: 0.3
  debug_color: [0, 0, 0]
  debug_size: 1.0
  occludes_floor_above: true
"#;
        let def: OverworldObjectDefinition = serde_yaml::from_str(yaml).expect("definition parses");
        let mut defs_map = HashMap::new();
        defs_map.insert("roof".to_string(), def);
        let defs = OverworldObjectDefinitions::new_for_test(defs_map);

        let mut state = ClientGameState::default();
        // Roof on floor 1 (raw z=2 in half-block units) → covers floor 0 at (5, 5).
        state.world_objects.insert(
            1,
            ClientWorldObjectState {
                object_id: 1,
                definition_id: "roof".to_string(),
                position: SpacePosition::new(SpaceId(7), TilePosition::new(5, 5, 2)),
                tile_position: TilePosition::new(5, 5, 2),
                vitals: None,
                is_container: false,
                is_npc: false,
                is_movable: false,
                is_rotatable: false,
                quantity: 1,
                has_dialog: false,
                facing: Direction::default(),
                state: None,
                is_shopkeeper: false,
                is_hidden: false,
                is_hostile: false,
                is_targeting_local_player: false,
                placement_seq: 0,
            },
        );

        let mut app = App::new();
        app.insert_resource(state);
        app.insert_resource(defs);
        app.insert_resource(FloorTilesetDefinitions::default());
        app.insert_resource(IndoorTileMap::default());
        app.add_systems(Update, recompute_indoor_tile_map);
        app.update();

        let map = app.world().resource::<IndoorTileMap>();
        assert!(map.contains(SpaceId(7), 5, 5, 0));
        assert!(!map.contains(SpaceId(7), 5, 5, 1));
        assert!(!map.contains(SpaceId(7), 4, 5, 0));
    }

    /// A painted FloorMap tile on the floor above the player should mark the
    /// player's floor as indoor — same role as a `floor_plank` object used to
    /// play, now driven by the floor tileset's `occludes_floor_above` flag.
    #[test]
    fn recompute_populates_indoor_tile_for_painted_upper_floor() {
        use crate::game::resources::ClientGameState;
        use crate::world::floor_definitions::{FloorTilesetDefinition, FloorTilesetDefinitions};
        use crate::world::floor_map::FloorMap;
        use std::collections::HashMap;

        let space = SpaceId(9);
        let mut state = ClientGameState::default();
        // Paint one tile of "wooden_floor" at (5, 5) on floor 1 — should mark
        // floor 0 at (5, 5) as indoor.
        let mut grid = FloorMap::new_filled(10, 10, None);
        grid.set(5, 5, Some("wooden_floor".to_string()));
        state.floor_maps.insert((space, 1), grid);

        let defs = OverworldObjectDefinitions::new_for_test(HashMap::new());
        let mut floor_by_id = HashMap::new();
        floor_by_id.insert(
            "wooden_floor".to_string(),
            FloorTilesetDefinition {
                id: "wooden_floor".to_string(),
                name: "Wooden Floor".to_string(),
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
        let floor_defs = FloorTilesetDefinitions::for_test(floor_by_id, HashMap::new());

        let mut app = App::new();
        app.insert_resource(state);
        app.insert_resource(defs);
        app.insert_resource(floor_defs);
        app.insert_resource(IndoorTileMap::default());
        app.add_systems(Update, recompute_indoor_tile_map);
        app.update();

        let map = app.world().resource::<IndoorTileMap>();
        assert!(map.contains(space, 5, 5, 0));
        assert!(!map.contains(space, 4, 5, 0));
        assert!(!map.contains(space, 5, 5, 1));
    }

    /// Player on floor 0 directly under a painted upper-floor tile must have
    /// `highest_visible` clamped to 0 — the wooden_floor at floor 1 acts as
    /// a roof. Mirrors the in-game scenario where a player walks under a
    /// `wooden_floor`-painted upper storey.
    #[test]
    fn visible_floor_range_clamps_under_painted_upper_floor() {
        use crate::game::resources::ClientGameState;
        use crate::world::components::{SpacePosition, TilePosition};
        use crate::world::floor_definitions::{FloorTilesetDefinition, FloorTilesetDefinitions};
        use crate::world::floor_map::FloorMap;
        use std::collections::HashMap;

        let space = SpaceId(11);
        let mut state = ClientGameState {
            player_position: Some(SpacePosition::new(space, TilePosition::new(5, 5, 0))),
            ..Default::default()
        };
        state.floor_maps.insert((space, 0), FloorMap::default());
        let mut upper = FloorMap::new_filled(10, 10, None);
        upper.set(5, 5, Some("wooden_floor".to_string()));
        state.floor_maps.insert((space, 1), upper);

        let defs = OverworldObjectDefinitions::new_for_test(HashMap::new());
        let mut floor_by_id = HashMap::new();
        floor_by_id.insert(
            "wooden_floor".to_string(),
            FloorTilesetDefinition {
                id: "wooden_floor".to_string(),
                name: "Wooden Floor".to_string(),
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
        let floor_defs = FloorTilesetDefinitions::for_test(floor_by_id, HashMap::new());

        let mut app = App::new();
        app.insert_resource(state);
        app.insert_resource(defs);
        app.insert_resource(floor_defs);
        app.insert_resource(VisibleFloorRange::default());
        app.add_systems(Update, recompute_visible_floors);
        app.update();

        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.player_floor, 0);
        assert_eq!(
            range.highest_visible, 0,
            "painted upper floor at (player.x, player.y, floor=1) must clamp highest_visible"
        );
    }

    /// With the player on a high floor and the space's ground floor at
    /// `z = 0`, every floor below the player should be visible — no
    /// `MAX_FLOORS_BELOW` cap. Upper floors run the occlusion-driven scan to
    /// `MAX_FLOORS_ABOVE`; with no roof above the player, the full cap is used.
    #[test]
    fn visible_floor_range_uncapped_below_and_scans_above() {
        use crate::game::resources::ClientGameState;
        use crate::world::components::{SpacePosition, TilePosition};
        use crate::world::floor_map::FloorMap;
        use crate::world::object_definitions::OverworldObjectDefinitions;
        use std::collections::HashMap;

        let space = SpaceId(3);
        // Player on floor 2 = raw z=4 in half-block units.
        let mut state = ClientGameState {
            player_position: Some(SpacePosition::new(space, TilePosition::new(0, 0, 4))),
            ..Default::default()
        };
        // Only the ground floor has a `FloorMap` entry — matches how this
        // codebase actually populates `floor_maps`. Upper-story content lives
        // in `world_objects`, not `floor_maps`.
        state.floor_maps.insert((space, 0), FloorMap::default());

        let defs = OverworldObjectDefinitions::new_for_test(HashMap::new());

        let mut app = App::new();
        app.insert_resource(state);
        app.insert_resource(defs);
        app.insert_resource(FloorTilesetDefinitions::default());
        app.insert_resource(VisibleFloorRange::default());
        app.add_systems(Update, recompute_visible_floors);
        app.update();

        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.player_floor, 2);
        assert_eq!(range.lowest_visible, 0, "no MAX_FLOORS_BELOW cap");
        assert_eq!(
            range.highest_visible,
            2 + MAX_FLOORS_ABOVE,
            "no roof above player → full upper scan"
        );
    }
}
