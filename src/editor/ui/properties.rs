use bevy::prelude::*;

use crate::editor::dialog_index::EditorDialogIndex;
use crate::editor::resources::{
    EditingField, EditorPickRectResult, EditorPropertyEditBuffer, EditorState, EditorTool,
    PickRectTarget, UndoOp, UndoStack,
};
use crate::world::components::OverworldObject;
use crate::world::map_layout::{MapBehavior, TileRectangle};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;

/// Root panel for the properties sidebar.
#[derive(Component)]
pub struct EditorPropertiesRoot;

/// Marks the properties content area that gets rebuilt.
#[derive(Component)]
pub struct EditorPropertiesContent;

/// A row in the property list; carries its index.
#[derive(Component, Clone, Copy)]
pub struct EditorPropertyRow {
    pub index: usize,
}

/// Marks the key text of a property row.
#[derive(Component, Clone, Copy)]
pub struct EditorPropertyKeyText {
    pub index: usize,
}

/// Marks the value text of a property row.
#[derive(Component, Clone, Copy)]
pub struct EditorPropertyValueText {
    pub index: usize,
}

/// Add-property key input placeholder.
#[derive(Component)]
pub struct EditorAddKeyInput;

/// Add-property value input placeholder.
#[derive(Component)]
pub struct EditorAddValueInput;

/// Button to confirm adding a new property.
#[derive(Component)]
pub struct EditorPropertyAddButton;

/// Header text showing the object type.
#[derive(Component)]
pub struct EditorPropertiesHeader;

#[derive(Component, Clone, Copy)]
pub struct BehaviorSetButton {
    pub kind: BehaviorButtonKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BehaviorButtonKind {
    Roam,
    RoamAndChase,
    Clear,
}

#[derive(Component)]
pub struct BehaviorPickBoundsButton;

#[derive(Component, Clone, Copy)]
pub struct BehaviorNudgeButton {
    pub field: BehaviorNudgeField,
    pub delta: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BehaviorNudgeField {
    StepIntervalTenths,
    DetectDistance,
    DisengageDistance,
}

#[derive(Component, Clone)]
pub struct DialogSelectButton {
    pub dialog_id: Option<String>,
}

/// Spawn the right-sidebar properties panel (initially empty).
pub fn spawn_properties_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorPropertiesRoot,
            Node {
                width: Val::Px(220.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
            BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
            Visibility::Hidden,
        ))
        .with_children(|panel| {
            // Header
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Properties"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                    ));
                    h.spawn((
                        EditorPropertiesHeader,
                        Text::new(""),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.70, 0.66, 0.60)),
                    ));
                });

            // Content area (rebuilt when selection changes)
            panel.spawn((
                EditorPropertiesContent,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(6.0)),
                    row_gap: Val::Px(4.0),
                    overflow: Overflow::clip_y(),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

/// Sync the properties panel: show/hide it and rebuild property rows when
/// the selection or buffer changes.
#[allow(clippy::too_many_arguments)]
pub fn sync_properties_panel(
    editor_state: Res<EditorState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    object_registry: Res<ObjectRegistry>,
    object_definitions: Res<OverworldObjectDefinitions>,
    dialog_index: Res<EditorDialogIndex>,
    object_query: Query<&OverworldObject>,
    mut root_query: Query<&mut Visibility, With<EditorPropertiesRoot>>,
    mut header_query: Query<&mut Text, With<EditorPropertiesHeader>>,
    content_query: Query<Entity, With<EditorPropertiesContent>>,
    mut commands: Commands,
) {
    let Ok(mut visibility) = root_query.single_mut() else {
        return;
    };

    let Some(selected_id) = editor_state.selected_object_id else {
        *visibility = Visibility::Hidden;
        return;
    };

    *visibility = Visibility::Visible;

    // Update header with type info.
    let type_label = object_query
        .iter()
        .find(|o| o.object_id == selected_id)
        .map(|o| format!("{} (id: {})", o.definition_id, o.object_id))
        .unwrap_or_else(|| format!("id: {selected_id}"));

    if let Ok(mut text) = header_query.single_mut() {
        text.0 = type_label;
    }

    // Only rebuild content when buffer actually changed.
    if !prop_buffer.is_changed()
        && !editor_state.is_changed()
        && !object_registry.is_changed()
        && !dialog_index.is_changed()
    {
        return;
    }

    let Ok(content_entity) = content_query.single() else {
        return;
    };

    // Despawn old property rows.
    commands
        .entity(content_entity)
        .despawn_related::<Children>();

    // Get current properties to display.
    let entries = if prop_buffer.object_id == Some(selected_id) {
        prop_buffer.entries.clone()
    } else {
        object_registry
            .properties(selected_id)
            .map(|p| {
                let mut v: Vec<(String, String)> =
                    p.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                v
            })
            .unwrap_or_default()
    };

    // Rebuild rows.
    commands.entity(content_entity).with_children(|content| {
        if entries.is_empty() {
            content.spawn((
                Text::new("(no properties)"),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.55, 0.52, 0.48)),
            ));
        }

        for (index, (key, value)) in entries.iter().enumerate() {
            let is_editing_value = prop_buffer.editing_index == Some(index)
                && prop_buffer.editing_field == EditingField::Value;
            let is_editing_key = prop_buffer.editing_index == Some(index)
                && prop_buffer.editing_field == EditingField::Key;

            let displayed_key = if is_editing_key {
                format!("[{}]", prop_buffer.edit_text)
            } else {
                key.clone()
            };
            let displayed_value = if is_editing_value {
                format!("[{}]", prop_buffer.edit_text)
            } else {
                value.clone()
            };

            content
                .spawn((
                    Button,
                    EditorPropertyRow { index },
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(4.0),
                        padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(if is_editing_value || is_editing_key {
                        Color::srgba(0.20, 0.15, 0.08, 0.90)
                    } else {
                        Color::srgba(0.10, 0.08, 0.06, 0.70)
                    }),
                    BorderColor::all(if is_editing_value || is_editing_key {
                        Color::srgb(0.90, 0.72, 0.40)
                    } else {
                        Color::srgb(0.22, 0.16, 0.12)
                    }),
                ))
                .with_children(|row| {
                    row.spawn((
                        EditorPropertyKeyText { index },
                        Text::new(displayed_key),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.80, 0.76, 0.68)),
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    row.spawn((
                        Text::new(":"),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.55, 0.50, 0.45)),
                    ));
                    row.spawn((
                        EditorPropertyValueText { index },
                        Text::new(displayed_value),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.92, 0.80)),
                        Node {
                            overflow: Overflow::clip_x(),
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                });
        }

        // "Add property" row.
        content
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    margin: UiRect::top(Val::Px(6.0)),
                    border: UiRect::top(Val::Px(1.0)),
                    padding: UiRect::top(Val::Px(4.0)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.25, 0.18, 0.12)),
            ))
            .with_children(|footer| {
                footer
                    .spawn((
                        Button,
                        EditorPropertyAddButton,
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.12, 0.09, 0.06, 0.80)),
                        BorderColor::all(Color::srgb(0.30, 0.22, 0.14)),
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("+ Add property"),
                            TextFont {
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.85, 0.80, 0.70)),
                        ));
                    });
            });

        // ── NPC Behavior + Dialog ───────────────────────────────────────────
        let definition_id = object_query
            .iter()
            .find(|o| o.object_id == selected_id)
            .map(|o| o.definition_id.clone());
        let is_npc_like = definition_id
            .as_deref()
            .map(|id| object_definitions.extends(id, "npc"))
            .unwrap_or(false);
        if is_npc_like {
            let behavior = object_registry.behavior(selected_id).cloned();
            spawn_behavior_section(content, behavior.as_ref());
            spawn_dialog_section(
                content,
                object_registry
                    .properties(selected_id)
                    .and_then(|p| p.get("dialog_id"))
                    .cloned(),
                &dialog_index.names,
            );
        }
    });
}

fn spawn_behavior_section(parent: &mut ChildSpawnerCommands, behavior: Option<&MapBehavior>) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                margin: UiRect::top(Val::Px(8.0)),
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(4.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.25, 0.18, 0.12)),
        ))
        .with_children(|sec| {
            sec.spawn((
                Text::new("Behavior"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.84, 0.62)),
            ));
            sec.spawn((
                Text::new(behavior_summary(behavior)),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.74, 0.66)),
            ));
            sec.spawn((Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                ..default()
            },))
                .with_children(|row| {
                    behavior_button(row, "Roam", BehaviorButtonKind::Roam);
                    behavior_button(row, "Chase", BehaviorButtonKind::RoamAndChase);
                    behavior_button(row, "Clear", BehaviorButtonKind::Clear);
                });
            sec.spawn((
                Button,
                BehaviorPickBoundsButton,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.16, 0.10, 0.06, 0.95)),
                BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new("Pick bounds on map"),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.86, 0.74)),
                ));
            });

            // Numeric nudge rows (visible when a behavior is set).
            if let Some(b) = behavior {
                let (step, detect, disengage) = match b {
                    MapBehavior::Roam {
                        step_interval_seconds,
                        ..
                    } => (*step_interval_seconds, None, None),
                    MapBehavior::RoamAndChase {
                        step_interval_seconds,
                        detect_distance_tiles,
                        disengage_distance_tiles,
                        ..
                    } => (
                        *step_interval_seconds,
                        Some(*detect_distance_tiles),
                        Some(*disengage_distance_tiles),
                    ),
                };
                nudge_row(
                    sec,
                    &format!("step {step:.2}s"),
                    BehaviorNudgeField::StepIntervalTenths,
                );
                if let Some(d) = detect {
                    nudge_row(sec, &format!("detect {d}t"), BehaviorNudgeField::DetectDistance);
                }
                if let Some(d) = disengage {
                    nudge_row(sec, &format!("dis. {d}t"), BehaviorNudgeField::DisengageDistance);
                }
            }
        });
}

fn behavior_button(parent: &mut ChildSpawnerCommands, label: &str, kind: BehaviorButtonKind) {
    parent
        .spawn((
            Button,
            BehaviorSetButton { kind },
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

fn nudge_row(parent: &mut ChildSpawnerCommands, label: &str, field: BehaviorNudgeField) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(4.0),
            align_items: AlignItems::Center,
            ..default()
        },))
        .with_children(|row| {
            nudge_button(row, "−", BehaviorNudgeButton { field, delta: -1 });
            row.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.74, 0.66)),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            nudge_button(row, "+", BehaviorNudgeButton { field, delta: 1 });
        });
}

fn nudge_button(parent: &mut ChildSpawnerCommands, label: &str, marker: BehaviorNudgeButton) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
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
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.86, 0.74)),
            ));
        });
}

fn behavior_summary(behavior: Option<&MapBehavior>) -> String {
    match behavior {
        None => "(no behavior)".to_owned(),
        Some(MapBehavior::Roam {
            step_interval_seconds,
            bounds,
        }) => format!(
            "Roam {:.2}s ({},{})-({},{})",
            step_interval_seconds, bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
        ),
        Some(MapBehavior::RoamAndChase {
            step_interval_seconds,
            bounds,
            detect_distance_tiles,
            disengage_distance_tiles,
        }) => format!(
            "Chase d={} dis={} {:.2}s ({},{})-({},{})",
            detect_distance_tiles,
            disengage_distance_tiles,
            step_interval_seconds,
            bounds.min_x,
            bounds.min_y,
            bounds.max_x,
            bounds.max_y
        ),
    }
}

fn spawn_dialog_section(
    parent: &mut ChildSpawnerCommands,
    current: Option<String>,
    available: &[String],
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                margin: UiRect::top(Val::Px(8.0)),
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(4.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.25, 0.18, 0.12)),
        ))
        .with_children(|sec| {
            sec.spawn((
                Text::new("Dialog"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.84, 0.62)),
            ));
            sec.spawn((
                Text::new(format!(
                    "current: {}",
                    current.as_deref().unwrap_or("(none)")
                )),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.74, 0.66)),
            ));
            // "(none)" first.
            dialog_button(sec, "(none)", None, current.is_none());
            for name in available {
                let is_current = current.as_deref() == Some(name.as_str());
                dialog_button(sec, name, Some(name.clone()), is_current);
            }
            if available.is_empty() {
                sec.spawn((
                    Text::new("(no .yarn files in assets/dialogs/)"),
                    TextFont {
                        font_size: 9.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.55, 0.50, 0.45)),
                ));
            }
        });
}

fn dialog_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    dialog_id: Option<String>,
    selected: bool,
) {
    let bg = if selected {
        Color::srgb(0.28, 0.16, 0.08)
    } else {
        Color::srgba(0.10, 0.07, 0.06, 0.80)
    };
    let border = if selected {
        Color::srgb(0.85, 0.65, 0.30)
    } else {
        Color::srgb(0.25, 0.18, 0.12)
    };
    parent
        .spawn((
            Button,
            DialogSelectButton { dialog_id },
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
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

/// Click a property row to enter edit mode for that value.
pub fn handle_property_row_click(
    rows: Query<(&EditorPropertyRow, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
) {
    for (row, interaction) in &rows {
        if *interaction == Interaction::Pressed {
            let index = row.index;
            if prop_buffer.editing_index == Some(index) {
                // Clicking current editing row again: do nothing.
                continue;
            }
            if let Some(initial_value) = prop_buffer.entries.get(index).map(|e| e.1.clone()) {
                prop_buffer.editing_index = Some(index);
                prop_buffer.editing_field = EditingField::Value;
                prop_buffer.edit_text = initial_value;
            }
        }
    }
}

/// "Add property" button: add a new blank (key, value) entry and start editing the key.
pub fn handle_add_property_button(
    add_btns: Query<&Interaction, (Changed<Interaction>, With<EditorPropertyAddButton>)>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
) {
    for interaction in &add_btns {
        if *interaction == Interaction::Pressed {
            let new_index = prop_buffer.entries.len();
            prop_buffer.entries.push((String::new(), String::new()));
            prop_buffer.editing_index = Some(new_index);
            prop_buffer.editing_field = EditingField::Key;
            prop_buffer.edit_text.clear();
        }
    }
}

const DEFAULT_BEHAVIOR_BOUNDS: TileRectangle = TileRectangle {
    min_x: 0,
    min_y: 0,
    max_x: 8,
    max_y: 8,
};

/// Click handlers for the Behavior set/clear buttons.
pub fn handle_behavior_set_buttons(
    btns: Query<(&BehaviorSetButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let before = object_registry.behavior(selected).cloned();
        let next = match btn.kind {
            BehaviorButtonKind::Roam => Some(MapBehavior::Roam {
                step_interval_seconds: 0.5,
                bounds: existing_bounds(&before).unwrap_or(DEFAULT_BEHAVIOR_BOUNDS),
            }),
            BehaviorButtonKind::RoamAndChase => Some(MapBehavior::RoamAndChase {
                step_interval_seconds: 0.5,
                bounds: existing_bounds(&before).unwrap_or(DEFAULT_BEHAVIOR_BOUNDS),
                detect_distance_tiles: existing_detect(&before).unwrap_or(4),
                disengage_distance_tiles: existing_disengage(&before).unwrap_or(6),
            }),
            BehaviorButtonKind::Clear => None,
        };
        object_registry.set_behavior(selected, next);
        undo_stack.push_undo(UndoOp::SetBehavior {
            object_id: selected,
            before,
        });
        editor_state.dirty = true;
    }
}

/// `Pick bounds on map` button — kicks the editor into PickRect mode targeting
/// the currently-selected NPC's behavior bounds.
pub fn handle_behavior_pick_bounds(
    btns: Query<&Interaction, (Changed<Interaction>, With<BehaviorPickBoundsButton>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for interaction in &btns {
        if *interaction == Interaction::Pressed {
            if !matches!(editor_state.current_tool, EditorTool::PickRect { .. }) {
                editor_state.tool_before_pick = Some(editor_state.current_tool);
            }
            editor_state.current_tool = EditorTool::PickRect {
                target: PickRectTarget::InstanceBehavior,
            };
        }
    }
}

/// Reads `EditorPickRectResult` for the `InstanceBehavior` target and applies
/// it to the selected object's behavior bounds.
pub fn apply_pick_rect_to_instance_behavior(
    mut pick_result: ResMut<EditorPickRectResult>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(picked) = pick_result.pending else {
        return;
    };
    if !matches!(picked.target, PickRectTarget::InstanceBehavior) {
        return;
    }
    let Some(selected) = editor_state.selected_object_id else {
        pick_result.pending = None;
        return;
    };
    let before = object_registry.behavior(selected).cloned();
    let next = Some(match before.clone() {
        Some(MapBehavior::Roam {
            step_interval_seconds,
            ..
        }) => MapBehavior::Roam {
            step_interval_seconds,
            bounds: picked.rect,
        },
        Some(MapBehavior::RoamAndChase {
            step_interval_seconds,
            detect_distance_tiles,
            disengage_distance_tiles,
            ..
        }) => MapBehavior::RoamAndChase {
            step_interval_seconds,
            bounds: picked.rect,
            detect_distance_tiles,
            disengage_distance_tiles,
        },
        // No behavior yet: default to Roam at the picked rect.
        None => MapBehavior::Roam {
            step_interval_seconds: 0.5,
            bounds: picked.rect,
        },
    });
    object_registry.set_behavior(selected, next);
    undo_stack.push_undo(UndoOp::SetBehavior {
        object_id: selected,
        before,
    });
    editor_state.dirty = true;
    pick_result.pending = None;
}

pub fn handle_behavior_nudge_buttons(
    btns: Query<(&BehaviorNudgeButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut undo_stack: ResMut<UndoStack>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(before) = object_registry.behavior(selected).cloned() else {
            continue;
        };
        let next = match (before.clone(), btn.field) {
            (
                MapBehavior::Roam {
                    step_interval_seconds,
                    bounds,
                },
                BehaviorNudgeField::StepIntervalTenths,
            ) => MapBehavior::Roam {
                step_interval_seconds: ((step_interval_seconds * 10.0).round() as i32 + btn.delta)
                    .max(1) as f32
                    / 10.0,
                bounds,
            },
            (
                MapBehavior::RoamAndChase {
                    step_interval_seconds,
                    bounds,
                    detect_distance_tiles,
                    disengage_distance_tiles,
                },
                BehaviorNudgeField::StepIntervalTenths,
            ) => MapBehavior::RoamAndChase {
                step_interval_seconds: ((step_interval_seconds * 10.0).round() as i32 + btn.delta)
                    .max(1) as f32
                    / 10.0,
                bounds,
                detect_distance_tiles,
                disengage_distance_tiles,
            },
            (
                MapBehavior::RoamAndChase {
                    step_interval_seconds,
                    bounds,
                    detect_distance_tiles,
                    disengage_distance_tiles,
                },
                BehaviorNudgeField::DetectDistance,
            ) => MapBehavior::RoamAndChase {
                step_interval_seconds,
                bounds,
                detect_distance_tiles: (detect_distance_tiles + btn.delta).max(0),
                disengage_distance_tiles,
            },
            (
                MapBehavior::RoamAndChase {
                    step_interval_seconds,
                    bounds,
                    detect_distance_tiles,
                    disengage_distance_tiles,
                },
                BehaviorNudgeField::DisengageDistance,
            ) => MapBehavior::RoamAndChase {
                step_interval_seconds,
                bounds,
                detect_distance_tiles,
                disengage_distance_tiles: (disengage_distance_tiles + btn.delta).max(0),
            },
            // detect/disengage nudges are no-ops for plain Roam.
            (other, _) => other,
        };
        object_registry.set_behavior(selected, Some(next));
        undo_stack.push_undo(UndoOp::SetBehavior {
            object_id: selected,
            before: Some(before),
        });
        editor_state.dirty = true;
    }
}

pub fn handle_dialog_select_buttons(
    btns: Query<(&DialogSelectButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let mut props = object_registry
            .properties(selected)
            .cloned()
            .unwrap_or_default();
        match &btn.dialog_id {
            Some(id) => {
                props.insert("dialog_id".to_owned(), id.clone());
            }
            None => {
                props.remove("dialog_id");
            }
        }
        object_registry.set_properties(selected, props.clone());
        // Mirror the change into the editor's per-object property buffer
        // so the property list shows the updated `dialog_id` on next render.
        if prop_buffer.object_id == Some(selected) {
            prop_buffer.entries = props.into_iter().collect();
            prop_buffer.entries.sort_by(|a, b| a.0.cmp(&b.0));
            prop_buffer.editing_index = None;
        }
        editor_state.dirty = true;
    }
}

fn existing_bounds(behavior: &Option<MapBehavior>) -> Option<TileRectangle> {
    behavior.as_ref().map(|b| match b {
        MapBehavior::Roam { bounds, .. } => *bounds,
        MapBehavior::RoamAndChase { bounds, .. } => *bounds,
    })
}

fn existing_detect(behavior: &Option<MapBehavior>) -> Option<i32> {
    behavior.as_ref().and_then(|b| match b {
        MapBehavior::RoamAndChase {
            detect_distance_tiles,
            ..
        } => Some(*detect_distance_tiles),
        _ => None,
    })
}

fn existing_disengage(behavior: &Option<MapBehavior>) -> Option<i32> {
    behavior.as_ref().and_then(|b| match b {
        MapBehavior::RoamAndChase {
            disengage_distance_tiles,
            ..
        } => Some(*disengage_distance_tiles),
        _ => None,
    })
}
