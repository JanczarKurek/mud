use bevy::prelude::*;

use crate::asset_viewer::resources::AssetKind;
use crate::asset_viewer::systems::{ViewerFilterBox, ViewerPaletteItem, ViewerTab};
use crate::magic::resources::SpellDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;

pub fn spawn_palette_panel(
    parent: &mut ChildSpawnerCommands,
    object_defs: &OverworldObjectDefinitions,
    spell_defs: &SpellDefinitions,
) {
    parent
        .spawn((Node {
            width: Val::Px(220.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::right(Val::Px(1.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
        BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
        ))
        .with_children(|panel| {
            // Tab row
            panel.spawn((Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                border: UiRect::bottom(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
            ))
            .with_children(|tabs| {
                spawn_tab(tabs, "Objects", AssetKind::Object, true);
                spawn_tab(tabs, "Spells", AssetKind::Spell, false);
            });

            // Filter box
            panel.spawn((
                ViewerFilterBox,
                Button,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.05, 0.05, 0.90)),
                BorderColor::all(Color::srgb(0.25, 0.18, 0.12)),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new("filter…"),
                    TextFont { font_size: 11.0, ..default() },
                    TextColor(Color::srgb(0.50, 0.46, 0.42)),
                ));
            });

            // Scrollable item list
            panel
                .spawn((Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip_y(),
                    flex_grow: 1.0,
                    ..default()
                },))
                .with_children(|list| {
                    let mut object_ids: Vec<&str> = object_defs.ids().collect();
                    object_ids.sort();
                    for id in object_ids {
                        let Some(def) = object_defs.get(id) else { continue };
                        spawn_item(list, id, &def.name, def.debug_color(), AssetKind::Object);
                    }

                    let mut spell_ids: Vec<&str> = spell_defs.ids().collect();
                    spell_ids.sort();
                    for id in spell_ids {
                        let Some(def) = spell_defs.get(id) else { continue };
                        spawn_item(list, id, &def.name, Color::srgb(0.4, 0.6, 1.0), AssetKind::Spell);
                    }
                });
        });
}

fn spawn_tab(parent: &mut ChildSpawnerCommands, label: &str, kind: AssetKind, active: bool) {
    parent.spawn((
        Button,
        ViewerTab { kind },
        Node {
            flex_grow: 1.0,
            padding: UiRect::axes(Val::Px(8.0), Val::Px(7.0)),
            justify_content: JustifyContent::Center,
            border: UiRect::right(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(if active {
            Color::srgb(0.28, 0.16, 0.08)
        } else {
            Color::srgba(0.08, 0.05, 0.05, 0.80)
        }),
        BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(label),
            TextFont { font_size: 11.0, ..default() },
            TextColor(Color::srgb(0.88, 0.84, 0.78)),
        ));
    });
}

fn spawn_item(
    parent: &mut ChildSpawnerCommands,
    id: &str,
    name: &str,
    color: Color,
    kind: AssetKind,
) {
    parent
        .spawn((
            Button,
            ViewerPaletteItem {
                id: id.to_owned(),
                display_name: name.to_owned(),
                kind,
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
                    width: Val::Px(10.0),
                    height: Val::Px(10.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(color),
            ));
            btn.spawn((
                Text::new(format!("{} ({})", name, id)),
                TextFont { font_size: 10.0, ..default() },
                TextColor(Color::srgb(0.88, 0.84, 0.78)),
                Node { overflow: Overflow::clip_x(), flex_grow: 1.0, ..default() },
            ));
        });
}
