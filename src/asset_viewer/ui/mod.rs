pub mod inspector;
pub mod palette;

use bevy::prelude::*;

use crate::asset_viewer::systems::{ClipButtonContainer, TopBarTitle, ViewerSaveButton};
use crate::asset_viewer::ui::inspector::spawn_inspector_panel;
use crate::asset_viewer::ui::palette::spawn_palette_panel;
use crate::magic::resources::SpellDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Component)]
pub struct ViewerHudRoot;

pub fn spawn_viewer_hud(
    mut commands: Commands,
    object_defs: Res<OverworldObjectDefinitions>,
    spell_defs: Res<SpellDefinitions>,
) {
    commands
        .spawn((
            ViewerHudRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .with_children(|root| {
            // ── Top bar ───────────────────────────────────────────────────
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(38.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.94)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    TopBarTitle,
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ))
                .with_children(|t| {
                    t.spawn((
                        Text::new("Asset Viewer"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                    ));
                });

                // Save button
                bar.spawn((
                    Button,
                    ViewerSaveButton,
                    Node {
                        padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.07, 0.06, 0.70)),
                    BorderColor::all(Color::srgb(0.22, 0.16, 0.12)),
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Save"),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.88, 0.84, 0.78)),
                    ));
                });
            });

            // ── Main row ──────────────────────────────────────────────────
            root.spawn((Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                overflow: Overflow::clip(),
                ..default()
            },))
                .with_children(|row| {
                    // Left sidebar — palette
                    spawn_palette_panel(row, &object_defs, &spell_defs);

                    // Center — transparent (world renders here) + clip buttons
                    row.spawn((Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::FlexEnd,
                        padding: UiRect::bottom(Val::Px(10.0)),
                        ..default()
                    },))
                        .with_children(|center| {
                            // Clip button strip (populated dynamically by sync_clip_buttons)
                            center.spawn((
                                ClipButtonContainer,
                                Node {
                                    flex_direction: FlexDirection::Row,
                                    column_gap: Val::Px(6.0),
                                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                                    ..default()
                                },
                            ));
                        });

                    // Right sidebar — inspector
                    spawn_inspector_panel(row);
                });
        });
}

pub fn cleanup_viewer_hud(mut commands: Commands, query: Query<Entity, With<ViewerHudRoot>>) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}
