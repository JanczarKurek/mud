//! Bottom-of-screen status bar: cursor tile coords, current tool, brush
//! radius, fill mode, current editing floor, selection size, hovered
//! object type, and unsaved-dirty marker.

#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::resources::{EditorCamera, EditorContext, EditorState, EditorTool, FillMode};
use crate::editor::systems::cursor_to_tile_pub;
use crate::editor::ui::EditorPanelRoots;
use crate::player::components::Player;
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::WorldConfig;

#[derive(Component)]
pub struct EditorStatusBarRoot;

#[derive(Component)]
pub struct EditorStatusBarText;

/// Spawn the status bar at the bottom of the HUD root. Called once from
/// `spawn_editor_hud`.
pub fn spawn_status_bar(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorStatusBarRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(22.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(14.0),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
        ))
        .with_children(|bar| {
            bar.spawn((
                EditorStatusBarText,
                Text::new(""),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.72, 0.60)),
            ));
        });
}

/// Update status bar text each frame. Cheap enough to recompute every
/// frame — single ~120-byte string write.
#[allow(clippy::too_many_arguments)]
pub fn sync_status_bar(
    editor_state: Res<EditorState>,
    editor_context: Res<EditorContext>,
    editor_camera: Res<EditorCamera>,
    world_config: Res<WorldConfig>,
    windows: Query<&Window, With<PrimaryWindow>>,
    panel_roots: EditorPanelRoots,
    objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    object_registry: Res<crate::world::object_registry::ObjectRegistry>,
    mut texts: Query<&mut Text, With<EditorStatusBarText>>,
) {
    let Ok(mut text) = texts.single_mut() else {
        return;
    };
    let mut parts: Vec<String> = Vec::new();

    // Cursor coords + hovered object.
    if let Ok(window) = windows.single() {
        if let Some(cursor) = window.cursor_position() {
            if !panel_roots.cursor_over(cursor, window.scale_factor()) {
                let tile = cursor_to_tile_pub(cursor, window, &world_config, &editor_camera);
                parts.push(format!(
                    "({}, {}, floor {})",
                    tile.x, tile.y, editor_state.current_editing_floor
                ));
                if tile.x >= 0
                    && tile.y >= 0
                    && tile.x < editor_context.map_width
                    && tile.y < editor_context.map_height
                {
                    let hovered = objects.iter().find(|(_, r, p)| {
                        r.space_id == editor_context.space_id
                            && p.x == tile.x
                            && p.y == tile.y
                            && editor_state.tile_on_active_floor(p.z)
                    });
                    if let Some((obj, _, _)) = hovered {
                        let label = object_registry
                            .type_id(obj.object_id)
                            .map(str::to_owned)
                            .unwrap_or_else(|| obj.definition_id.clone());
                        parts.push(format!("hover: {}", label));
                    }
                }
            }
        }
    }

    // Tool + brush radius + fill mode.
    parts.push(format!("tool: {}", tool_label(editor_state.current_tool)));
    if matches!(
        editor_state.current_tool,
        EditorTool::Brush | EditorTool::FloorBrush
    ) {
        let r = editor_state.effective_brush_radius();
        if r > 1 {
            parts.push(format!("brush {}x{}", r, r));
        }
    }
    if editor_state.fill_mode != FillMode::Single {
        parts.push(format!("fill: {}", fill_label(editor_state.fill_mode)));
    }

    // Selection size.
    if let Some(sel) = editor_state.selection {
        parts.push(format!(
            "sel: {}x{} ({})",
            sel.width(),
            sel.height(),
            sel.width() * sel.height(),
        ));
    }

    // Map name + dirty marker.
    parts.push(format!("map: {}", editor_context.authored_id));
    if editor_state.dirty {
        parts.push("[unsaved]".to_owned());
    }

    text.0 = parts.join("   ");
}

fn tool_label(tool: EditorTool) -> &'static str {
    match tool {
        EditorTool::Brush => "Brush",
        EditorTool::Portal => "Portal",
        EditorTool::FloorBrush => "Floor",
        EditorTool::Select => "Select",
        EditorTool::PickRect { .. } => "PickRect",
        EditorTool::BuildingDraw => "Building",
    }
}

fn fill_label(mode: FillMode) -> &'static str {
    match mode {
        FillMode::Single => "single",
        FillMode::Rect => "rect (Shift+drag)",
        FillMode::Flood => "flood (G)",
    }
}
