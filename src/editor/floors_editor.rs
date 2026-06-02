//! Editor-side floor visibility — mirrors the in-game Tibia-style occlusion
//! rules (`crate::world::floors`) but sources the "viewer" from the editor
//! cursor instead of a player entity. The same `VisibleFloorRange` resource
//! the in-game systems write is reused: the in-game writer is gated to
//! `ClientAppState::InGame` and the editor writer to `MapEditor`, so the two
//! never contend.
//!
//! The effect: hovering at active floor z=N under a roof tile hides that roof
//! and everything above, exposing z=N for editing. Hovering outside opens the
//! view back up. Same logic regardless of which floor is currently active —
//! the cursor is always treated as a virtual player at
//! `(cursor.x, cursor.y, current_editing_floor)`.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::resources::{EditorCamera, EditorContext, EditorState};
use crate::editor::systems::cursor_to_tile_pub;
use crate::editor::ui::EditorPanelRoots;
use crate::player::components::Player;
use crate::world::components::{
    floor_index, OverworldObject, SpaceId, SpaceResident, TilePosition,
};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::floors::{floormap_tile_occludes, VisibleFloorRange, MAX_FLOORS_ABOVE};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::WorldConfig;

/// Cursor-derived "viewer tile" used to drive editor visibility. `None` when
/// the cursor isn't over a valid map tile (off-window, over a docked UI
/// panel, or off-map) — handled by the visibility recompute as "open the
/// view, no occlusion."
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct EditorHoverTile {
    pub tile: Option<TilePosition>,
}

/// Update `EditorHoverTile` from the primary window's cursor each frame.
/// Mirrors the cursor → tile branch already used by `update_editor_cursor_ghost`
/// (`src/editor/systems.rs:580-599`): window → cursor pos → panel-occlusion
/// → screen-to-tile → bounds check.
pub fn sync_editor_hover_tile(
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    panel_roots: EditorPanelRoots,
    mut hover: ResMut<EditorHoverTile>,
) {
    let Ok(window) = windows.single() else {
        hover.tile = None;
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        hover.tile = None;
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        hover.tile = None;
        return;
    }
    let tile = cursor_to_tile_pub(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        hover.tile = None;
        return;
    }
    hover.tile = Some(tile);
}

/// Editor-side occluder test: does floor `floor_idx + 1` at `(x, y)` in
/// `space_id` cover the tile below?
///
/// Mirrors the in-game `is_indoor_tile` rule (`src/world/floors.rs`), but
/// against the editor's data sources: ECS-backed `OverworldObject` entities
/// (not `ClientGameState.world_objects`, which is empty in `MapEditor` mode)
/// and the server-side `FloorMaps` resource. The FloorMap branch is delegated
/// to the shared `floormap_tile_occludes` helper so painted upper floors act
/// as occluders in both runtime modes.
pub fn editor_is_indoor_tile(
    objects: &Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    definitions: &OverworldObjectDefinitions,
    floor_maps: &FloorMaps,
    floor_defs: &FloorTilesetDefinitions,
    space_id: SpaceId,
    x: i32,
    y: i32,
    floor_idx: i32,
) -> bool {
    let object_occluder = objects.iter().any(|(object, resident, tile)| {
        if resident.space_id != space_id {
            return false;
        }
        if tile.x != x || tile.y != y {
            return false;
        }
        if floor_index(tile.z) != floor_idx + 1 {
            return false;
        }
        definitions
            .get(&object.definition_id)
            .is_some_and(|def| def.render.occludes_floor_above)
    });
    if object_occluder {
        return true;
    }
    floormap_tile_occludes(floor_maps.get(space_id, floor_idx + 1), floor_defs, x, y)
}

/// Recompute `VisibleFloorRange` from the editor cursor each frame. Mirrors
/// `recompute_visible_floors` (`src/world/floors.rs:148`) but uses
/// `(hover.x, hover.y, current_editing_floor)` as the virtual viewer.
///
/// Upper bound: scan up to `MAX_FLOORS_ABOVE` floors above the active floor,
/// breaking at the first occluder above the cursor. When the cursor is
/// invalid (`hover.tile == None`), skip the scan and open the view fully —
/// see plan "Cursor-off behavior" decision.
///
/// Lower bound: never capped. Min of (any authored floor below in this space)
/// and the active floor, so the active floor is always inside the range.
pub fn editor_recompute_visible_floors(
    hover: Res<EditorHoverTile>,
    editor_state: Res<EditorState>,
    editor_context: Res<EditorContext>,
    floor_maps: Res<FloorMaps>,
    definitions: Res<OverworldObjectDefinitions>,
    floor_defs: Res<FloorTilesetDefinitions>,
    objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut range: ResMut<VisibleFloorRange>,
) {
    let player_floor = editor_state.current_editing_floor;
    let player_z = editor_state.active_object_raw_z();
    let space_id = editor_context.space_id;

    let highest_visible = match hover.tile {
        None => player_floor + MAX_FLOORS_ABOVE,
        Some(tile) => {
            let mut highest = player_floor;
            for step in 1..=MAX_FLOORS_ABOVE {
                let floor = player_floor + step;
                let covered = editor_is_indoor_tile(
                    &objects,
                    &definitions,
                    &floor_maps,
                    &floor_defs,
                    space_id,
                    tile.x,
                    tile.y,
                    floor - 1,
                );
                if covered {
                    break;
                }
                highest = floor;
            }
            highest
        }
    };

    let space_min_floor = floor_maps
        .iter()
        .filter(|(sid, _, _)| *sid == space_id)
        .map(|(_, z, _)| z)
        .min()
        .unwrap_or(player_floor);

    range.player_floor = player_floor;
    range.player_z = player_z;
    range.lowest_visible = space_min_floor.min(player_floor);
    range.highest_visible = highest_visible;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::resources::{EditorContext, EditorState};
    use crate::world::floor_definitions::{
        FloorTilesetDefinition, FloorTilesetDefinitions, FloorTypeId,
    };
    use crate::world::floor_map::FloorMap;
    use crate::world::object_definitions::OverworldObjectDefinition;
    use std::collections::HashMap;

    const SPACE: SpaceId = SpaceId(7);

    fn make_app(current_editing_floor: i32, hover: Option<TilePosition>) -> App {
        // One "roof" def with occludes_floor_above. Tests place actual
        // occluders by spawning ECS entities with this id.
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
        let roof: OverworldObjectDefinition = serde_yaml::from_str(yaml).expect("def parses");
        let mut defs_map = HashMap::new();
        defs_map.insert("roof".to_string(), roof);
        let defs = OverworldObjectDefinitions::new_for_test(defs_map);

        // FloorTilesetDefinitions with a "wooden_planks" entry that has
        // occludes_floor_above + walkable_surface — used by the
        // painted-FloorMap-clamps-upper test below.
        let mut floor_by_id = HashMap::new();
        floor_by_id.insert(
            "wooden_planks".to_string(),
            FloorTilesetDefinition {
                id: "wooden_planks".to_string(),
                name: "Wooden Planks".to_string(),
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

        let mut floor_maps = FloorMaps::default();
        floor_maps.insert(SPACE, 0, FloorMap::default());

        let editor_context = EditorContext {
            space_id: SPACE,
            authored_id: "test".into(),
            map_width: 100,
            map_height: 100,
            fill_floor_type: "grass".into(),
        };
        let mut editor_state = EditorState::default();
        editor_state.current_editing_floor = current_editing_floor;

        let mut app = App::new();
        app.insert_resource(defs);
        app.insert_resource(floor_defs);
        app.insert_resource(floor_maps);
        app.insert_resource(editor_context);
        app.insert_resource(editor_state);
        app.insert_resource(EditorHoverTile { tile: hover });
        app.insert_resource(VisibleFloorRange::default());
        app.add_systems(Update, editor_recompute_visible_floors);
        app
    }

    fn spawn_roof(app: &mut App, x: i32, y: i32, z: i32) {
        app.world_mut().spawn((
            OverworldObject {
                object_id: 1,
                definition_id: "roof".to_string(),
                placement_seq: 0,
            },
            SpaceResident { space_id: SPACE },
            TilePosition::new(x, y, z),
        ));
    }

    /// Cursor under a roof on z=2 → active floor 0 covered → upper bound clamps to 0.
    #[test]
    fn cursor_under_roof_clamps_upper_visible_to_active_floor() {
        let mut app = make_app(0, Some(TilePosition::new(5, 5, 0)));
        spawn_roof(&mut app, 5, 5, 2);
        app.update();
        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.player_floor, 0);
        assert_eq!(range.highest_visible, 0, "roof above cursor hides upper");
        assert_eq!(range.lowest_visible, 0);
    }

    /// Cursor away from the roof → open sky → upper bound runs to the cap.
    #[test]
    fn cursor_outside_building_opens_view_to_cap() {
        let mut app = make_app(0, Some(TilePosition::new(8, 8, 0)));
        spawn_roof(&mut app, 5, 5, 2);
        app.update();
        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.highest_visible, 0 + MAX_FLOORS_ABOVE);
    }

    /// No hover → same fallback as "outside" — full upper cap, no occlusion.
    #[test]
    fn no_hover_opens_view_to_cap() {
        let mut app = make_app(0, None);
        spawn_roof(&mut app, 5, 5, 2);
        app.update();
        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.highest_visible, 0 + MAX_FLOORS_ABOVE);
    }

    /// Active floor=1, cursor at (5,5), occluder at (5,5,z=4) → floor above
    /// active is covered. Lower bound stays 0 (always visible below).
    #[test]
    fn active_floor_one_with_roof_above_clamps_upper() {
        let mut app = make_app(1, Some(TilePosition::new(5, 5, 2)));
        spawn_roof(&mut app, 5, 5, 4);
        app.update();
        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.player_floor, 1);
        assert_eq!(range.highest_visible, 1);
        assert_eq!(range.lowest_visible, 0, "ground floor still visible below");
    }

    /// Painted FloorMap entry on the floor above the cursor should clamp
    /// upper-visible — even without a world-object occluder. This is the
    /// "interior of a building" case the user hit: walls/roofs on the
    /// perimeter don't extend over the interior, but the upper-floor paint
    /// does, and we want to hide it so z=0 stays editable underneath.
    #[test]
    fn painted_floormap_above_cursor_clamps_upper() {
        let mut app = make_app(0, Some(TilePosition::new(5, 5, 0)));
        // Paint a single tile at (5, 5) on floor 1 — no world-object occluder
        // anywhere.
        let mut grid = FloorMap::new_filled(10, 10, None);
        grid.set(5, 5, Some(FloorTypeId::from("wooden_planks")));
        app.world_mut()
            .resource_mut::<FloorMaps>()
            .insert(SPACE, 1, grid);
        app.update();
        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.highest_visible, 0, "painted floor above hides upper");
        assert_eq!(range.lowest_visible, 0);
    }

    /// Painted FloorMap entry one tile off the cursor column shouldn't
    /// clamp — only the cursor's `(x, y)` matters.
    #[test]
    fn painted_floormap_off_cursor_column_does_not_clamp() {
        let mut app = make_app(0, Some(TilePosition::new(5, 5, 0)));
        let mut grid = FloorMap::new_filled(10, 10, None);
        grid.set(6, 5, Some(FloorTypeId::from("wooden_planks")));
        app.world_mut()
            .resource_mut::<FloorMaps>()
            .insert(SPACE, 1, grid);
        app.update();
        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.highest_visible, 0 + MAX_FLOORS_ABOVE);
    }

    /// Occluder on the WRONG tile shouldn't clamp — the cursor's column is
    /// the only one that matters.
    #[test]
    fn occluder_off_cursor_column_does_not_clamp() {
        let mut app = make_app(0, Some(TilePosition::new(5, 5, 0)));
        // Roof one tile north of the cursor — cursor itself is uncovered.
        spawn_roof(&mut app, 5, 4, 2);
        app.update();
        let range = *app.world().resource::<VisibleFloorRange>();
        assert_eq!(range.highest_visible, 0 + MAX_FLOORS_ABOVE);
    }
}
