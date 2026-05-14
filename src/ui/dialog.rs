//! Dialog window UI: reads `ActiveDialogState` and drives a floating
//! `MovableWindow` that's spawned when a dialog session opens and despawned
//! when it closes (mirrors `sync_trade_window_lifecycle` in
//! `crate::ui::trade`). The body holds a scrolling transcript log, the
//! current option buttons, and a Continue button. Click handlers queue
//! `DialogAdvance` / `DialogChoose` / `DialogEnd` game commands.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::ui::components::{
    DialogPanelCloseButton, DialogPanelContinueButton, DialogPanelOptionButton,
    DialogPanelOptionsContainer, DialogPanelRoot, DialogPanelTranscriptContainer,
    DialogPanelTranscriptScrollNode,
};
use crate::ui::movable_window::{
    spawn_movable_window, spawn_themed_close_button, val_to_px, MovableWindowDrag,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::resources::{ActiveDialogState, DialogEntry, DialogEntryKind};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};

/// Initial size when no cached `last_size` is available. The user can
/// drag the bottom-right grip to resize down to
/// [`MOVABLE_WINDOW_DEFAULT_MIN_SIZE`] or up to whatever fits the screen.
const DEFAULT_DIALOG_SIZE: Vec2 = Vec2::new(440.0, 360.0);

/// Per-render caches so the option/transcript sync systems can early-exit
/// without diffing vectors.
#[derive(Resource, Default)]
pub struct DialogPanelRenderState {
    pub last_revision: u64,
    pub last_transcript_revision: u64,
    /// Set true whenever the transcript was just rebuilt; an auto-pin
    /// system clears it once the scroll position is at the bottom.
    pub pin_to_bottom_pending: bool,
}

/// Spawn / despawn the dialog window based on `ActiveDialogState.is_active()`.
/// While open, caches position/size each frame so the next session re-opens
/// in the same place. Mirrors `sync_trade_window_lifecycle`.
#[allow(clippy::too_many_arguments)]
pub fn sync_dialog_window_lifecycle(
    mut commands: Commands,
    mut state: ResMut<ActiveDialogState>,
    mut render_state: ResMut<DialogPanelRenderState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    existing: Query<(Entity, &Node), With<DialogPanelRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.is_active();
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            let win = window_query
                .single()
                .map(|window| Vec2::new(window.width(), window.height()))
                .unwrap_or(Vec2::new(1280.0, 720.0));
            let size = state.last_size.unwrap_or(DEFAULT_DIALOG_SIZE);
            let pos = state
                .last_position
                .unwrap_or_else(|| ((win - size) * 0.5).max(Vec2::ZERO));
            let root = spawn_dialog_window(
                &mut commands,
                &theme,
                &palette,
                pos,
                size,
                &state.transcript,
                &state.options,
                state.awaiting_continue,
            );
            drag.focused = Some(root);
            // Window is born already populated with the current state, so
            // record the revisions to skip an unnecessary rebuild next
            // frame.
            render_state.last_revision = state.revision;
            render_state.last_transcript_revision = state.transcript_revision;
            render_state.pin_to_bottom_pending = true;
        }
        (false, Some((root, _))) => {
            commands.entity(root).despawn();
            if drag.focused == Some(root) {
                drag.focused = None;
            }
            if drag.dragging.is_some_and(|(e, _)| e == root) {
                drag.dragging = None;
            }
        }
        (true, Some((_, node))) => {
            let pos = Vec2::new(val_to_px(node.left), val_to_px(node.top));
            let size = Vec2::new(val_to_px(node.width), val_to_px(node.height));
            if state.last_position != Some(pos) {
                state.last_position = Some(pos);
            }
            if state.last_size != Some(size) {
                state.last_size = Some(size);
            }
        }
        (false, None) => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_dialog_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    position: Vec2,
    size: Vec2,
    transcript: &[DialogEntry],
    options: &[String],
    awaiting_continue: bool,
) -> Entity {
    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::Dialog,
        "Talk",
        size,
        position,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );

    commands.entity(spawned.root).insert(DialogPanelRoot);

    // Dialog has its own close button that emits `DialogEnd` rather than
    // despawning the entity directly — the lifecycle handles the despawn
    // once the server clears the session id.
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_themed_close_button(bar, theme, DialogPanelCloseButton);
    });

    let theme_owned = theme.clone();
    let palette_copy = *palette;
    let transcript_owned = transcript.to_vec();
    let options_owned = options.to_vec();

    commands.entity(spawned.body).with_children(move |body| {
        body.spawn((
            DialogPanelTranscriptContainer,
            Node {
                width: percent(100.0),
                flex_grow: 1.0,
                min_height: px(0.0),
                overflow: Overflow::scroll_y(),
                padding: UiRect::all(px(4.0)),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(palette_copy.surface_console_output),
        ))
        .with_children(|scroll| {
            scroll
                .spawn((
                    DialogPanelTranscriptScrollNode,
                    Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6.0),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|column| {
                    for entry in &transcript_owned {
                        spawn_transcript_entry(column, &palette_copy, entry);
                    }
                });
        });

        body.spawn((
            DialogPanelOptionsContainer,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                width: percent(100.0),
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|container| {
            for (idx, text) in options_owned.iter().enumerate() {
                spawn_option_button(container, &theme_owned, &palette_copy, idx, text);
            }
        });

        let (bg, border, text_color) = idle_colors(&palette_copy, ButtonStyle::Primary, false);
        body.spawn((
            Button,
            ThemedButton::new(ButtonStyle::Primary),
            DialogPanelContinueButton,
            Node {
                width: percent(100.0),
                min_height: px(28.0),
                padding: UiRect::axes(px(8.0), px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                flex_shrink: 0.0,
                display: if awaiting_continue {
                    Display::Flex
                } else {
                    Display::None
                },
                ..default()
            },
            ImageNode::new(theme_owned.button_frame.clone())
                .with_mode(theme_owned.button_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new("Continue"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(text_color),
            ));
        });
    });

    spawned.root
}

fn spawn_transcript_entry(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    entry: &DialogEntry,
) {
    let (prefix, prefix_color) = match entry.kind {
        DialogEntryKind::Npc => {
            let name = entry.speaker.as_deref().unwrap_or("???");
            (format!("{name}: "), palette.text_accent)
        }
        DialogEntryKind::Player => ("You: ".to_owned(), palette.text_value),
    };
    parent
        .spawn((
            Text::new(prefix),
            TextFont {
                font_size: 15.0,
                ..default()
            },
            TextColor(prefix_color),
            TextLayout::new(
                bevy::text::Justify::Left,
                bevy::text::LineBreak::WordBoundary,
            ),
            Node {
                width: percent(100.0),
                ..default()
            },
        ))
        .with_children(|line| {
            line.spawn((
                TextSpan::new(entry.text.clone()),
                TextFont {
                    font_size: 15.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));
        });
}

fn spawn_option_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    idx: usize,
    text: &str,
) {
    let (bg, border, text_color) = idle_colors(palette, ButtonStyle::Secondary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Secondary),
            DialogPanelOptionButton { option_idx: idx },
            Node {
                width: percent(100.0),
                min_height: px(28.0),
                padding: UiRect::axes(px(8.0), px(4.0)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ImageNode::new(theme.button_frame.clone())
                .with_mode(theme.button_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(text.to_owned()),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(text_color),
            ));
        });
}

/// Rebuild the transcript children when `transcript_revision` changes.
pub fn sync_dialog_panel_transcript(
    mut render_state: ResMut<DialogPanelRenderState>,
    dialog_state: Res<ActiveDialogState>,
    palette: Res<Palette>,
    container_query: Query<Entity, With<DialogPanelTranscriptScrollNode>>,
    mut commands: Commands,
) {
    if render_state.last_transcript_revision == dialog_state.transcript_revision {
        return;
    }
    if container_query.is_empty() {
        // Window not yet committed (just spawned this frame). The lifecycle
        // populated the transcript inline, so leave revisions to be caught
        // up by the spawn path. Don't advance our cursor here.
        return;
    }
    render_state.last_transcript_revision = dialog_state.transcript_revision;
    render_state.pin_to_bottom_pending = true;

    let palette = *palette;
    let entries = dialog_state.transcript.clone();
    for container in &container_query {
        commands.entity(container).despawn_related::<Children>();
        let entries = entries.clone();
        commands.entity(container).with_children(move |parent| {
            for entry in &entries {
                spawn_transcript_entry(parent, &palette, entry);
            }
        });
    }
}

/// After UI layout, if the transcript was just rebuilt, snap the scroll
/// position to the bottom. Runs in `PostUpdate` after `UiSystems::PostLayout`
/// so `ComputedNode` reflects the freshly rebuilt content.
pub fn auto_pin_dialog_transcript_scroll(
    mut render_state: ResMut<DialogPanelRenderState>,
    mut viewport_query: Query<
        (&ComputedNode, &mut ScrollPosition),
        With<DialogPanelTranscriptContainer>,
    >,
) {
    if !render_state.pin_to_bottom_pending {
        return;
    }
    let mut all_pinned = true;
    for (computed, mut scroll) in &mut viewport_query {
        if computed.size().y <= 0.0 {
            all_pinned = false;
            continue;
        }
        let max_offset =
            (computed.content_size().y - computed.size().y) * computed.inverse_scale_factor();
        let target = max_offset.max(0.0);
        if (scroll.y - target).abs() > 0.5 {
            scroll.y = target;
        }
    }
    if all_pinned {
        render_state.pin_to_bottom_pending = false;
    }
}

pub fn sync_dialog_panel_continue_button(
    dialog_state: Res<ActiveDialogState>,
    mut button_query: Query<&mut Node, With<DialogPanelContinueButton>>,
) {
    let target = if dialog_state.awaiting_continue {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut button_query {
        if node.display != target {
            node.display = target;
        }
    }
}

pub fn sync_dialog_panel_options(
    mut render_state: ResMut<DialogPanelRenderState>,
    dialog_state: Res<ActiveDialogState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    container_query: Query<Entity, With<DialogPanelOptionsContainer>>,
    mut commands: Commands,
) {
    if render_state.last_revision == dialog_state.revision {
        return;
    }
    if container_query.is_empty() {
        // Same as the transcript sync: the lifecycle inline-populated the
        // options when it spawned the window; wait for the entity to be
        // committed before we start rebuilding.
        return;
    }
    render_state.last_revision = dialog_state.revision;

    let theme = theme.clone();
    let palette = *palette;
    let options = dialog_state.options.clone();
    for container in &container_query {
        commands.entity(container).despawn_related::<Children>();
        let theme = theme.clone();
        let options = options.clone();
        commands.entity(container).with_children(move |parent| {
            for (idx, text) in options.iter().enumerate() {
                spawn_option_button(parent, &theme, &palette, idx, text);
            }
        });
    }
}

/// Mouse-wheel scrolling for the transcript viewport. Mirrors
/// `handle_docked_panel_scrolling` — Bevy 0.18 doesn't auto-scroll on wheel
/// when `Overflow::scroll_y()` is used; we have to drive `ScrollPosition`
/// ourselves.
pub fn handle_dialog_transcript_scrolling(
    mut wheel_reader: MessageReader<MouseWheel>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut viewport_query: Query<
        (
            &Node,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ScrollPosition,
        ),
        With<DialogPanelTranscriptContainer>,
    >,
    mut render_state: ResMut<DialogPanelRenderState>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    for event in wheel_reader.read() {
        let mut delta_y = -event.y;
        if event.unit == MouseScrollUnit::Line {
            delta_y *= 21.0;
        }
        if delta_y == 0.0 {
            continue;
        }
        for (node, computed, transform, mut scroll) in &mut viewport_query {
            if !computed.contains_point(*transform, cursor) {
                continue;
            }
            if node.overflow.y != bevy::ui::OverflowAxis::Scroll {
                continue;
            }
            let max_offset =
                (computed.content_size().y - computed.size().y) * computed.inverse_scale_factor();
            if max_offset <= 0.0 {
                break;
            }
            scroll.y = (scroll.y + delta_y).clamp(0.0, max_offset);
            // User took manual control — disable auto-pin until the next
            // transcript update reasserts it.
            render_state.pin_to_bottom_pending = false;
            break;
        }
    }
}

/// Route clicks from the dialog panel's buttons (continue, close, options)
/// into game commands. Reads Bevy's per-Button `Interaction::Pressed` state
/// rather than doing manual cursor-vs-rect hit testing — the widgets are
/// already `Button`s, and `Interaction` correctly accounts for layout,
/// transforms, scrolling, scale factor, and `Display::None` without us
/// reimplementing any of it.
pub fn handle_dialog_panel_clicks(
    mut dialog_state: ResMut<ActiveDialogState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    continue_query: Query<&Interaction, (Changed<Interaction>, With<DialogPanelContinueButton>)>,
    close_query: Query<&Interaction, (Changed<Interaction>, With<DialogPanelCloseButton>)>,
    option_query: Query<(&Interaction, &DialogPanelOptionButton), Changed<Interaction>>,
) {
    let Some(session_id) = dialog_state.session_id else {
        return;
    };

    if dialog_state.awaiting_continue {
        for interaction in &continue_query {
            if matches!(interaction, Interaction::Pressed) {
                pending_commands.push(GameCommand::DialogAdvance { session_id });
                return;
            }
        }
    }

    for interaction in &close_query {
        if matches!(interaction, Interaction::Pressed) {
            pending_commands.push(GameCommand::DialogEnd { session_id });
            return;
        }
    }

    for (interaction, option) in &option_query {
        if matches!(interaction, Interaction::Pressed) {
            let chosen_text = dialog_state
                .options
                .get(option.option_idx)
                .cloned()
                .unwrap_or_default();
            if !chosen_text.is_empty() {
                dialog_state.push_player_choice(chosen_text);
            }
            pending_commands.push(GameCommand::DialogChoose {
                session_id,
                option_idx: option.option_idx,
            });
            return;
        }
    }
}
