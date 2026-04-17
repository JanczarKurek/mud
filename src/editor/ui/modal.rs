#![allow(clippy::type_complexity)]
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::editor::resources::{ModalKind, ModalState};

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

// ── Colours (match title-screen / editor palette) ─────────────────────────────

const BG_OVERLAY: Color = Color::srgba(0.0, 0.0, 0.0, 0.72);
const BG_CARD: Color = Color::srgba(0.08, 0.05, 0.04, 0.97);
const BORDER: Color = Color::srgb(0.48, 0.36, 0.22);
const BORDER_FOCUSED: Color = Color::srgb(0.90, 0.72, 0.40);
const TEXT_HEADER: Color = Color::srgb(0.96, 0.84, 0.62);
const TEXT_LABEL: Color = Color::srgb(0.75, 0.70, 0.62);
const TEXT_VALUE: Color = Color::srgb(0.96, 0.92, 0.80);
const TEXT_PLACEHOLDER: Color = Color::srgb(0.45, 0.42, 0.38);
const TEXT_ERROR: Color = Color::srgb(1.0, 0.45, 0.30);
const BTN_NORMAL_BG: Color = Color::srgba(0.14, 0.10, 0.08, 0.96);
const BTN_HOVER_BG: Color = Color::srgb(0.28, 0.17, 0.10);
const BTN_PRESS_BG: Color = Color::srgb(0.55, 0.30, 0.14);
const BTN_CONFIRM_BORDER: Color = Color::srgb(0.70, 0.55, 0.28);

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
            BackgroundColor(BG_OVERLAY),
            // Consume all interaction so nothing behind the modal is clickable.
            Button,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(380.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(18.0)),
                        row_gap: Val::Px(10.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(BG_CARD),
                    BorderColor::all(BORDER),
                ))
                .with_children(|card| {
                    // Title
                    card.spawn((
                        Text::new(title_for(kind)),
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(TEXT_HEADER),
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
                        TextColor(TEXT_ERROR),
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
                                    TextColor(TEXT_LABEL),
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
                                            BORDER_FOCUSED
                                        } else {
                                            BORDER
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
                                                TextColor(TEXT_VALUE),
                                            ));
                                        } else {
                                            input.spawn((
                                                Text::new(field.placeholder.clone()),
                                                TextFont {
                                                    font_size: 13.0,
                                                    ..default()
                                                },
                                                TextColor(TEXT_PLACEHOLDER),
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
                            BorderColor::all(BORDER),
                            BackgroundColor(Color::srgba(0.05, 0.03, 0.03, 0.90)),
                        ))
                        .with_children(|list| {
                            for (i, name) in modal_state.list_items.iter().enumerate() {
                                let is_selected = modal_state.selected_list_item == Some(i);
                                list.spawn((
                                    Button,
                                    ModalListItem { index: i },
                                    Node {
                                        width: Val::Percent(100.0),
                                        padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                                        border: UiRect::bottom(Val::Px(1.0)),
                                        ..default()
                                    },
                                    BackgroundColor(if is_selected {
                                        Color::srgb(0.28, 0.16, 0.08)
                                    } else {
                                        Color::srgba(0.0, 0.0, 0.0, 0.0)
                                    }),
                                    BorderColor::all(Color::srgb(0.20, 0.14, 0.10)),
                                ))
                                .with_children(|btn| {
                                    btn.spawn((
                                        Text::new(name.clone()),
                                        TextFont {
                                            font_size: 13.0,
                                            ..default()
                                        },
                                        TextColor(if is_selected {
                                            Color::srgb(0.98, 0.90, 0.70)
                                        } else {
                                            TEXT_VALUE
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
                            row.spawn((
                                Button,
                                ModalCancelButton,
                                Node {
                                    padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    ..default()
                                },
                                BackgroundColor(BTN_NORMAL_BG),
                                BorderColor::all(BORDER),
                            ))
                            .with_children(|b| {
                                b.spawn((
                                    Text::new("Cancel"),
                                    TextFont {
                                        font_size: 13.0,
                                        ..default()
                                    },
                                    TextColor(TEXT_VALUE),
                                ));
                            });

                            row.spawn((
                                Button,
                                ModalConfirmButton,
                                Node {
                                    padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    ..default()
                                },
                                BackgroundColor(BTN_NORMAL_BG),
                                BorderColor::all(BTN_CONFIRM_BORDER),
                            ))
                            .with_children(|b| {
                                b.spawn((
                                    Text::new(confirm_label_for(kind)),
                                    TextFont {
                                        font_size: 13.0,
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.98, 0.90, 0.70)),
                                ));
                            });
                        });
                });
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

/// Sync button colours each frame (hover / press / normal).
pub fn sync_modal_button_colours(
    mut confirm_q: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        With<ModalConfirmButton>,
    >,
    mut cancel_q: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (With<ModalCancelButton>, Without<ModalConfirmButton>),
    >,
) {
    for (interaction, mut bg, mut border) in &mut confirm_q {
        let (bg_c, b_c) = match *interaction {
            Interaction::Pressed => (BTN_PRESS_BG, Color::srgb(1.0, 0.88, 0.60)),
            Interaction::Hovered => (BTN_HOVER_BG, Color::srgb(0.90, 0.75, 0.50)),
            Interaction::None => (BTN_NORMAL_BG, BTN_CONFIRM_BORDER),
        };
        bg.0 = bg_c;
        *border = BorderColor::all(b_c);
    }
    for (interaction, mut bg, mut border) in &mut cancel_q {
        let (bg_c, b_c) = match *interaction {
            Interaction::Pressed => (BTN_PRESS_BG, Color::srgb(0.80, 0.65, 0.45)),
            Interaction::Hovered => (BTN_HOVER_BG, Color::srgb(0.65, 0.50, 0.32)),
            Interaction::None => (BTN_NORMAL_BG, BORDER),
        };
        bg.0 = bg_c;
        *border = BorderColor::all(b_c);
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
