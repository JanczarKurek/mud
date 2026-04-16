use bevy::prelude::*;

use crate::asset_viewer::systems::{InspectorBody, InspectorTitle};

pub fn spawn_inspector_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((Node {
            width: Val::Px(280.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::left(Val::Px(1.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
        BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
        ))
        .with_children(|panel| {
            // Header
            panel.spawn((
                InspectorTitle,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
            ))
            .with_children(|h| {
                h.spawn((
                    Text::new("—"),
                    TextFont { font_size: 12.0, ..default() },
                    TextColor(Color::srgb(0.96, 0.84, 0.62)),
                ));
            });

            // Field rows (populated dynamically by sync_inspector_panel)
            panel.spawn((
                InspectorBody,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip_y(),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}
