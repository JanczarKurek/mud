use bevy::prelude::*;

use crate::editor::resources::EditorState;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Marks the palette panel root.
#[derive(Component)]
pub struct EditorPaletteRoot;

/// Marks a palette item button; carries the type_id it represents.
#[derive(Component, Clone)]
pub struct EditorPaletteItem {
    pub type_id: String,
}

/// Spawn the left-sidebar object palette.
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
            panel.spawn((
                Node {
                    padding: UiRect::all(Val::Px(8.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
            ))
            .with_children(|header| {
                header.spawn((
                    Text::new("Objects"),
                    TextFont { font_size: 14.0, ..default() },
                    TextColor(Color::srgb(0.96, 0.84, 0.62)),
                ));
            });

            // Scrollable list
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
                        let Some(def) = definitions.get(type_id) else { continue };
                        let color = def.debug_color();

                        list.spawn((
                            Button,
                            EditorPaletteItem {
                                type_id: type_id.to_owned(),
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
                            // Debug color swatch
                            btn.spawn((
                                Node {
                                    width: Val::Px(12.0),
                                    height: Val::Px(12.0),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                BackgroundColor(color),
                            ));

                            // Name
                            btn.spawn((
                                Text::new(def.name.clone()),
                                TextFont { font_size: 11.0, ..default() },
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

/// Highlight the selected palette item.
pub fn sync_palette_selection(
    editor_state: Res<EditorState>,
    mut items: Query<(&EditorPaletteItem, &Interaction, &mut BackgroundColor, &mut BorderColor), With<Button>>,
) {
    for (item, interaction, mut bg, mut border) in &mut items {
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
            (Interaction::None, false) => {
                (Color::srgba(0.10, 0.07, 0.06, 0.80), Color::srgb(0.20, 0.15, 0.10))
            }
        };

        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }
}

/// Update editor state when palette items are clicked.
pub fn handle_palette_clicks(
    mut editor_state: ResMut<EditorState>,
    items: Query<(&EditorPaletteItem, &Interaction), (Changed<Interaction>, With<Button>)>,
) {
    for (item, interaction) in &items {
        if *interaction == Interaction::Pressed {
            if editor_state.selected_type_id.as_deref() == Some(&item.type_id) {
                // Click same type again to deselect.
                editor_state.selected_type_id = None;
            } else {
                editor_state.selected_type_id = Some(item.type_id.clone());
                editor_state.selected_object_id = None;
            }
        }
    }
}
