use bevy::prelude::*;
use std::collections::HashSet;

use crate::game::resources::ClientGameState;
use crate::world::components::SpaceId;
use crate::world::direction::Direction;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Highest floor index the camera ever renders relative to the player's floor.
/// The lookup terminates earlier when a covering tile is found. Three is enough
/// for overworld buildings; dungeons with deep towers would raise this.
const MAX_FLOORS_ABOVE: i32 = 3;

/// How many floors below the player to keep visible for depth cues. Lower
/// floors render at full alpha — the floor-screen offset is the sole depth
/// cue, so brief one-tile elevations (climbing onto a barrel) don't make the
/// whole map flicker between bright and dim.
const MAX_FLOORS_BELOW: i32 = 3;

/// True iff `(x, y, z)` is "indoor" — i.e. some object on `(x, y, z+1)` in
/// `space_id` has `render.occludes_floor_above = true`. The same predicate
/// powers floor-roof culling (`recompute_visible_floors`) and outdoor-light
/// occlusion in the lighting system.
pub fn is_indoor_tile(
    state: &ClientGameState,
    definitions: &OverworldObjectDefinitions,
    space_id: SpaceId,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    state.world_objects.values().any(|object| {
        if object.position.space_id != space_id || object.tile_position.z != z + 1 {
            return false;
        }
        if object.tile_position.x != x || object.tile_position.y != y {
            return false;
        }
        definitions
            .get(&object.definition_id)
            .is_some_and(|def| def.render.occludes_floor_above)
    })
}

/// Cached set of indoor tiles for the current frame. Rebuilt in
/// `recompute_indoor_tile_map` from one sweep over `world_objects`, so the
/// per-frame consumers (`sync_tile_transforms`, `sync_floor_render_transforms`)
/// can answer "is `(space, x, y, z)` indoor?" in O(1). Without this cache the
/// floor-cell sync ran 4× O(world_objects) per cell — pathological on the 70×50
/// overworld.
#[derive(Resource, Default, Clone, Debug)]
pub struct IndoorTileMap {
    tiles: HashSet<(SpaceId, i32, i32, i32)>,
}

impl IndoorTileMap {
    pub fn contains(&self, space_id: SpaceId, x: i32, y: i32, z: i32) -> bool {
        self.tiles.contains(&(space_id, x, y, z))
    }
}

/// Build the per-frame `IndoorTileMap` from one sweep over `world_objects`.
/// Schedules after `apply_game_events_to_client_state` so the set reflects the
/// latest replicated state. Matches the predicate in `is_indoor_tile`: an
/// object on `(x, y, z+1)` with `occludes_floor_above` makes `(x, y, z)` indoor.
pub fn recompute_indoor_tile_map(
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
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
            tile.z - 1,
        ));
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
    z: i32,
    hide_when_inside_facing: Option<Direction>,
) -> bool {
    let (sx, sy) = match hide_when_inside_facing {
        None => (x, y),
        Some(Direction::South) => (x, y - 1),
        Some(Direction::East) => (x + 1, y),
        Some(Direction::North) => (x, y + 1),
        Some(Direction::West) => (x - 1, y),
    };
    indoor.contains(space_id, sx, sy, z)
}

/// Window of floor indices currently rendered on the client. Recomputed each
/// frame from the local player's position plus the covering tiles above them.
/// Consumed by `sync_tile_transforms` to cull/dim by floor.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct VisibleFloorRange {
    pub player_floor: i32,
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
    mut range: ResMut<VisibleFloorRange>,
) {
    let _t = crate::diagnostics::SystemTimer::new("recompute_visible_floors", 1.0);
    let Some(player_pos) = client_state.player_position else {
        return;
    };
    let player_floor = player_pos.tile_position.z;
    let player_x = player_pos.tile_position.x;
    let player_y = player_pos.tile_position.y;
    let space_id = player_pos.space_id;

    let mut highest_visible = player_floor;
    for step in 1..=MAX_FLOORS_ABOVE {
        let floor = player_floor + step;
        // Reuses `is_indoor_tile` as the "is the player covered by something
        // on the floor above?" predicate — same semantics as outdoor-light
        // occlusion in the lighting system.
        let covered = is_indoor_tile(
            &client_state,
            &definitions,
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

    range.player_floor = player_floor;
    range.lowest_visible = player_floor - MAX_FLOORS_BELOW;
    range.highest_visible = highest_visible;
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
        assert!(!should_apply_indoor_tint(&indoor, SpaceId(1), 6, 5, 0, None));
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
        let def: OverworldObjectDefinition =
            serde_yaml::from_str(yaml).expect("definition parses");
        let mut defs_map = HashMap::new();
        defs_map.insert("roof".to_string(), def);
        let defs = OverworldObjectDefinitions::new_for_test(defs_map);

        let mut state = ClientGameState::default();
        // Object at (5, 5, 1) → indoor at (5, 5, 0).
        state.world_objects.insert(
            1,
            ClientWorldObjectState {
                object_id: 1,
                definition_id: "roof".to_string(),
                position: SpacePosition::new(SpaceId(7), TilePosition::new(5, 5, 1)),
                tile_position: TilePosition::new(5, 5, 1),
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
            },
        );

        let mut app = App::new();
        app.insert_resource(state);
        app.insert_resource(defs);
        app.insert_resource(IndoorTileMap::default());
        app.add_systems(Update, recompute_indoor_tile_map);
        app.update();

        let map = app.world().resource::<IndoorTileMap>();
        assert!(map.contains(SpaceId(7), 5, 5, 0));
        assert!(!map.contains(SpaceId(7), 5, 5, 1));
        assert!(!map.contains(SpaceId(7), 4, 5, 0));
    }
}
