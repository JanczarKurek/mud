//! Toggleable side panel that edits the current map's `SpaceLightingDef`.
//!
//! Visibility is driven by `EditorState::lighting_panel_visible`. The panel
//! offers four sections:
//!
//! - **Time scrubber**: a horizontal track whose knob writes
//!   `WorldClock.time_of_day` so the darkness overlay shows lighting at the
//!   chosen instant. The world clock is already frozen in editor mode by
//!   `simulation_active`, so the value sits where the user puts it.
//! - **Ambient colors**: read-only previews of `indoor_ambient` /
//!   `outdoor_ambient` with stepper buttons (±16 per channel) — keeps the
//!   widget simple while still being editable.
//! - **`has_day_night` toggle**: flips the bool.
//! - **Keyframe list**: one row per `outdoor_curve` entry, with a color swatch
//!   plus Preview / Edit / Del buttons. Edit opens the
//!   `ModalKind::LightingKeyframeEdit` modal.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::editor::resources::{
    EditorLightingBuffer, EditorState, LightingKeyframeDraft, ModalKind, ModalState,
};
use crate::world::lighting::WorldClock;
use crate::world::map_layout::AmbientKeyframe;

/// Marker for the panel root — used by `EditorPanelRoots::cursor_over` and
/// the visibility-sync system.
#[derive(Component)]
pub struct EditorLightingRoot;

#[derive(Component)]
pub struct EditorLightingContent;

#[derive(Component)]
pub struct LightingScrubberTrack;

/// Filled portion of the time-of-day bar; width is `Val::Percent(ratio * 100.0)`.
/// Kept as a stable child entity so `sync_lighting_scrubber_visual` can update
/// it without despawning the scrubber `Button`, which would destroy the
/// `Interaction::Pressed` state mid-drag.
#[derive(Component)]
pub struct LightingScrubberFill;

/// Text node showing the current ratio next to the scrubber bar.
#[derive(Component)]
pub struct LightingScrubberLabel;

#[derive(Component, Clone, Copy)]
pub struct LightingKeyframeRow {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct LightingKeyframeEditButton {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct LightingKeyframeDeleteButton {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct LightingKeyframePreviewButton {
    pub index: usize,
}

#[derive(Component)]
pub struct LightingKeyframeAddButton;

#[derive(Component)]
pub struct LightingDayNightToggleButton;

/// Which channel/target a stepper button operates on. `+1`/`-1` is mapped to
/// a step of 16 per click on the color channels.
#[derive(Component, Clone, Copy)]
pub struct LightingAmbientStepperButton {
    pub target: AmbientStepperTarget,
    pub delta: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmbientStepperTarget {
    OutdoorR,
    OutdoorG,
    OutdoorB,
    IndoorR,
    IndoorG,
    IndoorB,
}

const PANEL_WIDTH_PX: f32 = 260.0;
const SCRUBBER_WIDTH_PX: f32 = 200.0;
const SCRUBBER_HEIGHT_PX: f32 = 14.0;
const COLOR_STEP: i16 = 16;

pub fn spawn_lighting_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorLightingRoot,
            Node {
                width: Val::Px(PANEL_WIDTH_PX),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
            BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
        ))
        .with_children(|panel| {
            // Header
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        align_items: AlignItems::Center,
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Lighting"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                });

            // Body (scrollable)
            panel.spawn((
                EditorLightingContent,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    padding: UiRect::all(Val::Px(8.0)),
                    row_gap: Val::Px(8.0),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                bevy::ui::ScrollPosition::default(),
            ));
        });
}

/// Toggle panel display via `EditorState::lighting_panel_visible`.
pub fn sync_lighting_panel_visibility(
    editor_state: Res<EditorState>,
    mut roots: Query<&mut Node, With<EditorLightingRoot>>,
) {
    if !editor_state.is_changed() {
        return;
    }
    let target = if editor_state.lighting_panel_visible {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut roots {
        if node.display != target {
            node.display = target;
        }
    }
}

/// Rebuild the panel contents whenever the buffer or visibility flag changes.
/// Deliberately **not** triggered by `WorldClock` changes — the scrubber drag
/// writes to it every frame, and a full despawn/respawn here would nuke the
/// scrubber `Button`'s `Interaction::Pressed` state and break the drag. The
/// per-frame fill update lives in `sync_lighting_scrubber_visual` below.
pub fn sync_lighting_panel(
    editor_state: Res<EditorState>,
    buffer: Res<EditorLightingBuffer>,
    world_clock: Res<WorldClock>,
    content: Query<Entity, With<EditorLightingContent>>,
    mut commands: Commands,
) {
    if !editor_state.lighting_panel_visible {
        return;
    }
    if !buffer.is_changed() && !editor_state.is_changed() {
        return;
    }

    let Ok(content_entity) = content.single() else {
        return;
    };

    commands
        .entity(content_entity)
        .despawn_related::<Children>();

    commands.entity(content_entity).with_children(|c| {
        // ── Time scrubber ──
        section_label(c, "Time of day");
        let ratio = world_clock.time_of_day.clamp(0.0, 1.0);
        c.spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        },))
            .with_children(|row| {
                row.spawn((
                    Button,
                    LightingScrubberTrack,
                    Node {
                        width: Val::Px(SCRUBBER_WIDTH_PX),
                        height: Val::Px(SCRUBBER_HEIGHT_PX),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.10, 0.08, 0.06, 0.95)),
                    BorderColor::all(Color::srgb(0.50, 0.40, 0.28)),
                ))
                .with_children(|track| {
                    // Filled portion
                    track.spawn((
                        LightingScrubberFill,
                        Node {
                            width: Val::Percent(ratio * 100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.80, 0.62, 0.32)),
                    ));
                });
                row.spawn((
                    LightingScrubberLabel,
                    Text::new(format!("{ratio:.3}")),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.86, 0.74)),
                ));
            });

        // ── has_day_night toggle ──
        section_label(c, "Day/night cycle");
        let dn_label = if buffer.config.has_day_night {
            "ON  (curve drives outdoor)"
        } else {
            "OFF (constant outdoor ambient)"
        };
        c.spawn((
            Button,
            LightingDayNightToggleButton,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(if buffer.config.has_day_night {
                Color::srgba(0.18, 0.12, 0.06, 0.95)
            } else {
                Color::srgba(0.10, 0.07, 0.06, 0.80)
            }),
            BorderColor::all(Color::srgb(0.55, 0.40, 0.22)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(dn_label),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.86, 0.66)),
            ));
        });

        // ── Ambient colors ──
        section_label(c, "Outdoor ambient");
        ambient_row(
            c,
            buffer.config.outdoor_ambient,
            [
                AmbientStepperTarget::OutdoorR,
                AmbientStepperTarget::OutdoorG,
                AmbientStepperTarget::OutdoorB,
            ],
        );
        section_label(c, "Indoor ambient");
        ambient_row(
            c,
            buffer.config.indoor_ambient,
            [
                AmbientStepperTarget::IndoorR,
                AmbientStepperTarget::IndoorG,
                AmbientStepperTarget::IndoorB,
            ],
        );

        // ── Keyframes ──
        section_label(c, "Day/night curve");
        if buffer.config.outdoor_curve.is_empty() {
            c.spawn((
                Text::new("(no keyframes — using engine default)"),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.55, 0.50, 0.44)),
            ));
        } else {
            for (index, kf) in buffer.config.outdoor_curve.iter().enumerate() {
                let selected = buffer.selected_keyframe == Some(index);
                let bg = if selected {
                    Color::srgba(0.20, 0.14, 0.08, 0.95)
                } else {
                    Color::srgba(0.10, 0.07, 0.06, 0.80)
                };
                let border = if selected {
                    Color::srgb(0.85, 0.65, 0.30)
                } else {
                    Color::srgb(0.20, 0.15, 0.10)
                };
                c.spawn((
                    Button,
                    LightingKeyframeRow { index },
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        row_gap: Val::Px(3.0),
                        ..default()
                    },
                    BackgroundColor(bg),
                    BorderColor::all(border),
                ))
                .with_children(|row| {
                    row.spawn((Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        ..default()
                    },))
                        .with_children(|line| {
                            line.spawn((
                                Node {
                                    width: Val::Px(28.0),
                                    height: Val::Px(16.0),
                                    border: UiRect::all(Val::Px(1.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb_u8(
                                    kf.color[0],
                                    kf.color[1],
                                    kf.color[2],
                                )),
                                BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
                            ));
                            line.spawn((
                                Text::new(format!("t={:.3}  α={:.2}", kf.time, kf.alpha)),
                                TextFont {
                                    font_size: 11.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.92, 0.86, 0.74)),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                        });
                    row.spawn((Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(4.0),
                        ..default()
                    },))
                        .with_children(|actions| {
                            row_action_button(
                                actions,
                                "Preview",
                                LightingKeyframePreviewButton { index },
                            );
                            row_action_button(
                                actions,
                                "Edit",
                                LightingKeyframeEditButton { index },
                            );
                            row_action_button(
                                actions,
                                "Del",
                                LightingKeyframeDeleteButton { index },
                            );
                        });
                });
            }
        }

        // Add-keyframe button
        c.spawn((
            Button,
            LightingKeyframeAddButton,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                margin: UiRect::top(Val::Px(4.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.18, 0.12, 0.06, 0.95)),
            BorderColor::all(Color::srgb(0.55, 0.40, 0.22)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("+ Add Keyframe"),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.86, 0.66)),
            ));
        });
    });
}

fn section_label(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text.to_owned()),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(Color::srgb(0.96, 0.84, 0.62)),
        Node {
            margin: UiRect::top(Val::Px(4.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            padding: UiRect::bottom(Val::Px(2.0)),
            ..default()
        },
        BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
    ));
}

fn ambient_row(
    parent: &mut ChildSpawnerCommands,
    rgb: [u8; 3],
    targets: [AmbientStepperTarget; 3],
) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(28.0),
                    height: Val::Px(28.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(rgb[0], rgb[1], rgb[2])),
                BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
            ));
            for (channel, value, target) in [
                ("R", rgb[0], targets[0]),
                ("G", rgb[1], targets[1]),
                ("B", rgb[2], targets[2]),
            ] {
                row.spawn((Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    align_items: AlignItems::Center,
                    flex_grow: 1.0,
                    ..default()
                },))
                    .with_children(|col| {
                        col.spawn((
                            Text::new(format!("{channel} {value}")),
                            TextFont {
                                font_size: 10.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.78, 0.74, 0.66)),
                        ));
                        col.spawn((Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(2.0),
                            ..default()
                        },))
                            .with_children(|btns| {
                                stepper_button(btns, "-", target, -COLOR_STEP);
                                stepper_button(btns, "+", target, COLOR_STEP);
                            });
                    });
            }
        });
}

fn stepper_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    target: AmbientStepperTarget,
    delta: i16,
) {
    parent
        .spawn((
            Button,
            LightingAmbientStepperButton { target, delta },
            Node {
                width: Val::Px(20.0),
                padding: UiRect::axes(Val::Px(2.0), Val::Px(1.0)),
                border: UiRect::all(Val::Px(1.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.14, 0.10, 0.08, 0.95)),
            BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.86, 0.74)),
            ));
        });
}

fn row_action_button<M: Component>(parent: &mut ChildSpawnerCommands, label: &str, marker: M) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.14, 0.10, 0.08, 0.95)),
            BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.86, 0.74)),
            ));
        });
}

/// Click + button handlers for the lighting panel. Bundles row select, action
/// buttons, day/night toggle, and ambient stepper buttons so we stay under
/// Bevy's per-system parameter cap.
#[allow(clippy::too_many_arguments)]
pub fn handle_lighting_panel_clicks(
    rows: Query<(&LightingKeyframeRow, &Interaction), (Changed<Interaction>, With<Button>)>,
    edit_btns: Query<
        (&LightingKeyframeEditButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    delete_btns: Query<
        (&LightingKeyframeDeleteButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    preview_btns: Query<
        (&LightingKeyframePreviewButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    add_btn: Query<&Interaction, (Changed<Interaction>, With<LightingKeyframeAddButton>)>,
    toggle_btn: Query<&Interaction, (Changed<Interaction>, With<LightingDayNightToggleButton>)>,
    stepper_btns: Query<
        (&LightingAmbientStepperButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    mut buffer: ResMut<EditorLightingBuffer>,
    mut editor_state: ResMut<EditorState>,
    mut modal_state: ResMut<ModalState>,
    mut world_clock: ResMut<WorldClock>,
) {
    for (row, interaction) in &rows {
        if *interaction == Interaction::Pressed {
            buffer.selected_keyframe = Some(row.index);
        }
    }

    for (btn, interaction) in &edit_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(kf) = buffer.config.outdoor_curve.get(btn.index).copied() else {
            continue;
        };
        buffer.selected_keyframe = Some(btn.index);
        open_keyframe_modal(&mut modal_state, Some(btn.index), Some(&kf));
    }

    for (btn, interaction) in &delete_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if btn.index >= buffer.config.outdoor_curve.len() {
            continue;
        }
        buffer.config.outdoor_curve.remove(btn.index);
        if buffer.selected_keyframe == Some(btn.index) {
            buffer.selected_keyframe = None;
        } else if let Some(s) = buffer.selected_keyframe {
            if s > btn.index {
                buffer.selected_keyframe = Some(s - 1);
            }
        }
        editor_state.dirty = true;
    }

    for (btn, interaction) in &preview_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(kf) = buffer.config.outdoor_curve.get(btn.index) else {
            continue;
        };
        world_clock.time_of_day = kf.time.clamp(0.0, 1.0);
        buffer.selected_keyframe = Some(btn.index);
    }

    for interaction in &add_btn {
        if *interaction != Interaction::Pressed || modal_state.active.is_some() {
            continue;
        }
        open_keyframe_modal(&mut modal_state, None, None);
    }

    for interaction in &toggle_btn {
        if *interaction == Interaction::Pressed {
            buffer.config.has_day_night = !buffer.config.has_day_night;
            editor_state.dirty = true;
        }
    }

    for (btn, interaction) in &stepper_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        apply_ambient_stepper(&mut buffer.config, btn.target, btn.delta);
        editor_state.dirty = true;
    }
}

fn apply_ambient_stepper(
    cfg: &mut crate::world::map_layout::SpaceLightingDef,
    target: AmbientStepperTarget,
    delta: i16,
) {
    let (rgb, ch) = match target {
        AmbientStepperTarget::OutdoorR => (&mut cfg.outdoor_ambient, 0),
        AmbientStepperTarget::OutdoorG => (&mut cfg.outdoor_ambient, 1),
        AmbientStepperTarget::OutdoorB => (&mut cfg.outdoor_ambient, 2),
        AmbientStepperTarget::IndoorR => (&mut cfg.indoor_ambient, 0),
        AmbientStepperTarget::IndoorG => (&mut cfg.indoor_ambient, 1),
        AmbientStepperTarget::IndoorB => (&mut cfg.indoor_ambient, 2),
    };
    let next = (rgb[ch] as i16 + delta).clamp(0, 255) as u8;
    rgb[ch] = next;
}

fn open_keyframe_modal(
    modal_state: &mut ModalState,
    editing_index: Option<usize>,
    existing: Option<&AmbientKeyframe>,
) {
    let draft = match (editing_index, existing) {
        (Some(idx), Some(kf)) => LightingKeyframeDraft::from_existing(idx, kf),
        _ => LightingKeyframeDraft::default(),
    };
    modal_state.active = Some(ModalKind::LightingKeyframeEdit { editing_index });
    modal_state.error_message = None;
    modal_state.confirm_triggered = false;
    modal_state.confirmed = None;
    modal_state.lighting_keyframe_draft = Some(draft);
}

/// Cheap in-place update of the scrubber's filled portion + ratio label,
/// reflecting the current `time_of_day` without rebuilding the panel.
/// Keeping the scrubber `Button` entity alive is what lets the drag handler
/// observe a continuous `Interaction::Pressed` while the user is scrubbing.
pub fn sync_lighting_scrubber_visual(
    editor_state: Res<EditorState>,
    world_clock: Res<WorldClock>,
    mut fill_q: Query<&mut Node, With<LightingScrubberFill>>,
    mut label_q: Query<&mut Text, With<LightingScrubberLabel>>,
) {
    if !editor_state.lighting_panel_visible {
        return;
    }
    if !world_clock.is_changed() && !editor_state.is_changed() {
        return;
    }
    let ratio = world_clock.time_of_day.clamp(0.0, 1.0);
    for mut node in &mut fill_q {
        node.width = Val::Percent(ratio * 100.0);
    }
    for mut text in &mut label_q {
        text.0 = format!("{ratio:.3}");
    }
}

/// Drag handler for the time scrubber. Reads the primary window's cursor
/// position; when the mouse is held inside the track, maps the x coordinate to
/// `time_of_day ∈ [0, 1]` and writes it to `WorldClock`.
pub fn handle_lighting_scrubber_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    track_q: Query<(&Interaction, &ComputedNode, &UiGlobalTransform), With<LightingScrubberTrack>>,
    editor_state: Res<EditorState>,
    mut world_clock: ResMut<WorldClock>,
) {
    if !editor_state.lighting_panel_visible || !mouse.pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    // `ComputedNode::size` and `UiGlobalTransform` live in physical pixels,
    // while `cursor_position` is logical — convert before any hit-test or
    // ratio math so HiDPI displays don't silently produce wrong results.
    let cursor_physical = cursor * window.scale_factor();
    for (interaction, computed, transform) in &track_q {
        // Trigger only when interaction is Pressed (mouse down) OR while
        // dragging — Bevy keeps Pressed as long as the button is held over
        // the node, so this captures both initial click and ongoing drag.
        if !matches!(interaction, Interaction::Pressed | Interaction::Hovered) {
            continue;
        }
        if !computed.contains_point(*transform, cursor_physical) {
            continue;
        }
        let size = computed.size();
        if size.x <= f32::EPSILON {
            continue;
        }
        let translation = transform.translation;
        let left = translation.x - size.x * 0.5;
        let ratio = ((cursor_physical.x - left) / size.x).clamp(0.0, 1.0);
        world_clock.time_of_day = ratio;
    }
}
