#![allow(clippy::type_complexity)]
use bevy::prelude::*;

use crate::editor::resources::EditorState;
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Component)]
pub struct EditorPaletteRoot;

#[derive(Component, Clone)]
pub struct EditorPaletteItem {
    pub type_id: String,
    /// Display name for filter matching.
    pub display_name: String,
}

#[derive(Component)]
pub struct EditorPaletteFilterBox;

pub fn spawn_palette_panel(
    parent: &mut ChildSpawnerCommands,
    definitions: &OverworldObjectDefinitions,
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

            // Scrollable item list
            panel
                .spawn((Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip_y(),
                    ..default()
                },))
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
            &mut Visibility,
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

    for (item, interaction, mut bg, mut border, mut vis) in &mut items {
        // Show/hide based on filter
        let matches = filter.is_empty()
            || item.type_id.to_lowercase().contains(&filter)
            || item.display_name.to_lowercase().contains(&filter);
        *vis = if matches {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

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
            // Clicking a palette item exits filter mode
            editor_state.palette_filter_focused = false;
            if editor_state.selected_type_id.as_deref() == Some(&item.type_id) {
                editor_state.selected_type_id = None;
            } else {
                editor_state.selected_type_id = Some(item.type_id.clone());
                editor_state.selected_object_id = None;
            }
        }
    }
}
