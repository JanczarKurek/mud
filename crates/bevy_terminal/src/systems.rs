use std::time::Duration;

use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::time::Time;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::widget::{
    LineStyle, Terminal, TerminalContent, TerminalCursor, TerminalFocus, TerminalInputAfter,
    TerminalInputBefore, TerminalInputPrompt, TerminalRoot, TerminalTheme, TerminalViewport,
};
use crate::{TerminalCompletionRequest, TerminalSubmit};

pub fn terminal_has_focus(focus: Res<TerminalFocus>) -> bool {
    focus.focused.is_some()
}

pub fn terminal_not_focused(focus: Res<TerminalFocus>) -> bool {
    focus.focused.is_none()
}

/// Walk up to `max_hops` parents from `start`; return true if `root` is
/// reached. The widget's tree is at most ~4 deep, so a hop cap of 8 is
/// safely generous.
fn ancestor_is(
    mut start: Entity,
    root: Entity,
    parents: &Query<&ChildOf>,
    max_hops: usize,
) -> bool {
    for _ in 0..max_hops {
        if start == root {
            return true;
        }
        let Ok(parent) = parents.get(start) else {
            return false;
        };
        start = parent.0;
    }
    false
}

/// Reads keyboard events and routes them to the terminal whose `focus_id`
/// matches `TerminalFocus::focused`. Mouse-wheel scrolling lives in
/// [`terminal_wheel_input`], which routes by cursor hover instead of focus
/// so users can scroll unfocused transcripts. Runs in `PreUpdate`.
#[allow(clippy::too_many_arguments)]
pub fn terminal_input(
    mut focus: ResMut<TerminalFocus>,
    mut key_events: MessageReader<KeyboardInput>,
    mut submit_events: bevy::ecs::message::MessageWriter<TerminalSubmit>,
    mut completion_events: bevy::ecs::message::MessageWriter<TerminalCompletionRequest>,
    mut terminals: Query<(Entity, &TerminalRoot, &mut Terminal)>,
    mut scroll_query: Query<(&ComputedNode, &mut ScrollPosition), With<TerminalViewport>>,
    parents: Query<&ChildOf>,
    viewport_entities: Query<Entity, With<TerminalViewport>>,
) {
    // Drain in all paths so a stale absorb token from a previous
    // close-then-reopen never bleeds into a subsequent frame.
    let absorbed_key = focus.absorbed_key.take();
    let Some(focused_id) = focus.focused else {
        // Drain so we don't accumulate events while no terminal is focused.
        key_events.read().for_each(|_| {});
        return;
    };

    let mut target = None;
    for (entity, root, terminal) in terminals.iter_mut() {
        if root.focus_id == focused_id {
            target = Some((entity, terminal));
            break;
        }
    }
    let Some((root_entity, mut terminal)) = target else {
        return;
    };

    // Resolve the viewport that belongs to this terminal once per call.
    let viewport_entity = viewport_entities
        .iter()
        .find(|e| ancestor_is(*e, root_entity, &parents, 6));

    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        if Some(event.key_code) == absorbed_key {
            continue;
        }
        match event.key_code {
            KeyCode::Enter => {
                if !terminal.input.enabled {
                    continue;
                }
                let text = terminal.input.submit();
                terminal.bump_input();
                if !text.is_empty() {
                    submit_events.write(TerminalSubmit {
                        terminal: root_entity,
                        text,
                    });
                }
            }
            KeyCode::Backspace => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.backspace();
                terminal.bump_input();
            }
            KeyCode::Delete => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.delete();
                terminal.bump_input();
            }
            KeyCode::ArrowLeft => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.move_left();
                terminal.bump_input();
            }
            KeyCode::ArrowRight => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.move_right();
                terminal.bump_input();
            }
            KeyCode::ArrowUp => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.history_up();
                terminal.bump_input();
            }
            KeyCode::ArrowDown => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.history_down();
                terminal.bump_input();
            }
            KeyCode::Home => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.move_home();
                terminal.bump_input();
            }
            KeyCode::End => {
                if !terminal.input.enabled {
                    continue;
                }
                terminal.input.move_end();
                terminal.bump_input();
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                let direction = if matches!(event.key_code, KeyCode::PageUp) {
                    -1.0_f32
                } else {
                    1.0
                };
                if let Some(vp) = viewport_entity {
                    if let Ok((computed, mut scroll)) = scroll_query.get_mut(vp) {
                        let page = computed.size().y * 0.9 * computed.inverse_scale_factor();
                        let max = (computed.content_size().y - computed.size().y).max(0.0)
                            * computed.inverse_scale_factor();
                        scroll.y = (scroll.y + direction * page).clamp(0.0, max);
                        terminal.auto_pin_bottom = !(max > 0.0 && (max - scroll.y) > 1.0);
                    }
                }
            }
            KeyCode::Tab => {
                if !terminal.input.enabled || !terminal.input.completion {
                    continue;
                }
                completion_events.write(TerminalCompletionRequest {
                    terminal: root_entity,
                    text_before_cursor: terminal.input.text_before_cursor(),
                });
            }
            _ => {
                if !terminal.input.enabled {
                    continue;
                }
                if event.repeat && matches!(event.logical_key, Key::Character(_)) {
                    continue;
                }
                match &event.logical_key {
                    Key::Character(c) => {
                        terminal.input.insert_str(c.as_str());
                        terminal.bump_input();
                    }
                    Key::Space => {
                        terminal.input.insert_str(" ");
                        terminal.bump_input();
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Routes mouse-wheel events to whichever terminal viewport the cursor is
/// hovering over, regardless of keyboard focus. Lets users scroll an
/// unfocused chat transcript or the python console without first claiming
/// focus. Runs in `PreUpdate` alongside [`terminal_input`].
pub fn terminal_wheel_input(
    mut wheel_events: MessageReader<MouseWheel>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut viewports: Query<
        (
            Entity,
            &ComputedNode,
            &UiGlobalTransform,
            &mut ScrollPosition,
        ),
        With<TerminalViewport>,
    >,
    mut terminals: Query<&mut Terminal>,
    parents: Query<&ChildOf>,
) {
    let wheel_total: f32 = wheel_events
        .read()
        .map(|e| {
            let scale = if matches!(e.unit, MouseScrollUnit::Line) {
                21.0
            } else {
                1.0
            };
            -e.y * scale
        })
        .sum();
    if wheel_total.abs() <= f32::EPSILON {
        return;
    }
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    let mut hit: Option<(Entity, f32, f32)> = None;
    for (vp_entity, computed, transform, mut scroll) in &mut viewports {
        let inv = computed.inverse_scale_factor();
        let physical = if inv > 0.0 { cursor / inv } else { cursor };
        if !computed.contains_point(*transform, physical) {
            continue;
        }
        let max = (computed.content_size().y - computed.size().y).max(0.0) * inv;
        scroll.y = (scroll.y + wheel_total).clamp(0.0, max);
        hit = Some((vp_entity, scroll.y, max));
        break;
    }
    let Some((vp_entity, scroll_y, max)) = hit else {
        return;
    };

    // Walk parents from the viewport to find the owning `Terminal` so we
    // can clear `auto_pin_bottom` when the user scrolls away from the
    // bottom (mirrors what `terminal_input` used to do for the focused
    // terminal).
    let mut current = vp_entity;
    for _ in 0..6 {
        if let Ok(mut terminal) = terminals.get_mut(current) {
            if max > 0.0 && (max - scroll_y) > 1.0 {
                terminal.auto_pin_bottom = false;
            }
            break;
        }
        let Ok(parent) = parents.get(current) else {
            break;
        };
        current = parent.0;
    }
}

/// Rebuilds the line-Text children whenever `Terminal::revision` advances.
pub fn terminal_sync_buffer(
    theme: Res<TerminalTheme>,
    mut commands: Commands,
    mut terminals: Query<(Entity, &mut Terminal)>,
    content_q: Query<Entity, With<TerminalContent>>,
    parents: Query<&ChildOf>,
) {
    for (root_entity, mut terminal) in &mut terminals {
        if terminal.revision == terminal.last_rendered_revision {
            continue;
        }
        let content_entity = content_q
            .iter()
            .find(|e| ancestor_is(*e, root_entity, &parents, 6));
        let Some(content_entity) = content_entity else {
            continue;
        };

        terminal.last_rendered_revision = terminal.revision;
        let lines: Vec<(String, LineStyle)> = terminal
            .buffer
            .iter()
            .map(|l| (l.text.clone(), l.style))
            .collect();
        let theme = theme.clone();
        commands
            .entity(content_entity)
            .despawn_related::<Children>();
        commands.entity(content_entity).with_children(move |c| {
            for (text, style) in lines {
                c.spawn((
                    Text::new(text),
                    TextLayout::new(Justify::Left, LineBreak::WordBoundary),
                    TextFont {
                        font_size: theme.font_size,
                        ..default()
                    },
                    TextColor(theme.color_for(style)),
                    Node {
                        width: Val::Percent(100.0),
                        margin: UiRect::bottom(Val::Px(theme.line_gap)),
                        ..default()
                    },
                ));
            }
        });
    }
}

pub fn terminal_sync_input_line(
    theme: Res<TerminalTheme>,
    mut terminals: Query<(Entity, &mut Terminal)>,
    mut prompt_q: Query<
        (Entity, &mut Text, &mut TextColor),
        (
            With<TerminalInputPrompt>,
            Without<TerminalInputBefore>,
            Without<TerminalInputAfter>,
        ),
    >,
    mut before_q: Query<
        (Entity, &mut Text, &mut TextColor),
        (
            With<TerminalInputBefore>,
            Without<TerminalInputPrompt>,
            Without<TerminalInputAfter>,
        ),
    >,
    mut after_q: Query<
        (Entity, &mut Text, &mut TextColor),
        (
            With<TerminalInputAfter>,
            Without<TerminalInputPrompt>,
            Without<TerminalInputBefore>,
        ),
    >,
    mut cursor_q: Query<(Entity, &mut Node), With<TerminalCursor>>,
    parents: Query<&ChildOf>,
) {
    for (root_entity, mut terminal) in &mut terminals {
        if !terminal.input.enabled {
            continue;
        }
        if terminal.input_revision == terminal.last_input_revision {
            continue;
        }
        terminal.last_input_revision = terminal.input_revision;
        let prompt_text = terminal.input.prompt.clone();
        let buffer = terminal.input.buffer.clone();
        let cursor_chars = terminal.input.cursor;
        let font_size = theme.font_size;
        let input_color = theme.input_color;
        let prompt_color = theme.prompt_color;

        // Compute the byte split for the cursor so we can hand each Text
        // its own slice — Bevy's layout pipeline then positions the cursor
        // Node automatically in the flex flow between the two segments.
        let split_byte = buffer
            .char_indices()
            .nth(cursor_chars)
            .map(|(i, _)| i)
            .unwrap_or(buffer.len());
        let (before, after) = buffer.split_at(split_byte);

        for (entity, mut text, mut color) in &mut prompt_q {
            if !ancestor_is(entity, root_entity, &parents, 6) {
                continue;
            }
            text.0 = prompt_text.clone();
            color.0 = prompt_color;
        }
        for (entity, mut text, mut color) in &mut before_q {
            if !ancestor_is(entity, root_entity, &parents, 6) {
                continue;
            }
            text.0 = before.to_owned();
            color.0 = input_color;
        }
        for (entity, mut text, mut color) in &mut after_q {
            if !ancestor_is(entity, root_entity, &parents, 6) {
                continue;
            }
            text.0 = after.to_owned();
            color.0 = input_color;
        }
        for (cursor_entity, mut node) in &mut cursor_q {
            if !ancestor_is(cursor_entity, root_entity, &parents, 6) {
                continue;
            }
            node.height = Val::Px(font_size + 2.0);
            node.width = Val::Px(2.0);
        }
    }
}

/// After UI layout, pin the viewport's scroll position to the bottom for
/// any terminal whose buffer was just mutated. Mirrors the project's
/// `auto_pin_dialog_transcript_scroll`.
pub fn terminal_pin_scroll(
    mut terminals: Query<(Entity, &mut Terminal)>,
    mut viewport_q: Query<(Entity, &ComputedNode, &mut ScrollPosition), With<TerminalViewport>>,
    parents: Query<&ChildOf>,
) {
    for (root_entity, mut terminal) in &mut terminals {
        if !terminal.auto_pin_bottom {
            continue;
        }
        let mut pinned = false;
        for (vp_entity, computed, mut scroll) in &mut viewport_q {
            if !ancestor_is(vp_entity, root_entity, &parents, 6) {
                continue;
            }
            if computed.size().y <= 0.0 {
                continue;
            }
            let max = (computed.content_size().y - computed.size().y).max(0.0)
                * computed.inverse_scale_factor();
            let target = max.max(0.0);
            if (scroll.y - target).abs() > 0.5 {
                scroll.y = target;
            }
            pinned = true;
        }
        if pinned {
            terminal.auto_pin_bottom = false;
        }
    }
}

pub fn terminal_blink_cursor(
    time: Res<Time>,
    focus: Res<TerminalFocus>,
    mut blink: Local<BlinkState>,
    terminals: Query<(Entity, &TerminalRoot, &Terminal)>,
    mut cursor_q: Query<(Entity, &mut Visibility), With<TerminalCursor>>,
    parents: Query<&ChildOf>,
) {
    if blink.timer.duration().is_zero() {
        blink.timer = Timer::new(Duration::from_millis(500), TimerMode::Repeating);
        blink.visible = true;
    }
    if blink.timer.tick(time.delta()).just_finished() {
        blink.visible = !blink.visible;
    }

    let focused_root = terminals
        .iter()
        .find(|(_, root, _)| Some(root.focus_id) == focus.focused)
        .map(|(e, _, _)| e);

    for (cursor_entity, mut visibility) in &mut cursor_q {
        let on = focused_root
            .map(|root| ancestor_is(cursor_entity, root, &parents, 6))
            .unwrap_or(false);
        let want = if on && blink.visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
    }
}

#[derive(Default)]
pub struct BlinkState {
    timer: Timer,
    visible: bool,
}
