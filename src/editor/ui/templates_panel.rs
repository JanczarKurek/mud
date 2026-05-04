//! Togglable side panel listing on-disk template files.
//!
//! Visibility is driven by `EditorState::templates_panel_visible`. The panel
//! mirrors `palette.rs`'s scrollable column pattern. Clicking a template name
//! loads it into `EditorClipboard` and enters paste mode (the user can then
//! click on the map to stamp).

use bevy::prelude::*;

use crate::editor::resources::{EditorClipboard, EditorState};
use crate::editor::templates::{list_templates, load_template, EditorTemplatesIndex};

/// Marker for the templates panel root node — used by `cursor_over_editor_panels`
/// (so panel clicks don't fall through to the world) and by visibility-sync.
#[derive(Component)]
pub struct EditorTemplatesRoot;

#[derive(Component)]
pub struct EditorTemplatesContent;

#[derive(Component, Clone)]
pub struct EditorTemplateRow {
    pub name: String,
}

#[derive(Component)]
pub struct EditorTemplatesRefreshButton;

const PANEL_WIDTH_PX: f32 = 200.0;

pub fn spawn_templates_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorTemplatesRoot,
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
            // Header row: title + refresh.
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
                        Text::new("Templates"),
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
                        EditorTemplatesRefreshButton,
                        Node {
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.12, 0.08, 0.06, 0.90)),
                        BorderColor::all(Color::srgb(0.38, 0.28, 0.18)),
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("⟳"),
                            TextFont {
                                font_size: 12.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.88, 0.84, 0.76)),
                        ));
                    });
                });

            // Scrollable content area; contents are rebuilt each time
            // `EditorTemplatesIndex.names` changes.
            panel.spawn((
                EditorTemplatesContent,
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

/// Applies `templates_panel_visible` by toggling the panel root's `Display`.
/// Bevy 0.18 uses `Display::Flex` / `Display::None` rather than a layout-aware
/// `Visibility` for hide-and-collapse behavior.
pub fn sync_templates_panel_visibility(
    editor_state: Res<EditorState>,
    mut roots: Query<&mut Node, With<EditorTemplatesRoot>>,
) {
    if !editor_state.is_changed() {
        return;
    }
    let target = if editor_state.templates_panel_visible {
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

/// Lazy-load the on-disk template list when the panel becomes visible. Also
/// rebuilds the content list when the index changes (e.g. after a save or
/// refresh-button click).
pub fn sync_templates_panel(
    editor_state: Res<EditorState>,
    mut templates_index: ResMut<EditorTemplatesIndex>,
    content: Query<Entity, With<EditorTemplatesContent>>,
    rows: Query<Entity, With<EditorTemplateRow>>,
    mut commands: Commands,
) {
    if !editor_state.templates_panel_visible {
        return;
    }
    if !templates_index.loaded {
        match list_templates() {
            Ok(names) => {
                templates_index.names = names;
                templates_index.loaded = true;
            }
            Err(e) => {
                warn!("Failed to list templates: {e}");
                templates_index.names.clear();
                templates_index.loaded = true;
            }
        }
    }

    if !templates_index.is_changed() {
        return;
    }

    for row in &rows {
        commands.entity(row).despawn();
    }

    let Ok(content_entity) = content.single() else {
        return;
    };
    if templates_index.names.is_empty() {
        commands.entity(content_entity).with_children(|c| {
            c.spawn((
                Text::new("(no templates yet)"),
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
    } else {
        let names = templates_index.names.clone();
        commands.entity(content_entity).with_children(|c| {
            for name in names {
                c.spawn((
                    Button,
                    EditorTemplateRow { name: name.clone() },
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                        align_items: AlignItems::Center,
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.07, 0.06, 0.80)),
                    BorderColor::all(Color::srgb(0.20, 0.15, 0.10)),
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new(name),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.88, 0.84, 0.78)),
                    ));
                });
            }
        });
    }
}

/// Click handlers: row → load fragment + enter paste mode; refresh → mark
/// index stale so the next sync rereads the directory.
pub fn handle_templates_panel_clicks(
    rows: Query<(&EditorTemplateRow, &Interaction), (Changed<Interaction>, With<Button>)>,
    refresh: Query<&Interaction, (Changed<Interaction>, With<EditorTemplatesRefreshButton>)>,
    mut clipboard: ResMut<EditorClipboard>,
    mut editor_state: ResMut<EditorState>,
    mut templates_index: ResMut<EditorTemplatesIndex>,
) {
    for interaction in &refresh {
        if *interaction == Interaction::Pressed {
            templates_index.loaded = false;
        }
    }
    for (row, interaction) in &rows {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match load_template(&row.name) {
            Ok(fragment) => {
                clipboard.fragment = Some(fragment);
                editor_state.paste_state.active = true;
            }
            Err(e) => warn!("Failed to load template '{}': {e}", row.name),
        }
    }
}
