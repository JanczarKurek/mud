use bevy::prelude::*;

use crate::editor::resources::{EditingField, EditorPropertyEditBuffer, EditorState};
use crate::world::components::OverworldObject;
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
                        TextFont { font_size: 14.0, ..default() },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                    ));
                    h.spawn((
                        EditorPropertiesHeader,
                        Text::new(""),
                        TextFont { font_size: 11.0, ..default() },
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
    if !prop_buffer.is_changed() && !editor_state.is_changed() {
        return;
    }

    let Ok(content_entity) = content_query.single() else {
        return;
    };

    // Despawn old property rows.
    commands.entity(content_entity).despawn_related::<Children>();

    // Get current properties to display.
    let entries = if prop_buffer.object_id == Some(selected_id) {
        prop_buffer.entries.clone()
    } else {
        object_registry
            .properties(selected_id)
            .map(|p| {
                let mut v: Vec<(String, String)> = p.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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
                TextFont { font_size: 11.0, ..default() },
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
                        TextFont { font_size: 11.0, ..default() },
                        TextColor(Color::srgb(0.80, 0.76, 0.68)),
                        Node { flex_shrink: 0.0, ..default() },
                    ));
                    row.spawn((
                        Text::new(":"),
                        TextFont { font_size: 11.0, ..default() },
                        TextColor(Color::srgb(0.55, 0.50, 0.45)),
                    ));
                    row.spawn((
                        EditorPropertyValueText { index },
                        Text::new(displayed_value),
                        TextFont { font_size: 11.0, ..default() },
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
        content.spawn((
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
                        TextFont { font_size: 11.0, ..default() },
                        TextColor(Color::srgb(0.85, 0.80, 0.70)),
                    ));
                });
        });
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
