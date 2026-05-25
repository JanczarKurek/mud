//! Side panel for the building-draw tool.
//!
//! Visibility is driven directly by `editor_state.current_tool ==
//! EditorTool::BuildingDraw` — the panel exists *because* the tool is active,
//! not via a separate `panel_visible` flag. Contents: a preset row list, a
//! floor-override row list, and a "Place door (click a wall)" toggle.
//! Selecting a preset / floor row writes into `editor_state.building`; the
//! drag handler (`crate::editor::building`) and door-swap branch in
//! `handle_editor_left_click` read from there.

use bevy::prelude::*;

use crate::editor::resources::{EditorState, EditorTool};
use crate::world::building_presets::BuildingPresets;
use crate::world::floor_definitions::FloorTilesetDefinitions;

/// Marker for the building panel root node — registered in `panel_roots.rs`
/// so the drag handler's `cursor_over` check treats panel clicks as chrome.
#[derive(Component)]
pub struct EditorBuildingRoot;

#[derive(Component)]
pub struct EditorBuildingContent;

#[derive(Component, Clone)]
pub struct EditorBuildingPresetRow {
    pub preset_id: String,
}

/// `None` = "use preset default". Authored as a sentinel row at the top of
/// the floor list so the user can revert an override without remembering the
/// preset's choice.
#[derive(Component, Clone)]
pub struct EditorBuildingFloorRow {
    pub floor_id: Option<String>,
}

#[derive(Component)]
pub struct EditorBuildingDoorArmButton;

/// What `sync_building_panel` last rendered. Stored in a `Local` so the
/// system can short-circuit when nothing user-visible has changed.
/// `content_entity` is part of the key so a fresh content node (e.g. after
/// editor re-entry) forces a rebuild even if the selection state is the
/// same as before.
#[derive(PartialEq, Eq)]
pub struct BuildingPanelSnapshot {
    content_entity: Entity,
    selected_preset_id: Option<String>,
    floor_override: Option<String>,
    door_armed: bool,
}

const PANEL_WIDTH_PX: f32 = 200.0;

pub fn spawn_building_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorBuildingRoot,
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
            // Header.
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
                        Text::new("Building"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                    ));
                });

            // Static hint.
            panel
                .spawn((Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    ..default()
                },))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Drag to draw. Click a wall after to place a door."),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.70, 0.66, 0.60)),
                    ));
                });

            // Scrollable content — preset rows, then floor override rows, then door-arm button.
            panel.spawn((
                EditorBuildingContent,
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

/// Show the panel only while the building tool is active.
pub fn sync_building_panel_visibility(
    editor_state: Res<EditorState>,
    mut roots: Query<&mut Node, With<EditorBuildingRoot>>,
) {
    if !editor_state.is_changed() {
        return;
    }
    let target = if editor_state.current_tool == EditorTool::BuildingDraw {
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

/// Build the preset/floor/door rows when the panel becomes visible. Rebuilt
/// in place only when the relevant `editor_state.building` fields actually
/// change — `editor_state` itself ticks `is_changed` almost every frame
/// (many editor systems write to it), so naively reacting to that caused
/// section headers to stack ("Preset / Floor override / Door" repeated)
/// and the row entities to be respawned mid-click, swallowing the
/// `Changed<Interaction>` Pressed transitions.
pub fn sync_building_panel(
    editor_state: Res<EditorState>,
    presets: Res<BuildingPresets>,
    floor_defs: Res<FloorTilesetDefinitions>,
    content: Query<(Entity, Option<&Children>), With<EditorBuildingContent>>,
    mut commands: Commands,
    mut last: Local<Option<BuildingPanelSnapshot>>,
) {
    if editor_state.current_tool != EditorTool::BuildingDraw {
        return;
    }
    let Ok((content_entity, children)) = content.single() else {
        return;
    };
    let want = BuildingPanelSnapshot {
        content_entity,
        selected_preset_id: editor_state.building.selected_preset_id.clone(),
        floor_override: editor_state.building.floor_override.clone(),
        door_armed: editor_state.building.place_door_armed,
    };
    if last.as_ref() == Some(&want) && !presets.is_changed() {
        return;
    }
    *last = Some(want);

    // Despawn *all* children of the content node — not just marker-tagged
    // rows. Section headers and the "(no presets)" empty note carry no
    // marker, so a marker-only sweep would let them accumulate on every
    // rebuild.
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }

    let active_preset = editor_state.building.selected_preset_id.clone();
    let active_floor_override = editor_state.building.floor_override.clone();
    let door_armed = editor_state.building.place_door_armed;
    let preset_list: Vec<(String, String)> = presets
        .iter()
        .map(|(id, def)| (id.clone(), def.name.clone()))
        .collect();
    let floor_list: Vec<(String, String)> = floor_defs
        .iter()
        .map(|def| (def.id.clone(), def.name.clone()))
        .collect();

    commands.entity(content_entity).with_children(|c| {
        section_header(c, "Preset");
        if preset_list.is_empty() {
            empty_note(c, "(no presets)");
        } else {
            for (id, name) in &preset_list {
                let is_active = active_preset.as_deref() == Some(id.as_str());
                spawn_row(
                    c,
                    name,
                    is_active,
                    EditorBuildingPresetRow {
                        preset_id: id.clone(),
                    },
                );
            }
        }

        section_header(c, "Floor override");
        spawn_row(
            c,
            "Use preset default",
            active_floor_override.is_none(),
            EditorBuildingFloorRow { floor_id: None },
        );
        for (id, name) in &floor_list {
            let is_active = active_floor_override.as_deref() == Some(id.as_str());
            spawn_row(
                c,
                name,
                is_active,
                EditorBuildingFloorRow {
                    floor_id: Some(id.clone()),
                },
            );
        }

        section_header(c, "Door");
        c.spawn((
            Button,
            EditorBuildingDoorArmButton,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(row_bg(door_armed)),
            BorderColor::all(row_border(door_armed)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(if door_armed {
                    "ARMED — click a wall"
                } else {
                    "Place door (click a wall)"
                }),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.86, 0.72)),
            ));
        });
    });
}

fn section_header(c: &mut ChildSpawnerCommands, label: &str) {
    c.spawn((Node {
        padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(8.0), Val::Px(2.0)),
        ..default()
    },))
        .with_children(|h| {
            h.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.60, 0.54, 0.46)),
            ));
        });
}

fn empty_note(c: &mut ChildSpawnerCommands, label: &str) {
    c.spawn((Node {
        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
        ..default()
    },))
        .with_children(|h| {
            h.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.50, 0.46, 0.42)),
            ));
        });
}

fn spawn_row<M: Component>(c: &mut ChildSpawnerCommands, label: &str, active: bool, marker: M) {
    c.spawn((
        Button,
        marker,
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            align_items: AlignItems::Center,
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(row_bg(active)),
        BorderColor::all(row_border(active)),
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(label.to_owned()),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(Color::srgb(0.88, 0.84, 0.78)),
        ));
    });
}

fn row_bg(active: bool) -> Color {
    if active {
        Color::srgb(0.28, 0.16, 0.08)
    } else {
        Color::srgba(0.10, 0.07, 0.06, 0.80)
    }
}

fn row_border(active: bool) -> Color {
    if active {
        Color::srgb(0.90, 0.76, 0.50)
    } else {
        Color::srgb(0.20, 0.15, 0.10)
    }
}

/// Click handler for preset rows, floor-override rows, and the door-arm
/// button. All edits go through `editor_state.building`.
pub fn handle_building_panel_clicks(
    preset_rows: Query<(&EditorBuildingPresetRow, &Interaction), Changed<Interaction>>,
    floor_rows: Query<(&EditorBuildingFloorRow, &Interaction), Changed<Interaction>>,
    door_btn: Query<&Interaction, (Changed<Interaction>, With<EditorBuildingDoorArmButton>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for (row, interaction) in &preset_rows {
        if *interaction == Interaction::Pressed {
            editor_state.building.selected_preset_id = Some(row.preset_id.clone());
        }
    }
    for (row, interaction) in &floor_rows {
        if *interaction == Interaction::Pressed {
            editor_state.building.floor_override = row.floor_id.clone();
        }
    }
    for interaction in &door_btn {
        if *interaction == Interaction::Pressed {
            editor_state.building.place_door_armed = !editor_state.building.place_door_armed;
        }
    }
}
