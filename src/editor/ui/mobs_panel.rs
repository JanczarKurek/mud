//! Toggleable side panel listing every NPC template (any object definition
//! whose `npc_behavior:` block is present). Each row has two action buttons:
//!
//! - **Place** — arms single-mob placement (palette-style: sets
//!   `EditorState::selected_type_id` + `EditorTool::Brush`). The next click
//!   on a map tile drops one NPC there with no spawn group.
//! - **+ Group** — stashes the template id on
//!   `EditorSpawnGroupBuffer.pending_new_spawn_group_template` and enters
//!   `EditorTool::PickRect { target: NewSpawnGroup }`. The user drags a
//!   rectangle on the map; on release, `apply_pick_rect_for_new_spawn_group`
//!   materializes the spawn group and opens the edit modal for fine-tuning.

use bevy::prelude::*;

use crate::editor::resources::{
    EditorPickRectResult, EditorSpawnGroupBuffer, EditorState, EditorTool, ModalState,
    PickRectTarget, UndoOp, UndoStack,
};
use crate::editor::ui::spawn_groups_panel::open_spawn_group_modal;
use crate::world::map_layout::{MapBehavior, SpawnArea, SpawnGroupDef};
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Component)]
pub struct EditorMobsRoot;

#[derive(Component)]
pub struct EditorMobsContent;

#[derive(Component, Clone)]
pub struct EditorMobPlaceButton {
    pub template_id: String,
}

#[derive(Component, Clone)]
pub struct EditorMobGroupButton {
    pub template_id: String,
}

const PANEL_WIDTH_PX: f32 = 220.0;

pub fn spawn_mobs_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorMobsRoot,
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
                        Text::new("Mobs"),
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
                });

            panel.spawn((
                EditorMobsContent,
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

pub fn sync_mobs_panel_visibility(
    editor_state: Res<EditorState>,
    mut roots: Query<&mut Node, With<EditorMobsRoot>>,
) {
    if !editor_state.is_changed() {
        return;
    }
    let target = if editor_state.mobs_panel_visible {
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

/// Build / rebuild the mob row list. Only runs when the panel is visible and
/// only repopulates if the row list is empty (the catalogue is static for the
/// editor session).
pub fn sync_mobs_panel(
    editor_state: Res<EditorState>,
    definitions: Res<OverworldObjectDefinitions>,
    content: Query<Entity, With<EditorMobsContent>>,
    place_btns: Query<Entity, With<EditorMobPlaceButton>>,
    mut commands: Commands,
) {
    if !editor_state.mobs_panel_visible {
        return;
    }
    if !place_btns.is_empty() {
        return;
    }
    let Ok(content_entity) = content.single() else {
        return;
    };

    let mut entries: Vec<(String, String, Option<u32>, f32, i32)> = Vec::new();
    for id in definitions.ids() {
        let Some(def) = definitions.get(id) else {
            continue;
        };
        let Some(behavior) = def.npc_behavior.as_ref() else {
            continue;
        };
        entries.push((
            id.to_owned(),
            def.name.clone(),
            def.level,
            behavior.step_interval_seconds,
            behavior.detect_distance_tiles,
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    if entries.is_empty() {
        commands.entity(content_entity).with_children(|c| {
            c.spawn((
                Text::new("(no NPC templates found)"),
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

    commands.entity(content_entity).with_children(|c| {
        for (id, name, level, step, detect) in entries {
            c.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.07, 0.06, 0.80)),
                BorderColor::all(Color::srgb(0.20, 0.15, 0.10)),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(format!("{}  ({})", name, id)),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.86, 0.66)),
                ));
                let lvl = level.map(|l| format!("lvl {l}")).unwrap_or_default();
                row.spawn((
                    Text::new(format!("{}  step {:.2}s  detect {}t", lvl, step, detect)),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.78, 0.74, 0.66)),
                ));
                row.spawn((Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(4.0),
                    margin: UiRect::top(Val::Px(3.0)),
                    ..default()
                },))
                    .with_children(|actions| {
                        action_button(
                            actions,
                            "Place",
                            EditorMobPlaceButton {
                                template_id: id.clone(),
                            },
                        );
                        action_button(
                            actions,
                            "+ Group",
                            EditorMobGroupButton {
                                template_id: id.clone(),
                            },
                        );
                    });
            });
        }
    });
}

fn action_button<M: Component>(parent: &mut ChildSpawnerCommands, label: &str, marker: M) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.14, 0.10, 0.08, 0.95)),
            BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.86, 0.74)),
            ));
        });
}

/// "Place" — arms single-mob placement via the same `selected_type_id` +
/// `Brush` tool flow the palette uses. The next click on a map tile drops a
/// single NPC there with no spawn group.
pub fn handle_mobs_panel_place_clicks(
    btns: Query<(&EditorMobPlaceButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        editor_state.selected_type_id = Some(btn.template_id.clone());
        editor_state.selected_object_id = None;
        editor_state.current_tool = EditorTool::Brush;
    }
}

/// "+ Group" — stashes the template id and enters PickRect mode. The user
/// drags a rect on the map; on release, `apply_pick_rect_for_new_spawn_group`
/// builds the spawn group and opens the edit modal.
pub fn handle_mobs_panel_group_clicks(
    btns: Query<(&EditorMobGroupButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut editor_state: ResMut<EditorState>,
    mut buffer: ResMut<EditorSpawnGroupBuffer>,
) {
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if !matches!(editor_state.current_tool, EditorTool::PickRect { .. }) {
            editor_state.tool_before_pick = Some(editor_state.current_tool);
        }
        editor_state.current_tool = EditorTool::PickRect {
            target: PickRectTarget::NewSpawnGroup,
        };
        buffer.pending_new_spawn_group_template = Some(btn.template_id.clone());
    }
}

/// Consumes a `PickRectTarget::NewSpawnGroup` result: builds a `SpawnGroupDef`
/// with the picked rect for both area bounds and roam-and-chase behavior
/// bounds, pushes it onto the buffer, and opens the spawn-group edit modal so
/// the user can tweak count / respawn / hostility.
pub fn apply_pick_rect_for_new_spawn_group(
    mut pick_result: ResMut<EditorPickRectResult>,
    mut buffer: ResMut<EditorSpawnGroupBuffer>,
    mut editor_state: ResMut<EditorState>,
    mut modal_state: ResMut<ModalState>,
    mut undo_stack: ResMut<UndoStack>,
) {
    let Some(picked) = pick_result.pending else {
        return;
    };
    if !matches!(picked.target, PickRectTarget::NewSpawnGroup) {
        return;
    }
    pick_result.pending = None;

    let Some(template_id) = buffer.pending_new_spawn_group_template.take() else {
        // Picker fired but the template was never set (e.g. an Esc-cancel
        // path that didn't clear the tool). Nothing to do.
        return;
    };

    let rect = picked.rect;
    let id = unique_group_id(&template_id, &buffer.groups);
    let new_group = SpawnGroupDef {
        id,
        template: template_id,
        max_count: 3,
        respawn_mean_seconds: 30.0,
        area: SpawnArea {
            bounds: Some(rect),
            tiles: None,
        },
        behavior: MapBehavior::RoamAndChase { bounds: rect },
    };

    let new_index = buffer.groups.len();
    buffer.groups.push(new_group.clone());
    undo_stack.push_undo(UndoOp::RemoveSpawnGroup { index: new_index });
    buffer.selected = Some(new_index);
    editor_state.spawn_groups_panel_visible = true;
    editor_state.dirty = true;

    open_spawn_group_modal(&mut modal_state, Some(new_index), Some(&new_group));
}

fn unique_group_id(template_id: &str, groups: &[SpawnGroupDef]) -> String {
    let mut candidate = template_id.to_owned();
    let mut suffix = 2;
    while groups.iter().any(|g| g.id == candidate) {
        candidate = format!("{template_id}_{suffix}");
        suffix += 1;
    }
    candidate
}
