//! Single-purpose editor hotkeys that don't fit cleanly into the larger
//! systems (`handle_editor_keyboard_input`, `handle_editor_floor_brush_hotkey`
//! etc.). Number-row tool switches, brush-radius adjust, eyedropper,
//! multi-floor PgUp/PgDn, and fill-mode toggle all live here.
//!
//! Bindings come from [`EditorKeybindings`]; the chord-matching it performs
//! reproduces the explicit modifier guards each system used to write by hand.

#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::resources::{
    EditorCamera, EditorContext, EditorPropertyEditBuffer, EditorState, EditorTool, FillMode,
    ModalState,
};
use crate::editor::ui::EditorPanelRoots;
use crate::player::components::Player;
use crate::ui::settings::{EditorAction, EditorKeybindings};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::object_registry::ObjectRegistry;
use crate::world::WorldConfig;

/// Returns true when no modal is open, no inline text editor has focus, and
/// no palette filter is being typed — i.e. it's safe to consume editor
/// hotkeys.
fn hotkeys_allowed(
    editor_state: &EditorState,
    modal_state: &ModalState,
    prop_buffer: &EditorPropertyEditBuffer,
    vendor_stash_buffer: &crate::editor::resources::EditorVendorStashBuffer,
) -> bool {
    modal_state.active.is_none()
        && prop_buffer.editing_index.is_none()
        && vendor_stash_buffer.editing.is_none()
        && !editor_state.palette_filter_focused
}

/// Number-row keys switch tools (default 1..5 → Brush/Portal/FloorBrush/Select/BuildingDraw).
pub fn handle_editor_tool_number_hotkeys(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_keys: Res<EditorKeybindings>,
    modal_state: Res<ModalState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    if !hotkeys_allowed(&editor_state, &modal_state, &prop_buffer, &vendor_stash_buffer) {
        return;
    }
    let new_tool = if editor_keys.just_pressed(EditorAction::ToolBrush, &keyboard) {
        Some(EditorTool::Brush)
    } else if editor_keys.just_pressed(EditorAction::ToolPortal, &keyboard) {
        Some(EditorTool::Portal)
    } else if editor_keys.just_pressed(EditorAction::ToolFloorBrush, &keyboard) {
        Some(EditorTool::FloorBrush)
    } else if editor_keys.just_pressed(EditorAction::ToolSelect, &keyboard) {
        Some(EditorTool::Select)
    } else if editor_keys.just_pressed(EditorAction::ToolBuildingDraw, &keyboard) {
        Some(EditorTool::BuildingDraw)
    } else {
        None
    };
    if let Some(tool) = new_tool {
        editor_state.current_tool = tool;
    }
}

/// `[` shrinks brush, `]` grows. Bounded to 1..=8.
pub fn handle_editor_brush_radius_hotkeys(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_keys: Res<EditorKeybindings>,
    modal_state: Res<ModalState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    if !hotkeys_allowed(&editor_state, &modal_state, &prop_buffer, &vendor_stash_buffer) {
        return;
    }
    let current = editor_state.effective_brush_radius();
    if editor_keys.just_pressed(EditorAction::BrushRadiusGrow, &keyboard) {
        editor_state.brush_radius = (current + 1).min(8);
    }
    if editor_keys.just_pressed(EditorAction::BrushRadiusShrink, &keyboard) {
        editor_state.brush_radius = current.saturating_sub(1).max(1);
    }
}

/// Cycle fill mode: Single → Rect → Flood → Single.
pub fn handle_editor_fill_mode_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_keys: Res<EditorKeybindings>,
    modal_state: Res<ModalState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    if !hotkeys_allowed(&editor_state, &modal_state, &prop_buffer, &vendor_stash_buffer) {
        return;
    }
    if editor_keys.just_pressed(EditorAction::CycleFillMode, &keyboard) {
        editor_state.fill_mode = match editor_state.fill_mode {
            FillMode::Single => FillMode::Rect,
            FillMode::Rect => FillMode::Flood,
            FillMode::Flood => FillMode::Single,
        };
    }
}

/// Read the object/floor under the cursor and set it as the active brush.
/// For tiles with both an object and a floor, the object wins (you can still
/// pick the floor by hovering an empty tile that only has a floor).
pub fn handle_editor_eyedropper(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_keys: Res<EditorKeybindings>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    modal_state: Res<ModalState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
    object_registry: Res<ObjectRegistry>,
    floor_maps: Res<crate::world::floor_map::FloorMaps>,
    objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    panel_roots: EditorPanelRoots,
) {
    if !hotkeys_allowed(&editor_state, &modal_state, &prop_buffer, &vendor_stash_buffer) {
        return;
    }
    if !editor_keys.just_pressed(EditorAction::Eyedropper, &keyboard) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if panel_roots.cursor_over(cursor, window.scale_factor()) {
        return;
    }
    let tile = crate::editor::systems::cursor_to_tile_pub(cursor, window, &world_config, &editor_camera);
    if tile.x < 0
        || tile.y < 0
        || tile.x >= editor_context.map_width
        || tile.y >= editor_context.map_height
    {
        return;
    }
    // Try object first. Match by floor_index so a half-block-stacked
    // object (chest sitting on the floor's base z) still picks up.
    let hit_object = objects.iter().find(|(_, resident, pos)| {
        resident.space_id == editor_context.space_id
            && pos.x == tile.x
            && pos.y == tile.y
            && editor_state.tile_on_active_floor(pos.z)
    });
    if let Some((obj, _, _)) = hit_object {
        let type_id = object_registry
            .type_id(obj.object_id)
            .unwrap_or(&obj.definition_id)
            .to_owned();
        editor_state.current_tool = EditorTool::Brush;
        editor_state.selected_type_id = Some(type_id.clone());
        editor_state.selected_object_id = None;
        editor_state.touch_recent_object(&type_id);
        return;
    }

    // Fall back to floor under cursor. Floor maps are keyed by floor_index.
    if let Some(floor_grid) =
        floor_maps.get(editor_context.space_id, editor_state.current_editing_floor)
    {
        if let Some(floor_id) = floor_grid.get(tile.x, tile.y).cloned() {
            editor_state.current_tool = EditorTool::FloorBrush;
            editor_state.selected_floor_type = Some(floor_id.clone());
            editor_state.touch_recent_floor(&floor_id);
        }
    }
    let _ = object_registry;
}

/// PgUp / PgDn cycle the active editing floor. Maps with no upper floors
/// (most of them) only ever have z=0 to switch to. When switching to a
/// floor that has no `FloorMap` allocated yet, an empty one is created
/// on-demand so the FloorBrush has somewhere to write.
pub fn handle_editor_floor_switch_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_keys: Res<EditorKeybindings>,
    modal_state: Res<ModalState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
    editor_context: Res<EditorContext>,
    mut floor_maps: ResMut<crate::world::floor_map::FloorMaps>,
) {
    if !hotkeys_allowed(&editor_state, &modal_state, &prop_buffer, &vendor_stash_buffer) {
        return;
    }
    // Cap at floor 8 (arbitrary; the engine has no hard upper limit but
    // gameplay rarely uses more than 2-3 floors).
    let new_z = if editor_keys.just_pressed(EditorAction::FloorUp, &keyboard) {
        Some((editor_state.current_editing_floor + 1).min(8))
    } else if editor_keys.just_pressed(EditorAction::FloorDown, &keyboard) {
        Some((editor_state.current_editing_floor - 1).max(TilePosition::GROUND_FLOOR))
    } else {
        None
    };
    if let Some(z) = new_z {
        if z != editor_state.current_editing_floor {
            editor_state.current_editing_floor = z;
            if floor_maps.get(editor_context.space_id, z).is_none() {
                let empty = crate::world::floor_map::FloorMap::new_filled(
                    editor_context.map_width,
                    editor_context.map_height,
                    None,
                );
                floor_maps.insert(editor_context.space_id, z, empty);
            }
        }
    }
}

/// Select from the recent-objects strip (default Alt+1..9, slot 0 = most recent).
pub fn handle_editor_alt_recent_hotkeys(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_keys: Res<EditorKeybindings>,
    modal_state: Res<ModalState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    if !hotkeys_allowed(&editor_state, &modal_state, &prop_buffer, &vendor_stash_buffer) {
        return;
    }
    let mut picked: Option<usize> = None;
    for slot in 0..crate::ui::settings::editor::RECENT_OBJECT_SLOTS {
        if editor_keys.just_pressed(EditorAction::SelectRecent(slot), &keyboard) {
            picked = Some(slot as usize);
            break;
        }
    }
    let Some(idx) = picked else { return };
    if let Some(type_id) = editor_state.recent_object_types.get(idx).cloned() {
        editor_state.current_tool = EditorTool::Brush;
        editor_state.selected_type_id = Some(type_id.clone());
        editor_state.selected_object_id = None;
        editor_state.touch_recent_object(&type_id);
    }
}
