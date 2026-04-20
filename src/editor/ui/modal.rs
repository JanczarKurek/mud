#![allow(clippy::type_complexity)]
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::editor::resources::{ModalKind, ModalState};
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

fn title_for(kind: ModalKind) -> &'static str {
    match kind {
        ModalKind::FileOpen => "Open Map",
        ModalKind::SaveAs => "Save Map As",
        ModalKind::NewMap => "New Map",
        ModalKind::PortalCreate => "Add Portal",
    }
}

fn confirm_label_for(kind: ModalKind) -> &'static str {
    match kind {
        ModalKind::FileOpen => "Open",
        ModalKind::SaveAs => "Save",
        ModalKind::NewMap => "Create",
        ModalKind::PortalCreate => "Add",
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
                // Trigger confirm via a flag (picked up by handle_modal_confirm each frame)
                modal_state.confirm_triggered = true;
            }
            KeyCode::Tab => {
                let len = modal_state.text_fields.len();
                if len > 0 {
                    modal_state.focused_field = (modal_state.focused_field + 1) % len;
                }
            }
            KeyCode::Backspace => {
                let idx = modal_state.focused_field;
                if let Some(field) = modal_state.text_fields.get_mut(idx) {
                    field.value.pop();
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
