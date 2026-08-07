use bevy::prelude::*;

use crate::asset_viewer::resources::{AssetKind, ViewerState};
use crate::asset_viewer::systems::{ViewerFilterBox, ViewerPaletteItem, ViewerTab};
use crate::editor::ui::palette::{
    spawn_filter_row, spawn_palette_row, PaletteHost, PaletteRowStyle,
};
use crate::editor::ui::style::{PANEL_BG, PANEL_BORDER};
use crate::magic::resources::SpellDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// The viewer's list rows are slightly denser than the editor's: smaller
/// swatch, smaller font, and a growing label ("name (id)").
const VIEWER_ROW_STYLE: PaletteRowStyle = PaletteRowStyle {
    swatch_px: 10.0,
    swatch_border: false,
    row_pad_y: 5.0,
    label_font_size: 10.0,
    label_flex_grow: 1.0,
};

pub fn spawn_palette_panel(
    parent: &mut ChildSpawnerCommands,
    object_defs: &OverworldObjectDefinitions,
    spell_defs: &SpellDefinitions,
) {
    parent
        .spawn((
            Node {
                width: Val::Px(220.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|panel| {
            // Tab row
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        border: UiRect::bottom(Val::Px(1.0)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_children(|tabs| {
                    spawn_tab(tabs, "Objects", AssetKind::Object, true);
                    spawn_tab(tabs, "Spells", AssetKind::Spell, false);
                });

            // Filter box (shared with the editor palette; flex_shrink 0 keeps
            // it pinned while the list below scrolls).
            spawn_filter_row(panel, ViewerFilterBox, ViewerState::FILTER_PLACEHOLDER, 0.0);

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
                        let Some(def) = object_defs.get(id) else {
                            continue;
                        };
                        spawn_item(list, id, &def.name, def.debug_color(), AssetKind::Object);
                    }

                    let mut spell_ids: Vec<&str> = spell_defs.ids().collect();
                    spell_ids.sort();
                    for id in spell_ids {
                        let Some(def) = spell_defs.get(id) else {
                            continue;
                        };
                        spawn_item(
                            list,
                            id,
                            &def.name,
                            Color::srgb(0.4, 0.6, 1.0),
                            AssetKind::Spell,
                        );
                    }
                });
        });
}

fn spawn_tab(parent: &mut ChildSpawnerCommands, label: &str, kind: AssetKind, active: bool) {
    parent
        .spawn((
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
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
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
    spawn_palette_row(
        parent,
        ViewerPaletteItem {
            id: id.to_owned(),
            display_name: name.to_owned(),
            kind,
        },
        &format!("{} ({})", name, id),
        color,
        VIEWER_ROW_STYLE,
    );
}
