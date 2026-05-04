#![allow(clippy::type_complexity, clippy::too_many_arguments)]
pub mod modal;
pub mod palette;
pub mod panel_roots;
pub mod properties;
pub mod templates_panel;

pub use panel_roots::EditorPanelRoots;

use bevy::prelude::*;

use crate::editor::clipboard::fragment_from_state;
use crate::editor::resources::{
    EditorContext, EditorState, EditorTool, ModalKind, ModalState, ModalTextField,
};
use crate::editor::systems::{open_file_dialog_impl, open_new_map_dialog_impl, open_save_as_impl};
use crate::editor::ui::palette::spawn_palette_panel;
use crate::editor::ui::properties::spawn_properties_panel;
use crate::editor::ui::templates_panel::spawn_templates_panel;
use crate::player::components::Player;
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;

// ── Component markers ─────────────────────────────────────────────────────────

#[derive(Component)]
pub struct EditorHudRoot;
/// Top-bar Node root. Drag-pan hit-tests this so MMB-press in the empty space
/// between buttons doesn't start a pan.
#[derive(Component)]
pub struct EditorTopBarRoot;
#[derive(Component)]
pub struct EditorSaveButton;
#[derive(Component)]
pub struct EditorDirtyIndicator;
#[derive(Component)]
pub struct EditorOpenButton;
#[derive(Component)]
pub struct EditorSaveAsButton;
#[derive(Component)]
pub struct EditorNewMapButton;
#[derive(Component)]
pub struct EditorPortalToolButton;
#[derive(Component)]
pub struct EditorUndoButton;
#[derive(Component)]
pub struct EditorRedoButton;
#[derive(Component)]
pub struct EditorSelectToolButton;
#[derive(Component)]
pub struct EditorSaveAsTemplateButton;
#[derive(Component)]
pub struct EditorTemplatesToggleButton;

// ── Spawn HUD ─────────────────────────────────────────────────────────────────

pub fn spawn_editor_hud(
    mut commands: Commands,
    definitions: Res<OverworldObjectDefinitions>,
    floor_defs: Res<FloorTilesetDefinitions>,
    editor_context: Res<EditorContext>,
) {
    commands
        .spawn((
            EditorHudRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .with_children(|root| {
            // ── Top bar ───────────────────────────────────────────────────────
            root.spawn((
                EditorTopBarRoot,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(40.0),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
            ))
            .with_children(|bar| {
                // Map name
                bar.spawn((
                    Text::new(format!("Map Editor — {}", editor_context.authored_id)),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.84, 0.62)),
                ));
                bar.spawn((
                    EditorDirtyIndicator,
                    Text::new(""),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.6, 0.3)),
                ));

                // File buttons
                spawn_top_btn(bar, "Open  Ctrl+O", EditorOpenButton);
                spawn_top_btn(bar, "Save As…  Ctrl+⇧+S", EditorSaveAsButton);
                spawn_top_btn(bar, "New Map", EditorNewMapButton);

                // Undo / Redo
                spawn_top_btn(bar, "Undo  Ctrl+Z", EditorUndoButton);
                spawn_top_btn(bar, "Redo  Ctrl+Y", EditorRedoButton);

                // Selection / templates
                spawn_top_btn(bar, "Select  M", EditorSelectToolButton);
                spawn_top_btn(bar, "Save Selection as Template", EditorSaveAsTemplateButton);
                spawn_top_btn(bar, "Templates", EditorTemplatesToggleButton);

                // Spacer
                bar.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });

                // Portal tool toggle
                spawn_top_btn(bar, "Portal Tool", EditorPortalToolButton);

                // Save
                bar.spawn((
                    Button,
                    EditorSaveButton,
                    Node {
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.14, 0.10, 0.08, 0.96)),
                    BorderColor::all(Color::srgb(0.48, 0.36, 0.24)),
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Save  Ctrl+S"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.95, 0.91, 0.80)),
                    ));
                });
            });

            // ── Content row ───────────────────────────────────────────────────
            root.spawn((Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                ..default()
            },))
                .with_children(|row| {
                    spawn_palette_panel(row, &definitions, &floor_defs);
                    row.spawn((Node {
                        flex_grow: 1.0,
                        ..default()
                    },));
                    spawn_templates_panel(row);
                    spawn_properties_panel(row);
                });
        });
}

fn spawn_top_btn<M: Component>(parent: &mut ChildSpawnerCommands, label: &str, marker: M) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.12, 0.08, 0.06, 0.90)),
            BorderColor::all(Color::srgb(0.38, 0.28, 0.18)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.88, 0.84, 0.76)),
            ));
        });
}

// ── Cleanup ───────────────────────────────────────────────────────────────────

pub fn cleanup_editor_hud(mut commands: Commands, hud_query: Query<Entity, With<EditorHudRoot>>) {
    for entity in &hud_query {
        commands.entity(entity).despawn();
    }
}

// ── Top-bar sync + button handlers ───────────────────────────────────────────

pub fn sync_editor_top_bar(
    editor_state: Res<EditorState>,
    mut dirty_q: Query<&mut Text, With<EditorDirtyIndicator>>,
    mut save_btn: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<EditorSaveButton>,
    >,
    mut portal_btn: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (With<EditorPortalToolButton>, Without<EditorSaveButton>),
    >,
    mut undo_btn: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (
            With<EditorUndoButton>,
            Without<EditorSaveButton>,
            Without<EditorPortalToolButton>,
        ),
    >,
    mut redo_btn: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (
            With<EditorRedoButton>,
            Without<EditorSaveButton>,
            Without<EditorPortalToolButton>,
            Without<EditorUndoButton>,
        ),
    >,
    mut select_btn: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (
            With<EditorSelectToolButton>,
            Without<EditorSaveButton>,
            Without<EditorPortalToolButton>,
            Without<EditorUndoButton>,
            Without<EditorRedoButton>,
        ),
    >,
    mut templates_btn: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (
            With<EditorTemplatesToggleButton>,
            Without<EditorSaveButton>,
            Without<EditorPortalToolButton>,
            Without<EditorUndoButton>,
            Without<EditorRedoButton>,
            Without<EditorSelectToolButton>,
        ),
    >,
) {
    if let Ok(mut text) = dirty_q.single_mut() {
        text.0 = if editor_state.dirty {
            "[unsaved]".to_owned()
        } else {
            String::new()
        };
    }

    let is_portal = editor_state.current_tool == EditorTool::Portal;
    let is_select = editor_state.current_tool == EditorTool::Select;
    let is_templates = editor_state.templates_panel_visible;

    for (interaction, mut bg, mut border) in &mut save_btn {
        let (b, br) = btn_colors(*interaction, false);
        bg.0 = b;
        *border = BorderColor::all(br);
    }
    for (interaction, mut bg, mut border) in &mut portal_btn {
        let (b, br) = btn_colors(*interaction, is_portal);
        bg.0 = b;
        *border = BorderColor::all(br);
    }
    for (interaction, mut bg, mut border) in &mut undo_btn {
        let (b, br) = btn_colors(*interaction, false);
        bg.0 = b;
        *border = BorderColor::all(br);
    }
    for (interaction, mut bg, mut border) in &mut redo_btn {
        let (b, br) = btn_colors(*interaction, false);
        bg.0 = b;
        *border = BorderColor::all(br);
    }
    for (interaction, mut bg, mut border) in &mut select_btn {
        let (b, br) = btn_colors(*interaction, is_select);
        bg.0 = b;
        *border = BorderColor::all(br);
    }
    for (interaction, mut bg, mut border) in &mut templates_btn {
        let (b, br) = btn_colors(*interaction, is_templates);
        bg.0 = b;
        *border = BorderColor::all(br);
    }
}

fn btn_colors(interaction: Interaction, active: bool) -> (Color, Color) {
    match (interaction, active) {
        (Interaction::Pressed, _) => (Color::srgb(0.55, 0.30, 0.14), Color::srgb(1.0, 0.88, 0.60)),
        (Interaction::Hovered, _) => (Color::srgb(0.28, 0.17, 0.10), Color::srgb(0.90, 0.75, 0.50)),
        (Interaction::None, true) => (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50)),
        (Interaction::None, false) => (
            Color::srgba(0.12, 0.08, 0.06, 0.90),
            Color::srgb(0.38, 0.28, 0.18),
        ),
    }
}

// ── Button click handlers ─────────────────────────────────────────────────────

pub fn handle_save_button_click(
    save_btn: Query<&Interaction, (Changed<Interaction>, With<EditorSaveButton>)>,
    mut editor_state: ResMut<EditorState>,
    editor_context: Res<EditorContext>,
    portal_buffer: Res<crate::editor::resources::EditorPortalBuffer>,
    object_registry: Res<crate::world::object_registry::ObjectRegistry>,
    floor_maps: Res<crate::world::floor_map::FloorMaps>,
    objects: Query<(
        &crate::world::components::OverworldObject,
        &crate::world::components::SpaceResident,
        &crate::world::components::TilePosition,
    )>,
) {
    for interaction in &save_btn {
        if *interaction == Interaction::Pressed {
            crate::editor::serializer::serialize_and_save(
                &editor_context,
                &portal_buffer,
                &object_registry,
                &objects,
                &floor_maps,
            );
            editor_state.dirty = false;
        }
    }
}

pub fn handle_open_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorOpenButton>)>,
    editor_context: Res<EditorContext>,
    mut modal_state: ResMut<ModalState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed && modal_state.active.is_none() {
            open_file_dialog_impl(&editor_context, &mut modal_state);
        }
    }
}

pub fn handle_save_as_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorSaveAsButton>)>,
    editor_context: Res<EditorContext>,
    mut modal_state: ResMut<ModalState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed && modal_state.active.is_none() {
            open_save_as_impl(&editor_context, &mut modal_state);
        }
    }
}

pub fn handle_new_map_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorNewMapButton>)>,
    mut modal_state: ResMut<ModalState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed && modal_state.active.is_none() {
            open_new_map_dialog_impl(&mut modal_state);
        }
    }
}

pub fn handle_portal_tool_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorPortalToolButton>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed {
            editor_state.current_tool = if editor_state.current_tool == EditorTool::Portal {
                EditorTool::Brush
            } else {
                EditorTool::Portal
            };
        }
    }
}

pub fn handle_undo_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorUndoButton>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed {
            editor_state.undo_requested = true;
        }
    }
}

pub fn handle_redo_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorRedoButton>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed {
            editor_state.redo_requested = true;
        }
    }
}

pub fn handle_select_tool_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorSelectToolButton>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed {
            editor_state.current_tool = if editor_state.current_tool == EditorTool::Select {
                EditorTool::Brush
            } else {
                EditorTool::Select
            };
        }
    }
}

pub fn handle_templates_toggle_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorTemplatesToggleButton>)>,
    mut editor_state: ResMut<EditorState>,
) {
    for interaction in &btn {
        if *interaction == Interaction::Pressed {
            editor_state.templates_panel_visible = !editor_state.templates_panel_visible;
        }
    }
}

pub fn handle_save_as_template_button_click(
    btn: Query<&Interaction, (Changed<Interaction>, With<EditorSaveAsTemplateButton>)>,
    editor_state: Res<EditorState>,
    editor_context: Res<EditorContext>,
    object_registry: Res<ObjectRegistry>,
    floor_maps: Res<FloorMaps>,
    objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut modal_state: ResMut<ModalState>,
) {
    for interaction in &btn {
        if *interaction != Interaction::Pressed || modal_state.active.is_some() {
            continue;
        }
        let Some(fragment) = fragment_from_state(
            &editor_state,
            &editor_context,
            &object_registry,
            &floor_maps,
            &objects,
        ) else {
            warn!("Save Selection as Template: empty selection");
            continue;
        };
        modal_state.active = Some(ModalKind::SaveAsTemplate);
        modal_state.text_fields = vec![ModalTextField {
            label: "Template name".into(),
            value: String::new(),
            placeholder: "shrub_cluster".into(),
            numeric_only: false,
        }];
        modal_state.focused_field = 0;
        modal_state.error_message = None;
        modal_state.confirm_triggered = false;
        modal_state.confirmed = None;
        modal_state.pending_template_fragment = Some(fragment);
    }
}
