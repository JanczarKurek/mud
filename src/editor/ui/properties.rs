use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::editor::dialog_index::EditorDialogIndex;
use crate::editor::resources::{
    EditingField, EditorCamera, EditorPickRectResult, EditorPropertyEditBuffer, EditorStackEdit,
    EditorState, EditorTool, PendingStackCountChanges, PickRectTarget, UndoOp, UndoStack,
};
use crate::editor::systems::insert_editor_visuals_pub;
use crate::editor::ui::style::{
    editor_action_button, editor_button, ButtonColors, BUTTON_BG, BUTTON_BORDER, BUTTON_TEXT,
    HEADER_TEXT, PANEL_BG, PANEL_BORDER,
};
use crate::world::components::{OverworldObject, Quantity, TilePosition};
use crate::world::map_layout::{MapBehavior, TileRectangle};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::WorldConfig;

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

#[derive(Component, Clone)]
pub struct DialogSelectButton {
    pub dialog_id: Option<String>,
}

// ── Stack-count section ─────────────────────────────────────────────────────

/// Draggable slider track for the pile size. `max` is the definition's
/// effective `max_stack_size`.
#[derive(Component, Clone, Copy)]
pub struct StackCountSlider {
    pub max: u32,
}

/// Filled portion of the slider track; its width % encodes the current count.
#[derive(Component)]
pub struct StackCountFill;

/// The "N / max" readout text.
#[derive(Component)]
pub struct StackReadout;

/// A `−` / `+` stepper button; `delta` is ±1.
#[derive(Component, Clone, Copy)]
pub struct StackStepButton {
    pub delta: i32,
}

/// The click-to-focus numeric input field for the stack count.
#[derive(Component)]
pub struct StackCountInput;

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
                // Hidden via `Display::None` (not `Visibility::Hidden`) so the
                // node collapses out of layout when empty — otherwise its
                // 220px×100% rectangle keeps blocking the map cursor along the
                // whole right edge (see `EditorPanelRoots::cursor_over`).
                display: Display::None,
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
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
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Properties"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(HEADER_TEXT),
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
pub fn sync_properties_panel(
    editor_state: Res<EditorState>,
    prop_buffer: Res<EditorPropertyEditBuffer>,
    object_registry: Res<ObjectRegistry>,
    object_definitions: Res<OverworldObjectDefinitions>,
    dialog_index: Res<EditorDialogIndex>,
    stack_edit: Res<EditorStackEdit>,
    contents_buffer: Res<crate::editor::resources::EditorContentsBuffer>,
    object_query: Query<&OverworldObject>,
    container_query: Query<(&OverworldObject, &crate::world::components::Container)>,
    mut root_query: Query<&mut Node, With<EditorPropertiesRoot>>,
    mut header_query: Query<&mut Text, With<EditorPropertiesHeader>>,
    content_query: Query<Entity, With<EditorPropertiesContent>>,
    mut commands: Commands,
) {
    let Ok(mut root_node) = root_query.single_mut() else {
        return;
    };

    let Some(selected_id) = editor_state.selected_object_id else {
        if root_node.display != Display::None {
            root_node.display = Display::None;
        }
        return;
    };

    if root_node.display != Display::Flex {
        root_node.display = Display::Flex;
    }

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
        && !stack_edit.is_changed()
        && !contents_buffer.is_changed()
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

    // Get current properties to display. The `quantity` key is edited through
    // the dedicated Stack section below, so hide it from the generic row list
    // to avoid two controls fighting over the same value.
    let mut entries = if prop_buffer.object_id == Some(selected_id) {
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
    entries.retain(|(k, _)| k != "quantity");

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

        // ── Stack count ─────────────────────────────────────────────────────
        // Only stackable definitions (max_stack_size > 1) get a pile-size
        // control. The effective cap is already flattened through the
        // `extends` chain at load, so a plain `get` gives the right value.
        let max_stack = definition_id
            .as_deref()
            .and_then(|id| object_definitions.get(id))
            .map(|def| def.max_stack_size)
            .unwrap_or(1);
        if max_stack > 1 {
            let current = object_registry
                .properties(selected_id)
                .and_then(|p| p.get("quantity"))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1)
                .clamp(1, max_stack);
            spawn_stack_section(content, current, max_stack, &stack_edit);
        }

        // ── Container contents ──────────────────────────────────────────────
        let capacity = definition_id
            .as_deref()
            .and_then(|id| object_definitions.get(id))
            .and_then(|def| def.container_capacity);
        if let Some(capacity) = capacity {
            if let Some((_, container)) = container_query
                .iter()
                .find(|(o, _)| o.object_id == selected_id)
            {
                crate::editor::ui::contents::spawn_contents_section(
                    content,
                    capacity,
                    container,
                    &object_definitions,
                    &contents_buffer,
                );
            }
        }
    });
}

/// Build the "Stack" section: a live "N / max" readout, a `−`/`+` stepper pair
/// flanking a draggable slider track, and a click-to-type numeric input.
fn spawn_stack_section(
    parent: &mut ChildSpawnerCommands,
    current: u32,
    max: u32,
    stack_edit: &EditorStackEdit,
) {
    let fill_pct = count_to_fill_pct(current, max);
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                margin: UiRect::top(Val::Px(8.0)),
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(4.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.25, 0.18, 0.12)),
        ))
        .with_children(|sec| {
            sec.spawn((
                Text::new("Stack"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(HEADER_TEXT),
            ));
            sec.spawn((
                StackReadout,
                Text::new(format!("{current} / {max}")),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.78, 0.74, 0.66)),
            ));
            // Stepper row: [−] [ slider track ] [+]
            sec.spawn((Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            },))
                .with_children(|row| {
                    stack_step_button(row, "-", -1);
                    // Slider track (fixed height, grows to fill the row).
                    row.spawn((
                        StackCountSlider { max },
                        Node {
                            flex_grow: 1.0,
                            height: Val::Px(14.0),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.10, 0.08, 0.06, 0.90)),
                        BorderColor::all(Color::srgb(0.35, 0.26, 0.16)),
                    ))
                    .with_children(|track| {
                        track.spawn((
                            StackCountFill,
                            Node {
                                width: Val::Percent(fill_pct),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.72, 0.52, 0.22)),
                        ));
                    });
                    stack_step_button(row, "+", 1);
                });
            // Numeric input field.
            let input_text = if stack_edit.editing {
                format!("[{}]", stack_edit.text)
            } else {
                current.to_string()
            };
            sec.spawn((
                Button,
                StackCountInput,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(if stack_edit.editing {
                    Color::srgba(0.20, 0.15, 0.08, 0.90)
                } else {
                    Color::srgba(0.12, 0.09, 0.06, 0.80)
                }),
                BorderColor::all(if stack_edit.editing {
                    Color::srgb(0.90, 0.72, 0.40)
                } else {
                    Color::srgb(0.30, 0.22, 0.14)
                }),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(input_text),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(BUTTON_TEXT),
                ));
            });
        });
}

fn stack_step_button(parent: &mut ChildSpawnerCommands, label: &str, delta: i32) {
    parent
        .spawn((
            Button,
            StackStepButton { delta },
            Node {
                width: Val::Px(20.0),
                height: Val::Px(18.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(BUTTON_BG),
            BorderColor::all(BUTTON_BORDER),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(BUTTON_TEXT),
            ));
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
                TextColor(HEADER_TEXT),
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
                BorderColor::all(BUTTON_BORDER),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new("Pick bounds on map"),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(BUTTON_TEXT),
                ));
            });
        });
}

fn behavior_button(parent: &mut ChildSpawnerCommands, label: &str, kind: BehaviorButtonKind) {
    editor_action_button(
        parent,
        label,
        BehaviorSetButton { kind },
        UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
    );
}

fn behavior_summary(behavior: Option<&MapBehavior>) -> String {
    match behavior {
        None => "(no behavior)".to_owned(),
        Some(MapBehavior::Roam { bounds }) => format!(
            "Roam  ({},{})-({},{})",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
        ),
        Some(MapBehavior::RoamAndChase { bounds }) => format!(
            "Chase  ({},{})-({},{})",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
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
                TextColor(HEADER_TEXT),
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
    editor_button(
        parent,
        DialogSelectButton { dialog_id },
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        ButtonColors {
            bg,
            border,
            text: BUTTON_TEXT,
        },
        label,
        10.0,
    );
}

/// Click a property row to enter edit mode for that value.
pub fn handle_property_row_click(
    rows: Query<(&EditorPropertyRow, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut vendor_stash_buffer: ResMut<crate::editor::resources::EditorVendorStashBuffer>,
    mut contents_buffer: ResMut<crate::editor::resources::EditorContentsBuffer>,
) {
    for (row, interaction) in &rows {
        if *interaction == Interaction::Pressed {
            let index = row.index;
            if prop_buffer.editing_index == Some(index) {
                // Clicking current editing row again: do nothing.
                continue;
            }
            if let Some(initial_value) = prop_buffer.entries.get(index).map(|e| e.1.clone()) {
                // Drop any active vendor-stash / contents edit so the keyboard
                // pipeline doesn't route keystrokes into multiple panels.
                vendor_stash_buffer.editing = None;
                vendor_stash_buffer.edit_text.clear();
                contents_buffer.editing = None;
                contents_buffer.edit_text.clear();
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
    mut vendor_stash_buffer: ResMut<crate::editor::resources::EditorVendorStashBuffer>,
    mut contents_buffer: ResMut<crate::editor::resources::EditorContentsBuffer>,
) {
    for interaction in &add_btns {
        if *interaction == Interaction::Pressed {
            vendor_stash_buffer.editing = None;
            vendor_stash_buffer.edit_text.clear();
            contents_buffer.editing = None;
            contents_buffer.edit_text.clear();
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
        let bounds = existing_bounds(&before).unwrap_or(DEFAULT_BEHAVIOR_BOUNDS);
        let next = match btn.kind {
            BehaviorButtonKind::Roam => Some(MapBehavior::Roam { bounds }),
            BehaviorButtonKind::RoamAndChase => Some(MapBehavior::RoamAndChase { bounds }),
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
    let next = Some(match before {
        Some(existing) => existing.with_bounds(picked.rect),
        // No behavior yet: default to Roam at the picked rect.
        None => MapBehavior::Roam {
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
        // `quantity` is excluded — the Stack section owns it (keeping it out of
        // the buffer preserves row-index alignment).
        if prop_buffer.object_id == Some(selected) {
            prop_buffer.entries = props.into_iter().filter(|(k, _)| k != "quantity").collect();
            prop_buffer.entries.sort_by(|a, b| a.0.cmp(&b.0));
            prop_buffer.editing_index = None;
        }
        editor_state.dirty = true;
    }
}

fn existing_bounds(behavior: &Option<MapBehavior>) -> Option<TileRectangle> {
    behavior.as_ref().map(|b| b.bounds())
}

/// Map a normalized slider fraction `0.0..=1.0` to a count in `1..=max`.
fn slider_frac_to_count(frac: f32, max: u32) -> u32 {
    let max = max.max(1);
    1 + (frac.clamp(0.0, 1.0) * (max - 1) as f32).round() as u32
}

/// Fill-bar width percentage for `count` within `1..=max`.
fn count_to_fill_pct(count: u32, max: u32) -> f32 {
    if max > 1 {
        (count.clamp(1, max) - 1) as f32 / (max - 1) as f32 * 100.0
    } else {
        0.0
    }
}

/// Read the current pile size from an object's `quantity` property (default 1).
fn stack_count_of(object_registry: &ObjectRegistry, object_id: u64) -> u32 {
    object_registry
        .properties(object_id)
        .and_then(|p| p.get("quantity"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1)
}

/// Local drag state for the stack slider.
#[derive(Default)]
pub struct StackSliderDragState {
    active: bool,
    count: u32,
}

/// Drag the stack slider. While the LMB is held after a press that landed on
/// the track, the cursor x maps to a count in `1..=max` and the fill / readout
/// update **in place** (no registry write, so the panel does not rebuild and
/// the slider node stays stable). The count is committed to the registry on
/// release via `PendingStackCountChanges`.
pub fn handle_stack_slider_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    editor_state: Res<EditorState>,
    sliders: Query<(&ComputedNode, &UiGlobalTransform, &StackCountSlider)>,
    mut fills: Query<&mut Node, With<StackCountFill>>,
    mut readouts: Query<&mut Text, With<StackReadout>>,
    mut pending: ResMut<PendingStackCountChanges>,
    mut drag: Local<StackSliderDragState>,
) {
    if !mouse.pressed(MouseButton::Left) {
        if drag.active {
            if let Some(id) = editor_state.selected_object_id {
                pending.0.push((id, drag.count.max(1)));
            }
            drag.active = false;
        }
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some((computed, transform, slider)) = sliders.iter().next() else {
        return;
    };
    let max = slider.max.max(1);

    if mouse.just_pressed(MouseButton::Left) {
        // Only begin a drag if the press landed on the track.
        if !crate::ui::movable_window::point_in_node(cursor, computed, transform) {
            return;
        }
        drag.active = true;
    }
    if !drag.active {
        return;
    }

    let size = computed.size();
    if size.x <= 0.0 {
        return;
    }
    // Physical-space math (mirrors the minimap pan handler) so drag tracking
    // survives HiDPI scaling.
    let inv = computed.inverse_scale_factor();
    let physical_x = if inv > 0.0 { cursor.x / inv } else { cursor.x };
    let left = transform.translation.x - size.x * 0.5;
    let frac = (physical_x - left) / size.x;
    let count = slider_frac_to_count(frac, max);
    drag.count = count;

    let fill_pct = count_to_fill_pct(count, max);
    if let Ok(mut node) = fills.single_mut() {
        node.width = Val::Percent(fill_pct);
    }
    if let Ok(mut text) = readouts.single_mut() {
        text.0 = format!("{count} / {max}");
    }
}

/// `−` / `+` steppers adjust the pile size by ±1 (clamped in the apply system).
pub fn handle_stack_step_buttons(
    btns: Query<(&StackStepButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    editor_state: Res<EditorState>,
    object_registry: Res<ObjectRegistry>,
    mut pending: ResMut<PendingStackCountChanges>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let current = stack_count_of(&object_registry, selected) as i32;
        let next = (current + btn.delta).max(1) as u32;
        pending.0.push((selected, next));
    }
}

/// Clicking the numeric field focuses it for digits-only text entry, seeding
/// the buffer with the current count. Drops the property / vendor-stash edits
/// so keystrokes route only to the stack input.
pub fn handle_stack_input_click(
    inputs: Query<&Interaction, (Changed<Interaction>, With<StackCountInput>)>,
    editor_state: Res<EditorState>,
    object_registry: Res<ObjectRegistry>,
    mut stack_edit: ResMut<EditorStackEdit>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut vendor_stash_buffer: ResMut<crate::editor::resources::EditorVendorStashBuffer>,
    mut contents_buffer: ResMut<crate::editor::resources::EditorContentsBuffer>,
) {
    for interaction in &inputs {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(selected) = editor_state.selected_object_id else {
            continue;
        };
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
        vendor_stash_buffer.editing = None;
        vendor_stash_buffer.edit_text.clear();
        contents_buffer.editing = None;
        contents_buffer.edit_text.clear();
        stack_edit.editing = true;
        stack_edit.text = stack_count_of(&object_registry, selected).to_string();
    }
}

/// Drain `PendingStackCountChanges`: clamp to the definition's `max_stack_size`,
/// write/remove the `quantity` property, insert/remove the `Quantity` component
/// on the live entity, and rebuild its editor sprite to reflect the stack tier.
/// Matches the existing property-edit convention of no undo entry (only sets
/// `editor_state.dirty`).
pub fn apply_stack_count_changes(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    object_definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut editor_state: ResMut<EditorState>,
    mut pending: ResMut<PendingStackCountChanges>,
    objects: Query<(Entity, &OverworldObject, &TilePosition)>,
) {
    if pending.0.is_empty() {
        return;
    }
    let changes = std::mem::take(&mut pending.0);
    for (object_id, requested) in changes {
        let Some(type_id) = object_registry.type_id(object_id).map(|s| s.to_owned()) else {
            continue;
        };
        let def = object_definitions.get(&type_id);
        let max = def.map(|d| d.max_stack_size).unwrap_or(1).max(1);
        let count = requested.clamp(1, max);

        let mut props = object_registry
            .properties(object_id)
            .cloned()
            .unwrap_or_default();
        if count > 1 {
            props.insert("quantity".to_owned(), count.to_string());
        } else {
            props.remove("quantity");
        }
        object_registry.set_properties(object_id, props);
        // `quantity` stays out of the generic property buffer (Stack section
        // owns it); just clear any active edit so the row list re-renders.
        if prop_buffer.object_id == Some(object_id) {
            prop_buffer.editing_index = None;
        }
        editor_state.dirty = true;

        if let Some((entity, _, tile)) = objects.iter().find(|(_, o, _)| o.object_id == object_id) {
            let tile = *tile;
            let mut ec = commands.entity(entity);
            if count > 1 {
                ec.try_insert(Quantity(count));
            } else {
                ec.remove::<Quantity>();
            }
            if let Some(def) = def {
                insert_editor_visuals_pub(
                    &mut ec,
                    &asset_server,
                    &mut texture_atlas_layouts,
                    def,
                    &world_config,
                    tile,
                    &editor_camera,
                    count,
                );
            }
        }
    }
}

#[cfg(test)]
mod stack_tests {
    use super::*;

    #[test]
    fn frac_maps_to_full_range() {
        // Endpoints and midpoint of a 1..=100 slider.
        assert_eq!(slider_frac_to_count(0.0, 100), 1);
        assert_eq!(slider_frac_to_count(1.0, 100), 100);
        assert_eq!(slider_frac_to_count(0.5, 100), 51); // 1 + round(0.5*99)
                                                        // Out-of-range fractions clamp.
        assert_eq!(slider_frac_to_count(-0.3, 50), 1);
        assert_eq!(slider_frac_to_count(2.0, 50), 50);
    }

    #[test]
    fn non_stackable_max_pins_to_one() {
        assert_eq!(slider_frac_to_count(0.7, 1), 1);
        assert_eq!(count_to_fill_pct(1, 1), 0.0);
    }

    #[test]
    fn fill_pct_endpoints() {
        assert_eq!(count_to_fill_pct(1, 100), 0.0);
        assert_eq!(count_to_fill_pct(100, 100), 100.0);
        // Over-max clamps rather than exceeding 100%.
        assert_eq!(count_to_fill_pct(250, 100), 100.0);
    }
}
