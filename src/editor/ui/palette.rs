#![allow(clippy::type_complexity)]
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::editor::resources::{EditorState, EditorTool};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Component)]
pub struct EditorPaletteRoot;

/// Marks one of the two scrollable list bodies inside the palette panel.
#[derive(Component, Clone, Copy, Debug)]
pub enum EditorScrollableList {
    Objects,
    Floors,
}

#[derive(Component, Clone)]
pub struct EditorPaletteItem {
    pub type_id: String,
    /// Display name for filter matching.
    pub display_name: String,
}

#[derive(Component)]
pub struct EditorPaletteFilterBox;

/// Marks a row in the floor-tile palette. `floor_id = None` is the eraser.
#[derive(Component, Clone)]
pub struct EditorFloorPaletteItem {
    pub floor_id: Option<String>,
}

pub fn spawn_palette_panel(
    parent: &mut ChildSpawnerCommands,
    definitions: &OverworldObjectDefinitions,
    floor_defs: &FloorTilesetDefinitions,
) {
    parent
        .spawn((
            EditorPaletteRoot,
            Node {
                width: Val::Px(200.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(Val::Px(1.0)),
                overflow: Overflow::clip_y(),
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
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Objects"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                    ));
                });

            // Filter row
            panel
                .spawn((
                    EditorPaletteFilterBox,
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.08, 0.05, 0.05, 0.90)),
                    BorderColor::all(Color::srgb(0.25, 0.18, 0.12)),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new("🔍 filter…"),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.50, 0.46, 0.42)),
                    ));
                });

            // Scrollable object list — shares remaining vertical space 50/50
            // with the floors list below via matching `flex_grow`. `min_height:
            // 0` lets flexbox actually shrink the list inside its scroll
            // viewport instead of letting its natural content height push the
            // floors section past the panel's bottom.
            panel
                .spawn((
                    EditorScrollableList::Objects,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                ))
                .with_children(|list| {
                    let mut type_ids: Vec<&str> = definitions.ids().collect();
                    type_ids.sort();

                    for type_id in type_ids {
                        let Some(def) = definitions.get(type_id) else {
                            continue;
                        };
                        let color = def.debug_color();
                        let display_name = def.name.clone();

                        list.spawn((
                            Button,
                            EditorPaletteItem {
                                type_id: type_id.to_owned(),
                                display_name: display_name.clone(),
                            },
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                border: UiRect::bottom(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.10, 0.07, 0.06, 0.80)),
                            BorderColor::all(Color::srgb(0.20, 0.15, 0.10)),
                        ))
                        .with_children(|btn| {
                            btn.spawn((
                                Node {
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(color),
                            ));
                            btn.spawn((
                                Text::new(display_name),
                                TextFont {
                                    font_size: 11.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.88, 0.84, 0.78)),
                                Node {
                                    overflow: Overflow::clip_x(),
                                    ..default()
                                },
                            ));
                        });
                    }
                });

            // Floors header
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        border: UiRect::axes(Val::Px(0.0), Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Floors  (B = object brush)"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                    ));
                });

            // Floor list (Erase + each FloorTilesetDefinition). Same flex
            // sizing as the objects list so they share remaining height 50/50.
            panel
                .spawn((
                    EditorScrollableList::Floors,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                ))
                .with_children(|list| {
                    spawn_floor_row(list, None, "Erase", Color::srgba(0.0, 0.0, 0.0, 0.0));

                    let mut floor_defs_sorted: Vec<&crate::world::floor_definitions::FloorTilesetDefinition> =
                        floor_defs.iter().collect();
                    floor_defs_sorted.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));

                    for def in floor_defs_sorted {
                        spawn_floor_row(list, Some(def.id.clone()), &def.name, def.debug_color());
                    }
                });
        });
}

fn spawn_floor_row(
    list: &mut ChildSpawnerCommands,
    floor_id: Option<String>,
    label: &str,
    swatch_color: Color,
) {
    list.spawn((
        Button,
        EditorFloorPaletteItem {
            floor_id: floor_id.clone(),
        },
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.10, 0.07, 0.06, 0.80)),
        BorderColor::all(Color::srgb(0.20, 0.15, 0.10)),
    ))
    .with_children(|btn| {
        btn.spawn((
            Node {
                width: Val::Px(12.0),
                height: Val::Px(12.0),
                flex_shrink: 0.0,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(swatch_color),
            BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
        ));
        btn.spawn((
            Text::new(label.to_owned()),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(Color::srgb(0.88, 0.84, 0.78)),
            Node {
                overflow: Overflow::clip_x(),
                ..default()
            },
        ));
    });
}

pub fn sync_palette_selection(
    editor_state: Res<EditorState>,
    mut items: Query<
        (
            &EditorPaletteItem,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut Node,
        ),
        With<Button>,
    >,
    mut filter_box: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (With<EditorPaletteFilterBox>, Without<EditorPaletteItem>),
    >,
) {
    let filter = editor_state.palette_filter.to_lowercase();
    let filter_focused = editor_state.palette_filter_focused;

    for (item, interaction, mut bg, mut border, mut node) in &mut items {
        // Hide non-matching rows from layout (not just from rendering) so the
        // remaining items collapse to the top of the list.
        let matches = filter.is_empty()
            || item.type_id.to_lowercase().contains(&filter)
            || item.display_name.to_lowercase().contains(&filter);
        let target_display = if matches {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != target_display {
            node.display = target_display;
        }

        if !matches {
            continue;
        }

        let is_selected = editor_state
            .selected_type_id
            .as_deref()
            .is_some_and(|id| id == item.type_id);
        let (bg_color, border_color) = match (*interaction, is_selected) {
            (Interaction::Pressed, _) => {
                (Color::srgb(0.50, 0.28, 0.12), Color::srgb(0.98, 0.84, 0.58))
            }
            (Interaction::Hovered, true) => {
                (Color::srgb(0.35, 0.20, 0.10), Color::srgb(0.98, 0.84, 0.58))
            }
            (Interaction::Hovered, false) => {
                (Color::srgb(0.20, 0.13, 0.10), Color::srgb(0.60, 0.45, 0.28))
            }
            (Interaction::None, true) => {
                (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50))
            }
            (Interaction::None, false) => (
                Color::srgba(0.10, 0.07, 0.06, 0.80),
                Color::srgb(0.20, 0.15, 0.10),
            ),
        };
        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }

    // Sync filter box appearance
    for (interaction, mut bg, mut border) in &mut filter_box {
        let (b, br) = if filter_focused {
            (
                Color::srgba(0.12, 0.08, 0.06, 0.95),
                Color::srgb(0.90, 0.72, 0.40),
            )
        } else {
            match *interaction {
                Interaction::Hovered => (
                    Color::srgba(0.12, 0.08, 0.06, 0.95),
                    Color::srgb(0.50, 0.38, 0.22),
                ),
                _ => (
                    Color::srgba(0.08, 0.05, 0.05, 0.90),
                    Color::srgb(0.25, 0.18, 0.12),
                ),
            }
        };
        bg.0 = b;
        *border = BorderColor::all(br);
    }
}

pub fn sync_palette_filter_text(
    editor_state: Res<EditorState>,
    filter_box: Query<Entity, With<EditorPaletteFilterBox>>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    if !editor_state.is_changed() {
        return;
    }
    let Ok(box_entity) = filter_box.single() else {
        return;
    };
    let Ok(kids) = children.get(box_entity) else {
        return;
    };
    for child in kids.iter() {
        if let Ok(mut text) = texts.get_mut(child) {
            text.0 = if editor_state.palette_filter_focused {
                format!("{}_", editor_state.palette_filter)
            } else if editor_state.palette_filter.is_empty() {
                "🔍 filter…".into()
            } else {
                format!("🔍 {}", editor_state.palette_filter)
            };
        }
    }
}

pub fn handle_palette_filter_click(
    filter_btn: Query<&Interaction, (Changed<Interaction>, With<EditorPaletteFilterBox>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for interaction in &filter_btn {
        if *interaction == Interaction::Pressed {
            editor_state.palette_filter_focused = true;
        }
    }
}

pub fn handle_palette_clicks(
    mut editor_state: ResMut<EditorState>,
    items: Query<(&EditorPaletteItem, &Interaction), (Changed<Interaction>, With<Button>)>,
) {
    for (item, interaction) in &items {
        if *interaction == Interaction::Pressed {
            // Clicking an object palette item exits filter mode and switches
            // back to the object brush, so selection in this list is always
            // active immediately.
            editor_state.palette_filter_focused = false;
            editor_state.current_tool = EditorTool::Brush;
            if editor_state.selected_type_id.as_deref() == Some(&item.type_id) {
                editor_state.selected_type_id = None;
            } else {
                editor_state.selected_type_id = Some(item.type_id.clone());
                editor_state.selected_object_id = None;
            }
        }
    }
}

pub fn handle_floor_palette_clicks(
    mut editor_state: ResMut<EditorState>,
    items: Query<(&EditorFloorPaletteItem, &Interaction), (Changed<Interaction>, With<Button>)>,
) {
    for (item, interaction) in &items {
        if *interaction == Interaction::Pressed {
            editor_state.palette_filter_focused = false;
            editor_state.current_tool = EditorTool::FloorBrush;
            editor_state.selected_floor_type = item.floor_id.clone();
        }
    }
}

pub fn handle_palette_scrolling(
    mut mouse_wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut lists: Query<(
        &EditorScrollableList,
        &Node,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    )>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    for event in mouse_wheel.read() {
        let mut delta_y = -event.y;
        if event.unit == MouseScrollUnit::Line {
            delta_y *= 21.0;
        }
        if delta_y == 0.0 {
            continue;
        }
        for (_marker, node, computed, transform, mut scroll_position) in &mut lists {
            if !computed.contains_point(*transform, cursor) {
                continue;
            }
            if node.overflow.y != bevy::ui::OverflowAxis::Scroll {
                continue;
            }
            let max_offset =
                (computed.content_size() - computed.size()) * computed.inverse_scale_factor();
            if max_offset.y <= 0.0 {
                continue;
            }
            scroll_position.y = (scroll_position.y + delta_y).clamp(0.0, max_offset.y);
            break;
        }
    }
}

pub fn sync_floor_palette_selection(
    editor_state: Res<EditorState>,
    mut items: Query<
        (
            &EditorFloorPaletteItem,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    let active_floor_tool = editor_state.current_tool == EditorTool::FloorBrush;
    for (item, interaction, mut bg, mut border) in &mut items {
        let is_selected =
            active_floor_tool && editor_state.selected_floor_type == item.floor_id;
        let (bg_color, border_color) = match (*interaction, is_selected) {
            (Interaction::Pressed, _) => {
                (Color::srgb(0.50, 0.28, 0.12), Color::srgb(0.98, 0.84, 0.58))
            }
            (Interaction::Hovered, true) => {
                (Color::srgb(0.35, 0.20, 0.10), Color::srgb(0.98, 0.84, 0.58))
            }
            (Interaction::Hovered, false) => {
                (Color::srgb(0.20, 0.13, 0.10), Color::srgb(0.60, 0.45, 0.28))
            }
            (Interaction::None, true) => {
                (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50))
            }
            (Interaction::None, false) => (
                Color::srgba(0.10, 0.07, 0.06, 0.80),
                Color::srgb(0.20, 0.15, 0.10),
            ),
        };
        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }
}
