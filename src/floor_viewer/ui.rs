use bevy::prelude::*;

use crate::floor_viewer::plugin::{ActiveFloor, ShowGrid, ViewMode, ViewModeKind};
use crate::world::floor_definitions::{FloorTilesetDefinitions, FloorTypeId};

#[derive(Component)]
pub struct PaletteRoot;

#[derive(Component)]
pub struct PaletteSwatch {
    pub id: FloorTypeId,
}

#[derive(Component)]
pub struct StatusText;

#[derive(Resource)]
pub struct PaletteDirty(pub bool);

impl Default for PaletteDirty {
    fn default() -> Self {
        Self(true)
    }
}

pub fn spawn_palette_ui() {
    // Initial spawn happens via sync_palette_panel on the first Update tick
    // (PaletteDirty starts true). Keeping startup minimal avoids duplicating
    // the panel-build code in two places.
}

pub fn sync_palette_panel(
    mut commands: Commands,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut palette_dirty: ResMut<PaletteDirty>,
    existing: Query<Entity, With<PaletteRoot>>,
) {
    if !palette_dirty.0 {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let mut defs: Vec<_> = floor_defs.iter().collect();
    defs.sort_by(|a, b| a.id.cmp(&b.id));

    commands
        .spawn((
            PaletteRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(10.0),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|row| {
                for (i, def) in defs.iter().enumerate() {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(4.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|item| {
                        item.spawn((
                            PaletteSwatch { id: def.id.clone() },
                            Node {
                                width: Val::Px(28.0),
                                height: Val::Px(28.0),
                                border: UiRect::all(Val::Px(2.0)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(def.debug_color()),
                            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                        ))
                        .with_children(|sw| {
                            let label = if i < 9 {
                                format!("{}", i + 1)
                            } else {
                                "·".to_owned()
                            };
                            sw.spawn((
                                Text::new(label),
                                TextFont {
                                    font_size: 12.0,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                            ));
                        });

                        item.spawn((
                            Text::new(def.name.clone()),
                            TextFont {
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.9, 0.9, 0.9)),
                        ));
                    });
                }
            });

            root.spawn((
                StatusText,
                Text::new(""),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.95, 0.85, 0.5)),
            ));

            root.spawn((
                Text::new(
                    "LMB paint  ·  RMB erase  ·  1-9 select  ·  0 none  ·  T toggle view  ·  G toggle grid  ·  R reload",
                ),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.7, 0.7, 0.7)),
            ));
        });

    palette_dirty.0 = false;
}

pub fn sync_status_text(
    view: Res<ViewMode>,
    grid: Res<ShowGrid>,
    active: Res<ActiveFloor>,
    mut query: Query<&mut Text, With<StatusText>>,
) {
    let mode = match view.0 {
        ViewModeKind::Tiled => "Tiled",
        ViewModeKind::Debug => "Debug",
    };
    let g = if grid.0 { "on" } else { "off" };
    let af = active.0.as_deref().unwrap_or("(none)");
    for mut text in &mut query {
        *text = Text::new(format!("Mode: {mode}  ·  Grid: {g}  ·  Active: {af}"));
    }
}

pub fn sync_palette_highlight(
    active: Res<ActiveFloor>,
    mut query: Query<(&PaletteSwatch, &mut BorderColor)>,
) {
    for (swatch, mut border) in &mut query {
        let is_active = active.0.as_ref() == Some(&swatch.id);
        let color = if is_active {
            Color::srgb(1.0, 0.85, 0.25)
        } else {
            Color::srgba(0.0, 0.0, 0.0, 0.0)
        };
        *border = BorderColor::all(color);
    }
}
