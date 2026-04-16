pub mod palette;
pub mod properties;

use bevy::prelude::*;

use crate::editor::resources::{EditorContext, EditorState};
use crate::editor::ui::palette::spawn_palette_panel;
use crate::editor::ui::properties::spawn_properties_panel;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Root marker for all editor HUD entities; despawned on exit.
#[derive(Component)]
pub struct EditorHudRoot;

/// Top-bar save button marker.
#[derive(Component)]
pub struct EditorSaveButton;

/// Top-bar "dirty" indicator text marker.
#[derive(Component)]
pub struct EditorDirtyIndicator;

/// Spawns the editor HUD: top bar + left palette sidebar + right properties sidebar.
pub fn spawn_editor_hud(
    mut commands: Commands,
    definitions: Res<OverworldObjectDefinitions>,
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
            // Top bar
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(40.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(12.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Text::new(format!("Map Editor — {}", editor_context.authored_id)),
                    TextFont { font_size: 16.0, ..default() },
                    TextColor(Color::srgb(0.96, 0.84, 0.62)),
                ));

                bar.spawn((
                    EditorDirtyIndicator,
                    Text::new(""),
                    TextFont { font_size: 14.0, ..default() },
                    TextColor(Color::srgb(1.0, 0.6, 0.3)),
                ));

                // Spacer
                bar.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });

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
                        TextFont { font_size: 14.0, ..default() },
                        TextColor(Color::srgb(0.95, 0.91, 0.80)),
                    ));
                });
            });

            // Content row: palette | world view | properties
            root.spawn((Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                ..default()
            },))
            .with_children(|row| {
                spawn_palette_panel(row, &definitions);

                // Center — transparent, world renders behind it
                row.spawn((Node {
                    flex_grow: 1.0,
                    ..default()
                },));

                spawn_properties_panel(row);
            });
        });
}

/// Despawn all editor HUD entities.
pub fn cleanup_editor_hud(
    mut commands: Commands,
    hud_query: Query<Entity, With<EditorHudRoot>>,
) {
    for entity in &hud_query {
        commands.entity(entity).despawn();
    }
}

/// Sync the save button hover style and dirty indicator.
pub fn sync_editor_top_bar(
    editor_state: Res<EditorState>,
    mut dirty_query: Query<&mut Text, With<EditorDirtyIndicator>>,
    mut save_btn_query: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<EditorSaveButton>,
    >,
) {
    if let Ok(mut text) = dirty_query.single_mut() {
        text.0 = if editor_state.dirty {
            "[unsaved changes]".to_owned()
        } else {
            String::new()
        };
    }

    for (interaction, mut bg, mut border) in &mut save_btn_query {
        let (bg_color, border_color) = match *interaction {
            Interaction::Pressed => (Color::srgb(0.55, 0.30, 0.14), Color::srgb(1.0, 0.88, 0.60)),
            Interaction::Hovered => (Color::srgb(0.28, 0.17, 0.10), Color::srgb(0.90, 0.75, 0.50)),
            Interaction::None => (Color::srgba(0.14, 0.10, 0.08, 0.96), Color::srgb(0.48, 0.36, 0.24)),
        };
        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }
}

/// Save button click handler (alternative to Ctrl+S).
pub fn handle_save_button_click(
    save_btn: Query<&Interaction, (Changed<Interaction>, With<EditorSaveButton>)>,
    mut editor_state: ResMut<EditorState>,
    editor_context: Res<EditorContext>,
    space_definitions: Res<crate::world::map_layout::SpaceDefinitions>,
    object_registry: Res<crate::world::object_registry::ObjectRegistry>,
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
                &space_definitions,
                &object_registry,
                &objects,
            );
            editor_state.dirty = false;
        }
    }
}
