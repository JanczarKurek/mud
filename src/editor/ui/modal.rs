#![allow(clippy::type_complexity)]
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::editor::resources::{
    ModalKind, ModalState, PickRectTarget, SpawnAreaKind, SpawnGroupField, BehaviorKind,
};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};

// ── Component markers ─────────────────────────────────────────────────────────

#[derive(Component)]
pub struct ModalOverlayRoot;

#[derive(Component)]
pub struct ModalTextField {
    pub index: usize,
}

#[derive(Component)]
pub struct ModalListItem {
    pub index: usize,
}

#[derive(Component)]
pub struct ModalConfirmButton;

#[derive(Component)]
pub struct ModalCancelButton;

#[derive(Component)]
pub struct ModalErrorText;

#[derive(Component, Clone, Copy)]
pub struct SpawnGroupFieldButton {
    pub field: SpawnGroupField,
}

#[derive(Component, Clone, Copy)]
pub struct SpawnGroupAreaKindButton {
    pub kind: SpawnAreaKind,
}

#[derive(Component, Clone, Copy)]
pub struct SpawnGroupBehaviorKindButton {
    pub kind: BehaviorKind,
}

#[derive(Component, Clone, Copy)]
pub struct SpawnGroupPickRectButton {
    pub target: PickRectTarget,
}

fn title_for(kind: ModalKind) -> &'static str {
    match kind {
        ModalKind::FileOpen => "Open Map",
        ModalKind::SaveAs => "Save Map As",
        ModalKind::NewMap => "New Map",
        ModalKind::PortalCreate => "Add Portal",
        ModalKind::SaveAsTemplate => "Save Selection as Template",
        ModalKind::SpawnGroupEdit { editing_index } => {
            if editing_index.is_some() {
                "Edit Spawn Group"
            } else {
                "Add Spawn Group"
            }
        }
    }
}

fn confirm_label_for(kind: ModalKind) -> &'static str {
    match kind {
        ModalKind::FileOpen => "Open",
        ModalKind::SaveAs => "Save",
        ModalKind::NewMap => "Create",
        ModalKind::PortalCreate => "Add",
        ModalKind::SaveAsTemplate => "Save",
        ModalKind::SpawnGroupEdit { .. } => "Save",
    }
}

// ── Spawn / rebuild ───────────────────────────────────────────────────────────

/// Rebuild the modal overlay whenever ModalState changes.
pub fn spawn_or_rebuild_modal(
    modal_state: Res<ModalState>,
    existing: Query<Entity, With<ModalOverlayRoot>>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    mut commands: Commands,
) {
    if !modal_state.is_changed() {
        return;
    }

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let Some(kind) = modal_state.active else {
        return;
    };

    let theme = theme.clone();
    let palette = *palette;
    let is_list = kind == ModalKind::FileOpen;
    if matches!(kind, ModalKind::SpawnGroupEdit { .. }) {
        spawn_spawn_group_modal(kind, &modal_state, &theme, palette, &mut commands);
        return;
    }

    commands
        .spawn((
            ModalOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(palette.surface_overlay_strong),
            // Consume all interaction so nothing behind the modal is clickable.
            Button,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    ThemedPanel,
                    Node {
                        width: Val::Px(380.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(18.0)),
                        row_gap: Val::Px(10.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    ImageNode::new(theme.panel_frame.clone())
                        .with_mode(theme.panel_image_mode())
                        .with_color(palette.surface_panel),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(palette.border_idle),
                ))
                .with_children(|card| {
                    // Title
                    card.spawn((
                        Text::new(title_for(kind)),
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(palette.text_accent),
                    ));

                    // Error / info message
                    let err_text = modal_state.error_message.clone().unwrap_or_default();
                    let err_visible = if err_text.is_empty() {
                        Visibility::Hidden
                    } else {
                        Visibility::Visible
                    };
                    card.spawn((
                        ModalErrorText,
                        Text::new(err_text),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(palette.text_danger),
                        err_visible,
                    ));

                    // Text fields
                    for (i, field) in modal_state.text_fields.iter().enumerate() {
                        let is_focused = modal_state.focused_field == i;
                        let display_value = if field.value.is_empty() && !is_focused {
                            None
                        } else if is_focused {
                            Some(format!("{}_", field.value))
                        } else {
                            Some(field.value.clone())
                        };

                        card.spawn((Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(3.0),
                            ..default()
                        },))
                            .with_children(|field_col| {
                                field_col.spawn((
                                    Text::new(field.label.clone()),
                                    TextFont {
                                        font_size: 12.0,
                                        ..default()
                                    },
                                    TextColor(palette.text_muted),
                                ));

                                field_col
                                    .spawn((
                                        ModalTextField { index: i },
                                        Node {
                                            width: Val::Percent(100.0),
                                            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                                            border: UiRect::all(Val::Px(1.0)),
                                            ..default()
                                        },
                                        BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.90)),
                                        BorderColor::all(if is_focused {
                                            palette.border_focus
                                        } else {
                                            palette.border_idle
                                        }),
                                    ))
                                    .with_children(|input| {
                                        if let Some(val) = display_value {
                                            input.spawn((
                                                Text::new(val),
                                                TextFont {
                                                    font_size: 13.0,
                                                    ..default()
                                                },
                                                TextColor(palette.text_value),
                                            ));
                                        } else {
                                            input.spawn((
                                                Text::new(field.placeholder.clone()),
                                                TextFont {
                                                    font_size: 13.0,
                                                    ..default()
                                                },
                                                TextColor(palette.text_placeholder),
                                            ));
                                        }
                                    });
                            });
                    }

                    // File list (FileOpen only)
                    if is_list {
                        card.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                max_height: Val::Px(240.0),
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::clip_y(),
                                border: UiRect::all(Val::Px(1.0)),
                                ..default()
                            },
                            BorderColor::all(palette.border_idle),
                            BackgroundColor(Color::srgba(0.05, 0.03, 0.03, 0.90)),
                        ))
                        .with_children(|list| {
                            for (i, name) in modal_state.list_items.iter().enumerate() {
                                let selected = modal_state.selected_list_item == Some(i);
                                let (bg, border, _) =
                                    idle_colors(&palette, ButtonStyle::Slot, selected);
                                list.spawn((
                                    Button,
                                    ThemedButton {
                                        style: ButtonStyle::Slot,
                                        selected,
                                    },
                                    ModalListItem { index: i },
                                    Node {
                                        width: Val::Percent(100.0),
                                        padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                                        border: UiRect::bottom(Val::Px(1.0)),
                                        ..default()
                                    },
                                    ImageNode::new(theme.button_frame.clone())
                                        .with_mode(theme.button_image_mode())
                                        .with_color(bg),
                                    BackgroundColor(Color::NONE),
                                    BorderColor::all(border),
                                ))
                                .with_children(|btn| {
                                    btn.spawn((
                                        Text::new(name.clone()),
                                        TextFont {
                                            font_size: 13.0,
                                            ..default()
                                        },
                                        TextColor(if selected {
                                            palette.text_accent
                                        } else {
                                            palette.text_value
                                        }),
                                    ));
                                });
                            }
                        });
                    }

                    // Button row
                    card.spawn((Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: Val::Px(8.0),
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    },))
                        .with_children(|row| {
                            spawn_modal_button(
                                row,
                                &theme,
                                &palette,
                                ButtonStyle::Secondary,
                                "Cancel",
                                ModalCancelButton,
                            );
                            spawn_modal_button(
                                row,
                                &theme,
                                &palette,
                                ButtonStyle::Primary,
                                confirm_label_for(kind),
                                ModalConfirmButton,
                            );
                        });
                });
        });
}

fn spawn_modal_button<T: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    style: ButtonStyle,
    label: &str,
    marker: T,
) {
    let (bg, border, text) = idle_colors(palette, style, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(style),
            marker,
            Node {
                padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            ImageNode::new(theme.button_frame.clone())
                .with_mode(theme.button_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

// ── Keyboard input ────────────────────────────────────────────────────────────

pub fn handle_modal_keyboard_input(
    mut keyboard_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    mut modal_state: ResMut<ModalState>,
) {
    let is_spawn_group = matches!(modal_state.active, Some(ModalKind::SpawnGroupEdit { .. }));
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        match event.key_code {
            KeyCode::Escape => {
                modal_state.active = None;
                modal_state.error_message = None;
            }
            KeyCode::Enter => {
                modal_state.confirm_triggered = true;
            }
            KeyCode::Tab => {
                if is_spawn_group {
                    if let Some(draft) = modal_state.spawn_group_draft.as_mut() {
                        draft.focused_field = next_spawn_group_field(draft.focused_field, draft.behavior_kind);
                    }
                } else {
                    let len = modal_state.text_fields.len();
                    if len > 0 {
                        modal_state.focused_field = (modal_state.focused_field + 1) % len;
                    }
                }
            }
            KeyCode::Backspace => {
                if is_spawn_group {
                    if let Some(draft) = modal_state.spawn_group_draft.as_mut() {
                        let f = draft.focused_field;
                        if let Some(s) = draft.field_mut(f) {
                            s.pop();
                        }
                    }
                } else {
                    let idx = modal_state.focused_field;
                    if let Some(field) = modal_state.text_fields.get_mut(idx) {
                        field.value.pop();
                    }
                }
            }
            _ => {
                if event.repeat {
                    continue;
                }
                let ch_str = match &event.logical_key {
                    Key::Character(c) => Some(c.as_str().to_owned()),
                    Key::Space => Some(" ".to_owned()),
                    _ => None,
                };
                if let Some(ch) = ch_str {
                    if is_spawn_group {
                        if let Some(draft) = modal_state.spawn_group_draft.as_mut() {
                            let f = draft.focused_field;
                            let numeric = crate::editor::resources::SpawnGroupDraft::is_field_numeric(f);
                            let allow = !numeric
                                || ch.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-');
                            if allow {
                                if let Some(s) = draft.field_mut(f) {
                                    s.push_str(&ch);
                                }
                            }
                        }
                    } else {
                        let idx = modal_state.focused_field;
                        if let Some(field) = modal_state.text_fields.get_mut(idx) {
                            if !field.numeric_only || ch.chars().all(|c| c.is_ascii_digit()) {
                                field.value.push_str(&ch);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn next_spawn_group_field(current: SpawnGroupField, behavior: BehaviorKind) -> SpawnGroupField {
    use SpawnGroupField::*;
    let cycle: &[SpawnGroupField] = if behavior == BehaviorKind::RoamAndChase {
        &[
            Id, Template, MaxCount, RespawnMean, AreaMinX, AreaMinY, AreaMaxX, AreaMaxY,
            StepInterval, BhvMinX, BhvMinY, BhvMaxX, BhvMaxY, DetectDistance, DisengageDistance,
        ]
    } else {
        &[
            Id, Template, MaxCount, RespawnMean, AreaMinX, AreaMinY, AreaMaxX, AreaMaxY,
            StepInterval, BhvMinX, BhvMinY, BhvMaxX, BhvMaxY,
        ]
    };
    let i = cycle.iter().position(|&f| f == current).unwrap_or(0);
    cycle[(i + 1) % cycle.len()]
}

// ── Confirm / Cancel button clicks ────────────────────────────────────────────

pub fn handle_modal_buttons(
    confirm_q: Query<&Interaction, (Changed<Interaction>, With<ModalConfirmButton>)>,
    cancel_q: Query<&Interaction, (Changed<Interaction>, With<ModalCancelButton>)>,
    mut modal_state: ResMut<ModalState>,
) {
    for interaction in &cancel_q {
        if *interaction == Interaction::Pressed {
            modal_state.active = None;
            modal_state.error_message = None;
        }
    }
    for interaction in &confirm_q {
        if *interaction == Interaction::Pressed {
            modal_state.confirm_triggered = true;
        }
    }
}

// ── List item clicks ──────────────────────────────────────────────────────────

pub fn handle_modal_list_click(
    items: Query<(&ModalListItem, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut modal_state: ResMut<ModalState>,
) {
    for (item, interaction) in &items {
        if *interaction == Interaction::Pressed {
            modal_state.selected_list_item = Some(item.index);
        }
    }
}

// ── Spawn-group modal layout ──────────────────────────────────────────────────

fn spawn_spawn_group_modal(
    kind: ModalKind,
    modal_state: &ModalState,
    theme: &UiThemeAssets,
    palette: Palette,
    commands: &mut Commands,
) {
    let Some(draft) = modal_state.spawn_group_draft.as_ref() else {
        return;
    };
    let theme = theme.clone();
    commands
        .spawn((
            ModalOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(palette.surface_overlay_strong),
            Button,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    ThemedPanel,
                    Node {
                        width: Val::Px(460.0),
                        max_height: Val::Percent(92.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(16.0)),
                        row_gap: Val::Px(8.0),
                        border: UiRect::all(Val::Px(1.0)),
                        overflow: Overflow::clip_y(),
                        ..default()
                    },
                    ImageNode::new(theme.panel_frame.clone())
                        .with_mode(theme.panel_image_mode())
                        .with_color(palette.surface_panel),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(palette.border_idle),
                ))
                .with_children(|card| {
                    card.spawn((
                        Text::new(title_for(kind)),
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(palette.text_accent),
                    ));

                    let err_text = modal_state.error_message.clone().unwrap_or_default();
                    let err_visible = if err_text.is_empty() {
                        Visibility::Hidden
                    } else {
                        Visibility::Visible
                    };
                    card.spawn((
                        ModalErrorText,
                        Text::new(err_text),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(palette.text_danger),
                        err_visible,
                    ));

                    // ── Identity ──
                    section_header(card, &palette, "Identity");
                    text_row(card, &palette, "id", &draft.id, draft.focused_field == SpawnGroupField::Id, SpawnGroupField::Id);
                    text_row(card, &palette, "template", &draft.template, draft.focused_field == SpawnGroupField::Template, SpawnGroupField::Template);

                    // ── Limits ──
                    section_header(card, &palette, "Limits");
                    two_col(card, |left| {
                        text_row(left, &palette, "max_count", &draft.max_count, draft.focused_field == SpawnGroupField::MaxCount, SpawnGroupField::MaxCount);
                    }, |right| {
                        text_row(right, &palette, "respawn_mean_seconds", &draft.respawn_mean_seconds, draft.focused_field == SpawnGroupField::RespawnMean, SpawnGroupField::RespawnMean);
                    });

                    // ── Spawn area ──
                    section_header(card, &palette, "Spawn Area");
                    radio_row(
                        card,
                        &palette,
                        &theme,
                        &[
                            ("Bounds", SpawnAreaKind::Bounds, draft.area_kind == SpawnAreaKind::Bounds),
                            ("Tiles (read-only in v1)", SpawnAreaKind::Tiles, draft.area_kind == SpawnAreaKind::Tiles),
                        ],
                        |label, kind| (label, SpawnGroupAreaKindButton { kind }),
                    );
                    rect_row(
                        card,
                        &palette,
                        "area",
                        &draft.area_min_x,
                        &draft.area_min_y,
                        &draft.area_max_x,
                        &draft.area_max_y,
                        draft.focused_field,
                        SpawnGroupField::AreaMinX,
                        SpawnGroupField::AreaMinY,
                        SpawnGroupField::AreaMaxX,
                        SpawnGroupField::AreaMaxY,
                    );
                    pick_rect_button(card, &palette, "Pick area on map", PickRectTarget::SpawnArea);
                    if draft.area_kind == SpawnAreaKind::Tiles {
                        card.spawn((
                            Text::new(format!("({} tiles in list — edit in YAML for now)", draft.area_tiles.len())),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(palette.text_muted),
                        ));
                    }

                    // ── Behavior ──
                    section_header(card, &palette, "Behavior");
                    radio_row(
                        card,
                        &palette,
                        &theme,
                        &[
                            ("Roam", BehaviorKind::Roam, draft.behavior_kind == BehaviorKind::Roam),
                            ("Roam + Chase", BehaviorKind::RoamAndChase, draft.behavior_kind == BehaviorKind::RoamAndChase),
                        ],
                        |label, kind| (label, SpawnGroupBehaviorKindButton { kind }),
                    );
                    text_row(card, &palette, "step_interval_seconds", &draft.step_interval_seconds, draft.focused_field == SpawnGroupField::StepInterval, SpawnGroupField::StepInterval);
                    rect_row(
                        card,
                        &palette,
                        "bounds",
                        &draft.bhv_min_x,
                        &draft.bhv_min_y,
                        &draft.bhv_max_x,
                        &draft.bhv_max_y,
                        draft.focused_field,
                        SpawnGroupField::BhvMinX,
                        SpawnGroupField::BhvMinY,
                        SpawnGroupField::BhvMaxX,
                        SpawnGroupField::BhvMaxY,
                    );
                    pick_rect_button(card, &palette, "Pick behavior bounds on map", PickRectTarget::SpawnBehavior);
                    if draft.behavior_kind == BehaviorKind::RoamAndChase {
                        two_col(card, |left| {
                            text_row(left, &palette, "detect_distance_tiles", &draft.detect_distance_tiles, draft.focused_field == SpawnGroupField::DetectDistance, SpawnGroupField::DetectDistance);
                        }, |right| {
                            text_row(right, &palette, "disengage_distance_tiles", &draft.disengage_distance_tiles, draft.focused_field == SpawnGroupField::DisengageDistance, SpawnGroupField::DisengageDistance);
                        });
                    }

                    // Buttons
                    card.spawn((Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: Val::Px(8.0),
                        margin: UiRect::top(Val::Px(8.0)),
                        ..default()
                    },))
                        .with_children(|row| {
                            spawn_modal_button(
                                row,
                                &theme,
                                &palette,
                                ButtonStyle::Secondary,
                                "Cancel",
                                ModalCancelButton,
                            );
                            spawn_modal_button(
                                row,
                                &theme,
                                &palette,
                                ButtonStyle::Primary,
                                confirm_label_for(kind),
                                ModalConfirmButton,
                            );
                        });
                });
        });
}

fn section_header(parent: &mut ChildSpawnerCommands, palette: &Palette, label: &str) {
    parent.spawn((
        Text::new(label.to_owned()),
        TextFont { font_size: 13.0, ..default() },
        TextColor(palette.text_accent),
        Node {
            margin: UiRect::top(Val::Px(4.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            padding: UiRect::bottom(Val::Px(2.0)),
            ..default()
        },
        BorderColor::all(palette.border_idle),
    ));
}

fn text_row(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    label: &str,
    value: &str,
    focused: bool,
    field: SpawnGroupField,
) {
    let display = if value.is_empty() && !focused {
        "(empty)".to_owned()
    } else if focused {
        format!("{value}_")
    } else {
        value.to_owned()
    };
    let display_color = if value.is_empty() && !focused {
        palette.text_placeholder
    } else {
        palette.text_value
    };
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            width: Val::Percent(100.0),
            ..default()
        },))
        .with_children(|col| {
            col.spawn((
                Text::new(label.to_owned()),
                TextFont { font_size: 11.0, ..default() },
                TextColor(palette.text_muted),
            ));
            col.spawn((
                Button,
                SpawnGroupFieldButton { field },
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.90)),
                BorderColor::all(if focused { palette.border_focus } else { palette.border_idle }),
            ))
            .with_children(|input| {
                input.spawn((
                    Text::new(display),
                    TextFont { font_size: 12.0, ..default() },
                    TextColor(display_color),
                ));
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn rect_row(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    label: &str,
    min_x: &str,
    min_y: &str,
    max_x: &str,
    max_y: &str,
    focused: SpawnGroupField,
    f_min_x: SpawnGroupField,
    f_min_y: SpawnGroupField,
    f_max_x: SpawnGroupField,
    f_max_y: SpawnGroupField,
) {
    parent.spawn((
        Text::new(label.to_owned()),
        TextFont { font_size: 11.0, ..default() },
        TextColor(palette.text_muted),
    ));
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(4.0),
            ..default()
        },))
        .with_children(|row| {
            for (val, f, lbl) in [
                (min_x, f_min_x, "min_x"),
                (min_y, f_min_y, "min_y"),
                (max_x, f_max_x, "max_x"),
                (max_y, f_max_y, "max_y"),
            ] {
                row.spawn((Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    flex_grow: 1.0,
                    ..default()
                },))
                    .with_children(|col| {
                        col.spawn((
                            Text::new(lbl.to_owned()),
                            TextFont { font_size: 9.0, ..default() },
                            TextColor(palette.text_muted),
                        ));
                        col.spawn((
                            Button,
                            SpawnGroupFieldButton { field: f },
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.90)),
                            BorderColor::all(if focused == f { palette.border_focus } else { palette.border_idle }),
                        ))
                        .with_children(|inp| {
                            let text = if focused == f {
                                format!("{val}_")
                            } else if val.is_empty() {
                                "—".to_owned()
                            } else {
                                val.to_owned()
                            };
                            inp.spawn((
                                Text::new(text),
                                TextFont { font_size: 12.0, ..default() },
                                TextColor(palette.text_value),
                            ));
                        });
                    });
            }
        });
}

fn pick_rect_button(parent: &mut ChildSpawnerCommands, palette: &Palette, label: &str, target: PickRectTarget) {
    parent
        .spawn((
            Button,
            SpawnGroupPickRectButton { target },
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                margin: UiRect::top(Val::Px(2.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.16, 0.10, 0.06, 0.95)),
            BorderColor::all(palette.border_idle),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_owned()),
                TextFont { font_size: 11.0, ..default() },
                TextColor(palette.text_value),
            ));
        });
}

fn radio_row<K, M>(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    _theme: &UiThemeAssets,
    options: &[(&str, K, bool)],
    mut to_marker: impl FnMut(&str, K) -> (&str, M),
) where
    K: Copy,
    M: Component,
{
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(6.0),
            ..default()
        },))
        .with_children(|row| {
            for &(label, kind, selected) in options {
                let (label, marker) = to_marker(label, kind);
                let (bg, border) = if selected {
                    (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50))
                } else {
                    (Color::srgba(0.12, 0.08, 0.06, 0.90), Color::srgb(0.38, 0.28, 0.18))
                };
                row.spawn((
                    Button,
                    marker,
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(bg),
                    BorderColor::all(border),
                ))
                .with_children(|b| {
                    b.spawn((
                        Text::new(label.to_owned()),
                        TextFont { font_size: 11.0, ..default() },
                        TextColor(palette.text_value),
                    ));
                });
            }
        });
}

fn two_col(
    parent: &mut ChildSpawnerCommands,
    left_fn: impl FnOnce(&mut ChildSpawnerCommands),
    right_fn: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                flex_grow: 1.0,
                ..default()
            },))
                .with_children(|col| left_fn(col));
            row.spawn((Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                flex_grow: 1.0,
                ..default()
            },))
                .with_children(|col| right_fn(col));
        });
}

// ── Spawn-group modal click handlers ──────────────────────────────────────────

pub fn handle_spawn_group_field_click(
    fields: Query<(&SpawnGroupFieldButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut modal_state: ResMut<ModalState>,
) {
    for (btn, interaction) in &fields {
        if *interaction == Interaction::Pressed {
            if let Some(draft) = modal_state.spawn_group_draft.as_mut() {
                draft.focused_field = btn.field;
            }
        }
    }
}

pub fn handle_spawn_group_area_kind_click(
    btns: Query<(&SpawnGroupAreaKindButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut modal_state: ResMut<ModalState>,
) {
    for (btn, interaction) in &btns {
        if *interaction == Interaction::Pressed {
            if let Some(draft) = modal_state.spawn_group_draft.as_mut() {
                draft.area_kind = btn.kind;
            }
        }
    }
}

pub fn handle_spawn_group_behavior_kind_click(
    btns: Query<(&SpawnGroupBehaviorKindButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut modal_state: ResMut<ModalState>,
) {
    for (btn, interaction) in &btns {
        if *interaction == Interaction::Pressed {
            if let Some(draft) = modal_state.spawn_group_draft.as_mut() {
                draft.behavior_kind = btn.kind;
            }
        }
    }
}

pub fn handle_spawn_group_pick_rect_click(
    btns: Query<(&SpawnGroupPickRectButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut modal_state: ResMut<ModalState>,
    mut editor_state: ResMut<crate::editor::resources::EditorState>,
) {
    use crate::editor::resources::EditorTool;
    for (btn, interaction) in &btns {
        if *interaction == Interaction::Pressed {
            // Stash the current tool so we can restore it after the pick.
            // SpawnGroupEdit is the only modal that opens pick mode today.
            if !matches!(editor_state.current_tool, EditorTool::PickRect { .. }) {
                editor_state.tool_before_pick = Some(editor_state.current_tool);
            }
            editor_state.current_tool = EditorTool::PickRect { target: btn.target };
            // Close the modal so the user can drag on the map. The draft is
            // preserved on `ModalState`; `apply_pick_rect_result` will reopen
            // the modal once a rect lands.
            modal_state.active = None;
        }
    }
}

/// Reopen the spawn-group modal with the picked rect applied to the matching
/// fields. Runs every frame; consumes `EditorPickRectResult.pending` once the
/// rect has been delivered.
pub fn apply_pick_rect_result_to_modal(
    mut pick_result: ResMut<crate::editor::resources::EditorPickRectResult>,
    mut modal_state: ResMut<ModalState>,
) {
    let Some(picked) = pick_result.pending.take() else {
        return;
    };
    let Some(draft) = modal_state.spawn_group_draft.as_mut() else {
        // No spawn-group draft to receive the pick; let the properties panel
        // pick it up via its own consumer (it polls `EditorPickRectResult`).
        pick_result.pending = Some(picked);
        return;
    };
    match picked.target {
        PickRectTarget::SpawnArea => {
            draft.area_min_x = picked.rect.min_x.to_string();
            draft.area_min_y = picked.rect.min_y.to_string();
            draft.area_max_x = picked.rect.max_x.to_string();
            draft.area_max_y = picked.rect.max_y.to_string();
            draft.area_kind = SpawnAreaKind::Bounds;
        }
        PickRectTarget::SpawnBehavior => {
            draft.bhv_min_x = picked.rect.min_x.to_string();
            draft.bhv_min_y = picked.rect.min_y.to_string();
            draft.bhv_max_x = picked.rect.max_x.to_string();
            draft.bhv_max_y = picked.rect.max_y.to_string();
        }
        PickRectTarget::InstanceBehavior => {
            // Belongs to the properties panel; put it back so its consumer
            // can grab it.
            pick_result.pending = Some(picked);
            return;
        }
    }
    // Reopen the modal in the same edit/create mode (taken from the draft).
    if !matches!(modal_state.active, Some(ModalKind::SpawnGroupEdit { .. })) {
        let editing_index = modal_state
            .spawn_group_draft
            .as_ref()
            .and_then(|d| d.editing_index);
        modal_state.active = Some(ModalKind::SpawnGroupEdit { editing_index });
    }
    modal_state.error_message = None;
    modal_state.confirm_triggered = false;
}

// ── Sync error text ───────────────────────────────────────────────────────────

pub fn sync_modal_error_text(
    modal_state: Res<ModalState>,
    mut error_q: Query<(&mut Text, &mut Visibility), With<ModalErrorText>>,
) {
    if !modal_state.is_changed() {
        return;
    }
    for (mut text, mut vis) in &mut error_q {
        if let Some(msg) = &modal_state.error_message {
            text.0 = msg.clone();
            *vis = Visibility::Visible;
        } else {
            text.0 = String::new();
            *vis = Visibility::Hidden;
        }
    }
}
