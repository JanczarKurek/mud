//! Rectangle and flood fill for the object/floor brush. Both modes are
//! armed by toggling `EditorState.fill_mode` via the `G` hotkey; commits
//! land as a single `UndoOp::Composite` so the whole stroke undoes in one
//! step.

#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::resources::{
    EditorCamera, EditorContext, EditorState, EditorTool, FillMode, UndoOp, UndoStack,
};
use crate::editor::systems::cursor_to_tile_pub;
use crate::editor::ui::EditorPanelRoots;
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::player::components::Player;
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::floor_map::FloorMaps;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::spawn_overworld_object;
use crate::world::WorldConfig;

// Suppress dead-import lints — these are part of the public surface for
// follow-up features (line tool, brush radius rect-fill) but not all
// branches use them today.
#[allow(unused_imports)]
use crate::world::components::SpaceId;

/// Local state for the rect-fill drag: where the LMB went down. Reset
/// when the button releases or the user switches modes.
#[derive(Default)]
pub struct RectFillDragState {
    pub anchor: Option<TilePosition>,
}

/// On LMB-press in rect mode, store anchor. On LMB-release, stamp the
/// rectangle. Works for both object and floor brushes.
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_rect_fill(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    definitions: Res<OverworldObjectDefinitions>,
    mut editor_state: ResMut<EditorState>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    mut pending_commands: ResMut<PendingGameCommands>,
    mut drag: Local<RectFillDragState>,
    existing_objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut commands: Commands,
    panel_roots: EditorPanelRoots,
) {
    if editor_state.fill_mode != FillMode::Rect {
        drag.anchor = None;
        return;
    }
    if !matches!(
        editor_state.current_tool,
        EditorTool::Brush | EditorTool::FloorBrush
    ) {
        drag.anchor = None;
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }

    if mouse.just_pressed(MouseButton::Left) {
        let tile = cursor_to_tile_pub(cursor, window, &world_config, &editor_camera);
        drag.anchor = Some(tile);
        return;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(anchor) = drag.anchor.take() else {
        return;
    };
    let end = cursor_to_tile_pub(cursor, window, &world_config, &editor_camera);

    // Two units in play: objects use raw half-block z (floor_index * 2)
    // because that's what `TilePosition.z` lives in; floor maps are keyed
    // by floor_index. Pick the right one per branch.
    let object_z = editor_state.active_object_raw_z();
    let floor_map_z = editor_state.current_editing_floor;
    let (min_x, max_x) = (anchor.x.min(end.x), anchor.x.max(end.x));
    let (min_y, max_y) = (anchor.y.min(end.y), anchor.y.max(end.y));

    let mut composite: Vec<UndoOp> = Vec::new();

    match editor_state.current_tool {
        EditorTool::Brush => {
            let Some(type_id) = editor_state.selected_type_id.clone() else {
                return;
            };
            let Some(def) = definitions.get(&type_id).cloned() else {
                return;
            };
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    if x < 0
                        || y < 0
                        || x >= editor_context.map_width
                        || y >= editor_context.map_height
                    {
                        continue;
                    }
                    let tile = TilePosition {
                        x,
                        y,
                        z: object_z,
                    };
                    // Skip cells that already have an object of the same type
                    // to keep rect-fill idempotent.
                    let occupied = existing_objects.iter().any(|(o, r, p)| {
                        r.space_id == editor_context.space_id
                            && *p == tile
                            && o.definition_id == type_id
                    });
                    if occupied {
                        continue;
                    }
                    let object_id = object_registry.allocate_runtime_id(type_id.clone());
                    let entity = spawn_overworld_object(
                        &mut commands,
                        &definitions,
                        &object_registry,
                        object_id,
                        &type_id,
                        None,
                        editor_context.space_id,
                        tile,
                        None,
                    );
                    let _ = entity; // Visual attachment runs the next frame.
                    composite.push(UndoOp::Despawn { object_id });
                }
            }
            let _ = def;
        }
        EditorTool::FloorBrush => {
            let floor_type = editor_state.selected_floor_type.clone();
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    if x < 0
                        || y < 0
                        || x >= editor_context.map_width
                        || y >= editor_context.map_height
                    {
                        continue;
                    }
                    pending_commands.push(GameCommand::EditorSetFloorTile {
                        space_id: editor_context.space_id,
                        z: floor_map_z,
                        x,
                        y,
                        floor_type: floor_type.clone(),
                    });
                }
            }
        }
        _ => {}
    }

    if !composite.is_empty() {
        undo_stack.push_undo(UndoOp::Composite { ops: composite });
        editor_state.dirty = true;
    } else if matches!(editor_state.current_tool, EditorTool::FloorBrush) {
        // Floor commands write asynchronously through CommandIntercept and
        // each emits its own undo entry via `process_floor_commands`, so
        // there's no Composite op to push here. Just mark dirty.
        editor_state.dirty = true;
    }
}

/// Flood-fill the contiguous region matching the tile under the cursor.
/// Triggered by LMB click while `fill_mode == Flood`. For objects: flood
/// over empty cells, stamping the selected type. For floors: flood the
/// contiguous floor-id region with the selected floor.
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_flood_fill(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    definitions: Res<OverworldObjectDefinitions>,
    mut editor_state: ResMut<EditorState>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    mut pending_commands: ResMut<PendingGameCommands>,
    floor_maps: Res<FloorMaps>,
    existing_objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut commands: Commands,
    panel_roots: EditorPanelRoots,
) {
    if editor_state.fill_mode != FillMode::Flood {
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if !matches!(
        editor_state.current_tool,
        EditorTool::Brush | EditorTool::FloorBrush
    ) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }
    let seed = cursor_to_tile_pub(cursor, window, &world_config, &editor_camera);
    // Objects use raw half-block z; floor maps use floor_index. Cache both.
    let object_z = editor_state.active_object_raw_z();
    let floor_map_z = editor_state.current_editing_floor;
    if seed.x < 0
        || seed.y < 0
        || seed.x >= editor_context.map_width
        || seed.y >= editor_context.map_height
    {
        return;
    }

    let cells = flood_region(
        seed.x,
        seed.y,
        editor_context.map_width,
        editor_context.map_height,
        |x, y| match editor_state.current_tool {
            EditorTool::Brush => {
                // Match empty cells (no object whose footprint is on the
                // active floor). Compare by floor_index, not raw z, so a
                // half-block-stacked object (chest at z=2+1) still counts
                // as occupying floor 1.
                !existing_objects.iter().any(|(_, r, p)| {
                    r.space_id == editor_context.space_id
                        && p.x == x
                        && p.y == y
                        && editor_state.tile_on_active_floor(p.z)
                })
            }
            EditorTool::FloorBrush => {
                let seed_floor = floor_maps
                    .get(editor_context.space_id, floor_map_z)
                    .and_then(|m| m.get(seed.x, seed.y).cloned());
                let here = floor_maps
                    .get(editor_context.space_id, floor_map_z)
                    .and_then(|m| m.get(x, y).cloned());
                seed_floor == here
            }
            _ => false,
        },
    );

    let mut composite: Vec<UndoOp> = Vec::new();

    match editor_state.current_tool {
        EditorTool::Brush => {
            let Some(type_id) = editor_state.selected_type_id.clone() else {
                return;
            };
            for (x, y) in cells {
                let tile = TilePosition {
                    x,
                    y,
                    z: object_z,
                };
                let object_id = object_registry.allocate_runtime_id(type_id.clone());
                let _ = spawn_overworld_object(
                    &mut commands,
                    &definitions,
                    &object_registry,
                    object_id,
                    &type_id,
                    None,
                    editor_context.space_id,
                    tile,
                    None,
                );
                composite.push(UndoOp::Despawn { object_id });
            }
        }
        EditorTool::FloorBrush => {
            let floor_type = editor_state.selected_floor_type.clone();
            for (x, y) in cells {
                pending_commands.push(GameCommand::EditorSetFloorTile {
                    space_id: editor_context.space_id,
                    z: floor_map_z,
                    x,
                    y,
                    floor_type: floor_type.clone(),
                });
            }
        }
        _ => {}
    }

    if !composite.is_empty() {
        undo_stack.push_undo(UndoOp::Composite { ops: composite });
    }
    editor_state.dirty = true;
}

/// 4-connected flood fill, returning the list of (x, y) cells matching the
/// predicate from the seed. Bounded by `(width, height)`. Iterative — the
/// stack version blows up on big maps.
pub fn flood_region(
    seed_x: i32,
    seed_y: i32,
    width: i32,
    height: i32,
    matches: impl Fn(i32, i32) -> bool,
) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    if !matches(seed_x, seed_y) {
        return out;
    }
    let mut seen = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back((seed_x, seed_y));
    seen.insert((seed_x, seed_y));
    // Cap to avoid runaway on accidental match-everything predicates.
    const MAX_CELLS: usize = 16_384;
    while let Some((x, y)) = queue.pop_front() {
        if x < 0 || y < 0 || x >= width || y >= height {
            continue;
        }
        out.push((x, y));
        if out.len() >= MAX_CELLS {
            break;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || ny < 0 || nx >= width || ny >= height {
                continue;
            }
            if !seen.insert((nx, ny)) {
                continue;
            }
            if matches(nx, ny) {
                queue.push_back((nx, ny));
            }
        }
    }
    out
}
