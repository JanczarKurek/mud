//! Marquee-rectangle selection for the map editor.
//!
//! `EditorTool::Select` (hotkey `M`): drag-LMB writes
//! `EditorState::selection`. The selection persists across tool switches and
//! is consumed by `clipboard.rs` (Ctrl+C / Ctrl+X) and the
//! "Save Selection as Template" toolbar button. Esc clears it (handled in
//! `handle_editor_escape`).
//!
//! Rendering is a single cyan `gizmos.rect_2d` over the selection bbox, drawn
//! by `render_selection`. We deliberately *don't* render anything mid-drag
//! beyond what's already in `editor_state.selection` — the drag system writes
//! the selection every frame, so the live rect and the committed rect coincide.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::resources::{
    EditorCamera, EditorContext, EditorPickRectResult, EditorSelection, EditorState, EditorTool,
    PickedRect,
};
use crate::world::components::TilePosition;
use crate::world::map_layout::TileRectangle;
use crate::world::WorldConfig;

/// Drag-state local to `handle_editor_select_drag`. `anchor` is the tile where
/// the LMB went down; `None` means no drag in progress.
#[derive(Default)]
pub struct SelectDragState {
    anchor: Option<TilePosition>,
}

#[allow(clippy::too_many_arguments)]
pub fn handle_editor_select_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut editor_state: ResMut<EditorState>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    mut state: Local<SelectDragState>,
) {
    if editor_state.current_tool != EditorTool::Select || editor_state.paste_state.active {
        state.anchor = None;
        return;
    }
    if !mouse.pressed(MouseButton::Left) {
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

/// Draws a cyan outline around the active selection. Runs every frame in the
/// editor regardless of tool — the user wants to see the selection while
/// switching to Brush/Floor for placement.
pub fn render_selection(
    mut gizmos: Gizmos,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_state: Res<EditorState>,
    editor_context: Res<EditorContext>,
) {
    let Some(sel) = editor_state.selection else {
        return;
    };
    if sel.space_id != editor_context.space_id {
        return;
    }
    let effective = world_config.tile_size * editor_camera.zoom_level;
    if effective <= f32::EPSILON {
        return;
    }
    // Bbox in tile coordinates (inclusive). Convert center+size to world space.
    let min_world_x = (sel.min.x as f32 - 0.5 - editor_camera.center.x) * effective;
    let max_world_x = (sel.max.x as f32 + 0.5 - editor_camera.center.x) * effective;
    let min_world_y = (sel.min.y as f32 - 0.5 - editor_camera.center.y) * effective;
    let max_world_y = (sel.max.y as f32 + 0.5 - editor_camera.center.y) * effective;
    let center = Vec2::new(
        (min_world_x + max_world_x) * 0.5,
        (min_world_y + max_world_y) * 0.5,
    );
    let size = Vec2::new(max_world_x - min_world_x, max_world_y - min_world_y);
    gizmos.rect_2d(
        Isometry2d::from_translation(center),
        size,
        Color::srgba(0.30, 0.85, 1.00, 0.95),
    );
}

/// `PickRect` mode: drag-LMB writes a `TileRectangle` to
/// `EditorPickRectResult` on release, then restores the tool that was active
/// before the user entered pick mode (stashed in `EditorState.tool_before_pick`).
#[allow(clippy::too_many_arguments)]
pub fn handle_editor_pick_rect_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut editor_state: ResMut<EditorState>,
    mut pick_result: ResMut<EditorPickRectResult>,
    panel_roots: crate::editor::ui::EditorPanelRoots,
    mut state: Local<SelectDragState>,
) {
    let EditorTool::PickRect { target } = editor_state.current_tool else {
        state.anchor = None;
        return;
    };
    if !mouse.pressed(MouseButton::Left) {
        // Mouse just released after a drag — finalise the pick and restore tool.
        if let Some(anchor) = state.anchor {
            if let Some(sel) = editor_state
                .selection
                .filter(|s| s.space_id == editor_context.space_id)
            {
                pick_result.pending = Some(PickedRect {
                    target,
                    rect: TileRectangle {
                        min_x: sel.min.x,
                        min_y: sel.min.y,
                        max_x: sel.max.x,
                        max_y: sel.max.y,
                    },
                });
            } else {
                // No drag occurred — treat the click as cancelled.
                let _ = anchor;
            }
            // Always restore the previous tool whether or not a rect landed,
            // so the user can press Esc-equivalent semantics by clicking and
            // not dragging.
            if let Some(prev) = editor_state.tool_before_pick.take() {
                editor_state.current_tool = prev;
            } else {
                editor_state.current_tool = EditorTool::Brush;
            }
            // Clear the marquee — it was driven by this drag and shouldn't
            // persist as a regular Select-tool selection.
            editor_state.selection = None;
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

/// Legacy single-letter switch to Select tool (default `M`).
pub fn handle_editor_select_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_keys: Res<crate::ui::settings::EditorKeybindings>,
    modal_state: Res<crate::editor::resources::ModalState>,
    mut editor_state: ResMut<EditorState>,
) {
    if modal_state.active.is_some() || editor_state.palette_filter_focused {
        return;
    }
    if editor_keys.just_pressed(crate::ui::settings::EditorAction::ToolSelectLegacy, &keyboard) {
        editor_state.current_tool = EditorTool::Select;
    }
}

// ── Local helpers ────────────────────────────────────────────────────────────

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
