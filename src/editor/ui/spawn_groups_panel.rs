//! Toggleable side panel that lists the current map's spawn groups and lets
//! the user create / edit / delete / duplicate them.
//!
//! Visibility is driven by `EditorState::spawn_groups_panel_visible`. The
//! panel mirrors `templates_panel.rs`'s scrollable column. Each row shows the
//! group's id, template, and limits; per-row buttons open the
//! `SpawnGroupEdit` modal (Edit), remove the group (Delete), or clone it
//! (Duplicate). A bottom-row "Add" button opens the modal in create mode.

use bevy::prelude::*;

use crate::editor::resources::{
    EditorContext, EditorSpawnGroupBuffer, EditorState, ModalKind, ModalState, SpawnGroupDraft,
    UndoOp, UndoStack,
};
use crate::world::map_layout::{MapBehavior, SpawnGroupDef, TileRectangle};

/// Marker for the panel root node — used by `EditorPanelRoots::cursor_over`
/// (so panel clicks don't fall through to the world) and by visibility-sync.
#[derive(Component)]
pub struct EditorSpawnGroupsRoot;

#[derive(Component)]
pub struct EditorSpawnGroupsContent;

#[derive(Component, Clone, Copy)]
pub struct EditorSpawnGroupRow {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct EditorSpawnGroupEditButton {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct EditorSpawnGroupDeleteButton {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct EditorSpawnGroupDuplicateButton {
    pub index: usize,
}

#[derive(Component)]
pub struct EditorSpawnGroupAddButton;

const PANEL_WIDTH_PX: f32 = 240.0;

pub fn spawn_spawn_groups_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorSpawnGroupsRoot,
            Node {
                width: Val::Px(PANEL_WIDTH_PX),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
            BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
        ))
        .with_children(|panel| {
            // Header
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Spawn Groups"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    h.spawn((
                        Button,
                        EditorSpawnGroupAddButton,
                        Node {
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.18, 0.12, 0.06, 0.95)),
                        BorderColor::all(Color::srgb(0.55, 0.40, 0.22)),
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("+ Add"),
                            TextFont {
                                font_size: 12.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.96, 0.86, 0.66)),
                        ));
                    });
                });

            // Rows
            panel.spawn((
                EditorSpawnGroupsContent,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                bevy::ui::ScrollPosition::default(),
            ));
        });
}

/// Toggle panel display via `EditorState::spawn_groups_panel_visible`.
pub fn sync_spawn_groups_panel_visibility(
    editor_state: Res<EditorState>,
    mut roots: Query<&mut Node, With<EditorSpawnGroupsRoot>>,
) {
    if !editor_state.is_changed() {
        return;
    }
    let target = if editor_state.spawn_groups_panel_visible {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut roots {
        if node.display != target {
            node.display = target;
        }
    }
}

/// Rebuild rows whenever the buffer changes.
pub fn sync_spawn_groups_panel(
    editor_state: Res<EditorState>,
    buffer: Res<EditorSpawnGroupBuffer>,
    content: Query<Entity, With<EditorSpawnGroupsContent>>,
    rows: Query<Entity, With<EditorSpawnGroupRow>>,
    mut commands: Commands,
) {
    if !editor_state.spawn_groups_panel_visible {
        return;
    }
    if !buffer.is_changed() && !editor_state.is_changed() {
        return;
    }

    for row in &rows {
        commands.entity(row).despawn();
    }
    let Ok(content_entity) = content.single() else {
        return;
    };

    if buffer.groups.is_empty() {
        commands.entity(content_entity).with_children(|c| {
            c.spawn((
                EditorSpawnGroupRow { index: usize::MAX },
                Text::new("(no spawn groups)"),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.50, 0.46, 0.42)),
                Node {
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
            ));
        });
        return;
    }

    let groups: Vec<(usize, SpawnGroupDef)> = buffer
        .groups
        .iter()
        .enumerate()
        .map(|(i, g)| (i, g.clone()))
        .collect();
    let selected = buffer.selected;

    commands.entity(content_entity).with_children(|c| {
        for (index, group) in groups {
            let is_selected = selected == Some(index);
            let bg = if is_selected {
                Color::srgba(0.20, 0.14, 0.08, 0.95)
            } else {
                Color::srgba(0.10, 0.07, 0.06, 0.80)
            };
            let border = if is_selected {
                Color::srgb(0.85, 0.65, 0.30)
            } else {
                Color::srgb(0.20, 0.15, 0.10)
            };
            c.spawn((
                Button,
                EditorSpawnGroupRow { index },
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(bg),
                BorderColor::all(border),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(group.id.clone()),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.86, 0.66)),
                ));
                row.spawn((
                    Text::new(format!(
                        "{}  ×{}  every {:.1}s",
                        group.template, group.max_count, group.respawn_mean_seconds
                    )),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.78, 0.74, 0.66)),
                ));
                row.spawn((
                    Text::new(behavior_summary(&group.behavior)),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.62, 0.58, 0.50)),
                ));
                // Action button row.
                row.spawn((Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(4.0),
                    margin: UiRect::top(Val::Px(2.0)),
                    ..default()
                },))
                    .with_children(|actions| {
                        spawn_action_button(actions, "Edit", EditorSpawnGroupEditButton { index });
                        spawn_action_button(
                            actions,
                            "Dup",
                            EditorSpawnGroupDuplicateButton { index },
                        );
                        spawn_action_button(actions, "Del", EditorSpawnGroupDeleteButton { index });
                    });
            });
        }
    });
}

fn behavior_summary(behavior: &MapBehavior) -> String {
    match behavior {
        MapBehavior::Roam { bounds } => format!(
            "Roam  ({},{})-({},{})",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
        ),
        MapBehavior::RoamAndChase { bounds } => format!(
            "Chase  ({},{})-({},{})",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
        ),
    }
}

fn spawn_action_button<M: Component>(parent: &mut ChildSpawnerCommands, label: &str, marker: M) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.14, 0.10, 0.08, 0.95)),
            BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.86, 0.74)),
            ));
        });
}

/// Click handlers: row select, action buttons, add button.
#[allow(clippy::too_many_arguments)]
pub fn handle_spawn_groups_panel_clicks(
    rows: Query<(&EditorSpawnGroupRow, &Interaction), (Changed<Interaction>, With<Button>)>,
    edit_btns: Query<
        (&EditorSpawnGroupEditButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    delete_btns: Query<
        (&EditorSpawnGroupDeleteButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    duplicate_btns: Query<
        (&EditorSpawnGroupDuplicateButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    add_btn: Query<&Interaction, (Changed<Interaction>, With<EditorSpawnGroupAddButton>)>,
    mut buffer: ResMut<EditorSpawnGroupBuffer>,
    mut undo_stack: ResMut<UndoStack>,
    mut editor_state: ResMut<EditorState>,
    mut modal_state: ResMut<ModalState>,
    editor_context: Res<EditorContext>,
) {
    // Plain row selection (no button overlap).
    for (row, interaction) in &rows {
        if *interaction == Interaction::Pressed && row.index != usize::MAX {
            buffer.selected = Some(row.index);
        }
    }

    for (btn, interaction) in &edit_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(group) = buffer.groups.get(btn.index).cloned() else {
            continue;
        };
        buffer.selected = Some(btn.index);
        open_spawn_group_modal(&mut modal_state, Some(btn.index), Some(&group));
    }

    for (btn, interaction) in &duplicate_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(group) = buffer.groups.get(btn.index).cloned() else {
            continue;
        };
        let mut clone = group.clone();
        clone.id = unique_clone_id(&clone.id, &buffer.groups);
        let new_index = buffer.groups.len();
        buffer.groups.push(clone.clone());
        undo_stack.push_undo(UndoOp::RemoveSpawnGroup { index: new_index });
        buffer.selected = Some(new_index);
        editor_state.dirty = true;
    }

    for (btn, interaction) in &delete_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if btn.index >= buffer.groups.len() {
            continue;
        }
        let removed = buffer.groups.remove(btn.index);
        undo_stack.push_undo(UndoOp::AddSpawnGroup {
            index: btn.index,
            group: removed,
        });
        if buffer.selected == Some(btn.index) {
            buffer.selected = None;
        } else if let Some(s) = buffer.selected {
            if s > btn.index {
                buffer.selected = Some(s - 1);
            }
        }
        editor_state.dirty = true;
    }

    for interaction in &add_btn {
        if *interaction != Interaction::Pressed || modal_state.active.is_some() {
            continue;
        }
        // Default a new group's bounds to the whole map so the user can shrink
        // rather than start at zero.
        let max_x = (editor_context.map_width - 1).to_string();
        let max_y = (editor_context.map_height - 1).to_string();
        let draft = SpawnGroupDraft {
            area_min_x: "0".into(),
            area_min_y: "0".into(),
            area_max_x: max_x.clone(),
            area_max_y: max_y.clone(),
            bhv_min_x: "0".into(),
            bhv_min_y: "0".into(),
            bhv_max_x: max_x,
            bhv_max_y: max_y,
            ..SpawnGroupDraft::default()
        };
        modal_state.active = Some(ModalKind::SpawnGroupEdit {
            editing_index: None,
        });
        modal_state.error_message = None;
        modal_state.confirm_triggered = false;
        modal_state.confirmed = None;
        modal_state.spawn_group_draft = Some(draft);
    }
}

/// Helper for the panel and toolbar to open the modal.
pub fn open_spawn_group_modal(
    modal_state: &mut ModalState,
    editing_index: Option<usize>,
    existing: Option<&SpawnGroupDef>,
) {
    let draft = match (editing_index, existing) {
        (Some(idx), Some(group)) => SpawnGroupDraft::from_existing(idx, group),
        _ => SpawnGroupDraft {
            editing_index,
            ..SpawnGroupDraft::default()
        },
    };
    modal_state.active = Some(ModalKind::SpawnGroupEdit { editing_index });
    modal_state.error_message = None;
    modal_state.confirm_triggered = false;
    modal_state.confirmed = None;
    modal_state.spawn_group_draft = Some(draft);
}

fn unique_clone_id(base: &str, groups: &[SpawnGroupDef]) -> String {
    let mut candidate = format!("{base}_copy");
    let mut suffix = 2;
    while groups.iter().any(|g| g.id == candidate) {
        candidate = format!("{base}_copy{suffix}");
        suffix += 1;
    }
    candidate
}

/// Draws a translucent rectangle / tile-dot overlay for the currently-
/// selected spawn group's spawn area on top of the map.
pub fn render_spawn_group_overlay(
    mut gizmos: Gizmos,
    world_config: Res<crate::world::WorldConfig>,
    editor_camera: Res<crate::editor::resources::EditorCamera>,
    editor_context: Res<EditorContext>,
    editor_state: Res<EditorState>,
    buffer: Res<EditorSpawnGroupBuffer>,
) {
    if !editor_state.spawn_groups_panel_visible {
        return;
    }
    let Some(idx) = buffer.selected else { return };
    let Some(group) = buffer.groups.get(idx) else {
        return;
    };
    let effective = world_config.tile_size * editor_camera.zoom_level;
    if effective <= f32::EPSILON {
        return;
    }
    let area_color = Color::srgba(1.00, 0.55, 0.20, 0.85);
    let bhv_color = Color::srgba(0.40, 0.85, 1.00, 0.65);

    if let Some(rect) = group.area.bounds {
        draw_tile_rect(&mut gizmos, rect, &editor_camera, effective, area_color);
    }
    if let Some(tiles) = &group.area.tiles {
        for tile in tiles {
            let center = Vec2::new(
                (tile.x as f32 - editor_camera.center.x) * effective,
                (tile.y as f32 - editor_camera.center.y) * effective,
            );
            gizmos.rect_2d(
                Isometry2d::from_translation(center),
                Vec2::splat(effective * 0.7),
                area_color,
            );
        }
    }

    let bhv_rect = match &group.behavior {
        MapBehavior::Roam { bounds, .. } => *bounds,
        MapBehavior::RoamAndChase { bounds, .. } => *bounds,
    };
    draw_tile_rect(&mut gizmos, bhv_rect, &editor_camera, effective, bhv_color);

    let _ = editor_context; // reserved for future per-space gating
}

fn draw_tile_rect(
    gizmos: &mut Gizmos,
    rect: TileRectangle,
    camera: &crate::editor::resources::EditorCamera,
    effective: f32,
    color: Color,
) {
    let min_world_x = (rect.min_x as f32 - 0.5 - camera.center.x) * effective;
    let max_world_x = (rect.max_x as f32 + 0.5 - camera.center.x) * effective;
    let min_world_y = (rect.min_y as f32 - 0.5 - camera.center.y) * effective;
    let max_world_y = (rect.max_y as f32 + 0.5 - camera.center.y) * effective;
    let center = Vec2::new(
        (min_world_x + max_world_x) * 0.5,
        (min_world_y + max_world_y) * 0.5,
    );
    let size = Vec2::new(max_world_x - min_world_x, max_world_y - min_world_y);
    gizmos.rect_2d(Isometry2d::from_translation(center), size, color);
}
