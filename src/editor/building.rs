//! Building-draw tool. While `EditorTool::BuildingDraw` is active, a
//! drag-LMB writes a marquee rectangle (reusing the cyan selection gizmo for
//! visual feedback) and on release stamps walls around the perimeter plus
//! floor inside the rectangle, driven by the active `BuildingPreset`.
//!
//! The heavy lifting reuses the clipboard's `stamp_fragment`: this module just
//! builds a `MapFragment` whose objects are the right wall `type_id`s for each
//! perimeter tile and whose floors carry the chosen floor id. `stamp_fragment`
//! handles entity spawn, visual attachment, floor diffing, and bundles the
//! whole thing into one `UndoOp::Composite` so the building undoes in a
//! single Ctrl+Z.
//!
//! Door placement is a separate, post-draw flow: see the early branch in
//! `handle_editor_left_click` in `crate::editor::systems`.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::clipboard::stamp_fragment;
use crate::editor::resources::{
    EditorCamera, EditorContext, EditorSelection, EditorState, EditorTool, FragmentFloor,
    FragmentObject, MapFragment, UndoOp, UndoStack,
};
use crate::editor::systems::insert_editor_visuals_pub;
use crate::game::resources::PendingGameCommands;
use crate::player::components::Player;
use crate::world::building_presets::{BuildingPreset, BuildingPresets, WallSlots};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::floor_map::FloorMaps;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::spawn_overworld_object;
use crate::world::WorldConfig;

/// Drag state local to `handle_editor_building_draw_drag`. `anchor` is the
/// tile where LMB went down; `None` means no drag in progress. Mirrors
/// `selection::SelectDragState` (which keeps its field private to its own
/// module, so we can't share it directly).
#[derive(Default)]
pub struct BuildingDragState {
    anchor: Option<TilePosition>,
}

/// Bundle read-only-ish definitions used by the building drag commit so the
/// containing system stays under Bevy's 16-param cap. All fields are
/// borrowed once per call.
#[derive(SystemParam)]
pub struct BuildingDefsAndMaps<'w> {
    pub object_definitions: Res<'w, OverworldObjectDefinitions>,
    pub presets: Res<'w, BuildingPresets>,
    pub floor_maps: Res<'w, FloorMaps>,
}

/// Drag handler for `EditorTool::BuildingDraw`. Modeled on
/// `handle_editor_pick_rect_drag` (`crate::editor::selection`) — same anchor-
/// on-mousedown, same cursor-to-tile clamp, same write of `editor_state.selection`
/// during the drag so the existing `render_selection` cyan rect is the live
/// preview for free. On mouse release, the rectangle is committed via
/// `stamp_building` and the marquee is cleared.
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_building_draw_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    defs_and_maps: BuildingDefsAndMaps,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut editor_state: ResMut<EditorState>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    mut pending_commands: ResMut<PendingGameCommands>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    mut commands: Commands,
    mut state: Local<BuildingDragState>,
) {
    if editor_state.current_tool != EditorTool::BuildingDraw {
        state.anchor = None;
        return;
    }
    // While door-arm is active, the same click is consumed by
    // `handle_editor_building_door_swap_click`; suppress drag-start so we
    // don't leave a 1×1 marquee behind after a door swap.
    if editor_state.building.place_door_armed {
        state.anchor = None;
        return;
    }

    if !mouse.pressed(MouseButton::Left) {
        // Mouse released — commit if we had an anchor and a selection.
        if state.anchor.is_some() {
            commit_drag(
                &mut editor_state,
                &editor_context,
                &defs_and_maps.presets,
                &defs_and_maps.object_definitions,
                &defs_and_maps.floor_maps,
                &mut object_registry,
                &mut undo_stack,
                &mut pending_commands,
                &world_config,
                &editor_camera,
                &asset_server,
                &mut texture_atlas_layouts,
                &mut commands,
            );
        }
        state.anchor = None;
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }

    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);
    let clamped = TilePosition::ground(
        tile.x.clamp(0, editor_context.map_width - 1),
        tile.y.clamp(0, editor_context.map_height - 1),
    );

    if mouse.just_pressed(MouseButton::Left) {
        state.anchor = Some(clamped);
        editor_state.selection = None;
    }

    let Some(anchor) = state.anchor else { return };
    let min_x = anchor.x.min(clamped.x);
    let max_x = anchor.x.max(clamped.x);
    let min_y = anchor.y.min(clamped.y);
    let max_y = anchor.y.max(clamped.y);
    editor_state.selection = Some(EditorSelection {
        space_id: editor_context.space_id,
        min: TilePosition::ground(min_x, min_y),
        max: TilePosition::ground(max_x, max_y),
    });
}

#[allow(clippy::too_many_arguments)]
fn commit_drag(
    editor_state: &mut EditorState,
    editor_context: &EditorContext,
    presets: &BuildingPresets,
    object_definitions: &OverworldObjectDefinitions,
    floor_maps: &FloorMaps,
    object_registry: &mut ObjectRegistry,
    undo_stack: &mut UndoStack,
    pending_commands: &mut PendingGameCommands,
    world_config: &WorldConfig,
    editor_camera: &EditorCamera,
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    commands: &mut Commands,
) {
    let Some(sel) = editor_state
        .selection
        .filter(|s| s.space_id == editor_context.space_id)
    else {
        return;
    };
    // Need at least 3 tiles per side for there to be a meaningful interior;
    // a 2×N "building" is just two parallel walls touching, no inside. We
    // still allow it (the user might want a wall corridor) but log so the
    // user knows what they got.
    if sel.width() < 2 || sel.height() < 2 {
        warn!(
            "Building tool: rectangle too small ({}×{}); need at least 2×2",
            sel.width(),
            sel.height()
        );
        editor_state.selection = None;
        return;
    }

    let Some(preset_id) = editor_state.building.selected_preset_id.clone() else {
        warn!("Building tool: no preset selected");
        editor_state.selection = None;
        return;
    };
    let Some(preset) = presets.get(&preset_id) else {
        warn!("Building tool: preset '{preset_id}' not found");
        editor_state.selection = None;
        return;
    };

    let floor_id = editor_state
        .building
        .floor_override
        .clone()
        .or_else(|| preset.default_floor.clone());

    let fragment = build_fragment(sel, preset, floor_id);
    if let Some(undo) = stamp_fragment(
        &fragment,
        TilePosition::ground(sel.min.x, sel.min.y),
        editor_context,
        object_registry,
        object_definitions,
        world_config,
        editor_camera,
        asset_server,
        texture_atlas_layouts,
        floor_maps,
        pending_commands,
        commands,
    ) {
        undo_stack.push_undo(undo);
        editor_state.dirty = true;
    }
    editor_state.selection = None;
}

/// Build a `MapFragment` describing a building inscribed in `sel`. Walls go
/// on every perimeter tile (one object per tile — sprites overlap because
/// the existing wall sprites are 2-tile-wide, but each tile gets its own
/// collision and `occludes_floor_above` marker, which is what makes the
/// interior register as indoor via `is_indoor_tile`). Floor goes on every
/// interior tile if a floor id is set; perimeter tiles also get the floor
/// underneath the wall so the building reads as one continuous surface
/// once a door is later swapped in.
fn build_fragment(
    sel: EditorSelection,
    preset: &BuildingPreset,
    floor_id: Option<String>,
) -> MapFragment {
    let width = sel.width();
    let height = sel.height();
    let mut objects = Vec::with_capacity((width as usize + height as usize) * 2);
    let mut floors = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let on_perimeter = x == 0 || y == 0 || x == width - 1 || y == height - 1;
            if on_perimeter {
                let wall_id = wall_for_position(x, y, width, height, &preset.walls);
                objects.push(FragmentObject {
                    dx: x,
                    dy: y,
                    z: 0,
                    type_id: wall_id,
                    properties: Default::default(),
                    behavior: None,
                });
            }
            if floor_id.is_some() {
                floors.push(FragmentFloor {
                    dx: x,
                    dy: y,
                    floor_id: floor_id.clone(),
                });
            }
        }
    }

    MapFragment {
        width,
        height,
        objects,
        floors,
    }
}

/// Pick the wall `type_id` for tile `(x, y)` inside a `width × height`
/// rectangle (local coords; `(0, 0)` is the building's south-west tile —
/// the fragment loops y from 0 upward and `stamp_fragment` lays it out as
/// `world_tile = sel.min + (dx, dy)`, where `sel.min.y` is the southmost
/// world row because Bevy's +y = north). So `y == 0` is the building's
/// SOUTH edge and `y == height - 1` is the NORTH edge.
fn wall_for_position(x: i32, y: i32, width: i32, height: i32, walls: &WallSlots) -> String {
    let on_south = y == 0;
    let on_north = y == height - 1;
    let on_west = x == 0;
    let on_east = x == width - 1;

    match (on_north, on_south, on_west, on_east) {
        (true, _, true, _) => walls
            .corner_nw
            .clone()
            .unwrap_or_else(|| walls.north.clone()),
        (true, _, _, true) => walls
            .corner_ne
            .clone()
            .unwrap_or_else(|| walls.north.clone()),
        (_, true, true, _) => walls
            .corner_sw
            .clone()
            .unwrap_or_else(|| walls.south.clone()),
        (_, true, _, true) => walls
            .corner_se
            .clone()
            .unwrap_or_else(|| walls.south.clone()),
        (true, _, _, _) => walls.north.clone(),
        (_, true, _, _) => walls.south.clone(),
        (_, _, true, _) => walls.west.clone(),
        (_, _, _, true) => walls.east.clone(),
        // Unreachable — caller only invokes for perimeter tiles.
        _ => walls.north.clone(),
    }
}

/// Post-draw door placement. When the building tool is active and
/// `place_door_armed` is set, a left-click on a perimeter wall whose
/// `type_id` matches the active preset's wall slots replaces that wall
/// with the preset's `default_door`. Runs before `handle_editor_left_click`
/// so the click is consumed here and not by the regular brush / select
/// flow. Auto-disarms after one successful swap so each toggle = one door.
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_building_door_swap_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    presets: Res<BuildingPresets>,
    object_definitions: Res<OverworldObjectDefinitions>,
    existing_objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut editor_state: ResMut<EditorState>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    mut commands: Commands,
) {
    if editor_state.current_tool != EditorTool::BuildingDraw {
        return;
    }
    if !editor_state.building.place_door_armed {
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }
    let tile = cursor_to_tile(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }

    let Some(preset_id) = editor_state.building.selected_preset_id.clone() else {
        return;
    };
    let Some(preset) = presets.get(&preset_id) else {
        return;
    };
    let Some(door_type) = preset.default_door.clone() else {
        warn!("Building tool: preset '{preset_id}' has no default_door");
        return;
    };
    let wall_ids: Vec<String> = preset.walls.all_wall_ids().map(str::to_owned).collect();

    // Find a wall object at this tile whose type belongs to the active preset.
    let target = existing_objects.iter().find(|(obj, resident, pos)| {
        resident.space_id == editor_context.space_id
            && **pos == tile
            && object_registry
                .type_id(obj.object_id)
                .is_some_and(|ty| wall_ids.iter().any(|w| w == ty))
    });
    let Some((wall_obj, _, _)) = target else {
        return;
    };
    let wall_object_id = wall_obj.object_id;
    let wall_type_id = object_registry
        .type_id(wall_object_id)
        .map(str::to_owned)
        .unwrap_or_default();
    let wall_properties = object_registry
        .properties(wall_object_id)
        .cloned()
        .unwrap_or_default();
    let wall_behavior = object_registry.behavior(wall_object_id).cloned();

    // Despawn the wall entity. The component-tuple query above doesn't
    // surface `Entity`, so we queue a deferred world-mutator that walks the
    // world and despawns whichever entity carries this `object_id`. We
    // don't clean up the registry slot — `UndoOp::Despawn`'s redo path
    // doesn't either; type/property lookups on stale ids are harmless.
    commands.queue(move |world: &mut World| {
        let mut found: Option<bevy::ecs::entity::Entity> = None;
        let mut q = world.query::<(bevy::ecs::entity::Entity, &OverworldObject)>();
        for (entity, obj) in q.iter(world) {
            if obj.object_id == wall_object_id {
                found = Some(entity);
                break;
            }
        }
        if let Some(entity) = found {
            world.despawn(entity);
        }
    });

    // Spawn the door at the same tile.
    let Some(door_def) = object_definitions.get(&door_type) else {
        warn!("Building tool: door type '{door_type}' not in definitions");
        return;
    };
    let door_object_id = object_registry.allocate_runtime_id(door_type.clone());
    let entity = spawn_overworld_object(
        &mut commands,
        &object_definitions,
        &object_registry,
        door_object_id,
        &door_type,
        None,
        editor_context.space_id,
        tile,
        None,
    );
    insert_editor_visuals_pub(
        &mut commands.entity(entity),
        &asset_server,
        &mut texture_atlas_layouts,
        door_def,
        &world_config,
        tile,
        &editor_camera,
    );

    // Composite undo: door swap reverses to (despawn door, respawn wall).
    let undo = UndoOp::Composite {
        ops: vec![
            UndoOp::Despawn {
                object_id: door_object_id,
            },
            UndoOp::Spawn {
                type_id: wall_type_id,
                space_id: editor_context.space_id,
                tile,
                properties: wall_properties,
                behavior: wall_behavior,
            },
        ],
    };
    undo_stack.push_undo(undo);
    editor_state.dirty = true;
    editor_state.building.place_door_armed = false;
}

// Private — copy of the same helper in `selection.rs` / `systems.rs`. Three-
// line duplication is cheaper than threading a shared helper through every
// drag handler, and matches the pattern those two files already follow.
fn cursor_to_tile(
    cursor: Vec2,
    window: &Window,
    world_config: &WorldConfig,
    camera: &EditorCamera,
) -> TilePosition {
    let effective = world_config.tile_size * camera.zoom_level;
    let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
    let offset = cursor - center;
    TilePosition::ground(
        (camera.center.x + offset.x / effective).round() as i32,
        (camera.center.y - offset.y / effective).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic_walls() -> WallSlots {
        WallSlots {
            north: "wall_n".into(),
            south: "wall_s".into(),
            east: "wall_e".into(),
            west: "wall_w".into(),
            corner_nw: None,
            corner_ne: None,
            corner_sw: None,
            corner_se: None,
        }
    }

    #[test]
    fn corner_falls_back_to_horizontal_when_no_override() {
        let walls = basic_walls();
        // Local (0, 0) = SW in world coords (smallest x, smallest y) → south
        assert_eq!(wall_for_position(0, 0, 5, 4, &walls), "wall_s");
        // Local (W-1, 0) = SE → south
        assert_eq!(wall_for_position(4, 0, 5, 4, &walls), "wall_s");
        // Local (0, H-1) = NW → north
        assert_eq!(wall_for_position(0, 3, 5, 4, &walls), "wall_n");
        // Local (W-1, H-1) = NE → north
        assert_eq!(wall_for_position(4, 3, 5, 4, &walls), "wall_n");
    }

    #[test]
    fn corner_override_wins_when_set() {
        let mut walls = basic_walls();
        walls.corner_ne = Some("corner_ne_sprite".into());
        // Local (W-1, H-1) is the world-NE corner.
        assert_eq!(wall_for_position(4, 3, 5, 4, &walls), "corner_ne_sprite",);
        // Other corners still fall back since their overrides are None.
        assert_eq!(wall_for_position(0, 0, 5, 4, &walls), "wall_s");
    }

    #[test]
    fn edge_tiles_pick_the_right_side() {
        let walls = basic_walls();
        // South edge non-corner (local y=0) → south
        assert_eq!(wall_for_position(2, 0, 5, 4, &walls), "wall_s");
        // West edge non-corner (local x=0) → west
        assert_eq!(wall_for_position(0, 1, 5, 4, &walls), "wall_w");
        // East edge non-corner (local x=W-1) → east
        assert_eq!(wall_for_position(4, 2, 5, 4, &walls), "wall_e");
        // North edge non-corner (local y=H-1) → north
        assert_eq!(wall_for_position(2, 3, 5, 4, &walls), "wall_n");
    }

    #[test]
    fn fragment_covers_perimeter_and_interior() {
        let sel = EditorSelection {
            space_id: crate::world::components::SpaceId(0),
            min: TilePosition::ground(10, 5),
            max: TilePosition::ground(13, 7),
        };
        let preset = BuildingPreset {
            id: "stone".into(),
            name: "Stone".into(),
            walls: basic_walls(),
            default_floor: Some("cobblestone".into()),
            default_door: Some("wooden_door".into()),
        };
        let fragment = build_fragment(sel, &preset, preset.default_floor.clone());
        // 4×3 rectangle: perimeter = 2*(4+3) - 4 = 10 tiles.
        assert_eq!(fragment.objects.len(), 10);
        // Floor on every tile = 12.
        assert_eq!(fragment.floors.len(), 12);
        // All floor entries carry the cobblestone id.
        assert!(fragment
            .floors
            .iter()
            .all(|ff| ff.floor_id.as_deref() == Some("cobblestone")));
        // Interior tile (1, 1) is NOT in the objects list.
        assert!(!fragment.objects.iter().any(|fo| fo.dx == 1 && fo.dy == 1));
        // Local (0, 0) is the SW corner — falls back to the south wall id.
        assert!(fragment
            .objects
            .iter()
            .any(|fo| fo.dx == 0 && fo.dy == 0 && fo.type_id == "wall_s"));
    }

    #[test]
    fn no_floors_emitted_when_floor_id_none() {
        let sel = EditorSelection {
            space_id: crate::world::components::SpaceId(0),
            min: TilePosition::ground(0, 0),
            max: TilePosition::ground(2, 2),
        };
        let preset = BuildingPreset {
            id: "x".into(),
            name: "x".into(),
            walls: basic_walls(),
            default_floor: None,
            default_door: None,
        };
        let fragment = build_fragment(sel, &preset, None);
        assert!(fragment.floors.is_empty());
        assert_eq!(fragment.objects.len(), 8);
    }
}
