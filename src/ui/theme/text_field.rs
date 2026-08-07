//! Shared click-to-focus text-field widget.
//!
//! Every text input in the game follows the same pattern: a muted label over a
//! bordered box that shows the current value (with an optional trailing `_`
//! cursor and placeholder), plus a keyboard loop that maps
//! Escape / Enter / Tab / Backspace / typed characters onto a `String` buffer.
//! This module centralizes the three shareable pieces:
//!
//! - [`spawn_text_field`]: the label + box + value-text spawn tree, styled via
//!   [`TextFieldStyle`] and rendered from a [`TextFieldVisual`].
//! - [`apply_text_edit`]: the pure per-event keyboard edit core. Call sites
//!   keep their own focus state machines and interpret the returned
//!   [`TextEditOutcome`] (e.g. Tab-cycling, modal confirm).
//! - [`drive_inline_edit_keyboard`]: the full keyboard loop shared by the
//!   editor's inline click-to-edit buffers ([`InlineEditState`]), including the
//!   "Escape clears an armed palette pick" preamble.

use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::ui::theme::Palette;

// ── Keyboard edit core ────────────────────────────────────────────────────────

/// What a single key event meant for a focused text field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEditOutcome {
    /// The event was not a text-field key (or the characters were filtered).
    Ignored,
    /// Backspace or typed characters changed the buffer.
    Edited,
    /// Escape — the caller should cancel the edit / close the dialog.
    Cancel,
    /// Enter — the caller should submit / confirm.
    Submit,
    /// Tab — the caller should move focus to the next field.
    Next,
}

/// Which typed characters a field accepts.
#[derive(Clone, Copy)]
pub enum CharPolicy {
    /// Insert printable characters from `Key::Character`, dropping control
    /// chars individually. The dedicated Space key is ignored (title-screen
    /// login / direct-connect behavior).
    PrintableChars,
    /// Insert `Key::Character` strings verbatim and the Space key as `' '`.
    Raw,
    /// Insert `Key::Character` / Space only when *every* char satisfies the
    /// predicate; otherwise the whole event is dropped.
    AllChars(fn(char) -> bool),
}

impl CharPolicy {
    /// ASCII digits only (generic numeric modal fields).
    pub fn digits() -> Self {
        Self::AllChars(|c| c.is_ascii_digit())
    }

    /// Digits plus `.` and `-` (signed / fractional editor fields).
    pub fn signed_decimal() -> Self {
        Self::AllChars(|c| c.is_ascii_digit() || c == '.' || c == '-')
    }
}

/// Apply one *pressed* [`KeyboardInput`] event to `buffer`.
///
/// Backspace pops (key repeat honoured); typed characters are inserted per
/// `policy` (key repeat suppressed, matching every existing handler). Escape /
/// Enter / Tab never touch the buffer — they are classified and returned for
/// the caller's focus/submit state machine to act on.
pub fn apply_text_edit(
    buffer: &mut String,
    event: &KeyboardInput,
    policy: CharPolicy,
) -> TextEditOutcome {
    match event.key_code {
        KeyCode::Escape => TextEditOutcome::Cancel,
        KeyCode::Enter => TextEditOutcome::Submit,
        KeyCode::Tab => TextEditOutcome::Next,
        KeyCode::Backspace => {
            buffer.pop();
            TextEditOutcome::Edited
        }
        _ => {
            if event.repeat {
                return TextEditOutcome::Ignored;
            }
            match policy {
                CharPolicy::PrintableChars => {
                    let Key::Character(chars) = &event.logical_key else {
                        return TextEditOutcome::Ignored;
                    };
                    let mut edited = false;
                    for ch in chars.chars() {
                        if !ch.is_control() {
                            buffer.push(ch);
                            edited = true;
                        }
                    }
                    if edited {
                        TextEditOutcome::Edited
                    } else {
                        TextEditOutcome::Ignored
                    }
                }
                CharPolicy::Raw => match &event.logical_key {
                    Key::Character(chars) => {
                        buffer.push_str(chars.as_str());
                        TextEditOutcome::Edited
                    }
                    Key::Space => {
                        buffer.push(' ');
                        TextEditOutcome::Edited
                    }
                    _ => TextEditOutcome::Ignored,
                },
                CharPolicy::AllChars(allowed) => {
                    let chars = match &event.logical_key {
                        Key::Character(chars) => chars.as_str(),
                        Key::Space => " ",
                        _ => return TextEditOutcome::Ignored,
                    };
                    if chars.chars().all(allowed) {
                        buffer.push_str(chars);
                        TextEditOutcome::Edited
                    } else {
                        TextEditOutcome::Ignored
                    }
                }
            }
        }
    }
}

// ── Editor inline-edit keyboard loop ──────────────────────────────────────────

/// Editing state contract for editor panels that funnel keyboard input into an
/// inline click-to-edit buffer, optionally alongside an armed "pick from
/// palette" state that Escape must be able to clear.
pub trait InlineEditState {
    fn is_editing(&self) -> bool;
    fn edit_text_mut(&mut self) -> &mut String;
    /// Drop the active edit without committing (Escape).
    fn cancel_edit(&mut self);
    fn has_pending_pick(&self) -> bool {
        false
    }
    fn clear_pending_pick(&mut self) {}
}

/// Drain this frame's keyboard events into an inline edit buffer.
///
/// With no active edit, Escape clears an armed palette pick (if any) and
/// everything else is dropped. With an active edit: Escape cancels, Enter and
/// Tab call `commit`, Backspace / typed characters edit the buffer raw.
pub fn drive_inline_edit_keyboard<S: InlineEditState>(
    keyboard_events: &mut MessageReader<KeyboardInput>,
    state: &mut S,
    mut commit: impl FnMut(&mut S),
) {
    if !state.is_editing() {
        if state.has_pending_pick() {
            for event in keyboard_events.read() {
                if event.state.is_pressed() && event.key_code == KeyCode::Escape {
                    state.clear_pending_pick();
                }
            }
        }
        return;
    }
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match apply_text_edit(state.edit_text_mut(), event, CharPolicy::Raw) {
            TextEditOutcome::Cancel => state.cancel_edit(),
            TextEditOutcome::Submit | TextEditOutcome::Next => commit(state),
            TextEditOutcome::Edited | TextEditOutcome::Ignored => {}
        }
    }
}

/// Bracketed display for an in-progress inline edit (`[abc]`) — the editor
/// panels' equivalent of the modal fields' trailing `_` cursor.
pub fn inline_edit_display(edit_text: &str) -> String {
    format!("[{edit_text}]")
}

// ── Spawn tree ────────────────────────────────────────────────────────────────

/// Static styling for one family of text fields (sizes, padding, colors).
pub struct TextFieldStyle {
    /// Container width; `None` leaves the column auto-sized.
    pub width: Option<Val>,
    /// Container flex-grow (used by compact grid cells).
    pub flex_grow: f32,
    /// Gap between the label and the box.
    pub row_gap: f32,
    pub label_font_size: f32,
    pub label_color: Color,
    /// Fixed box height; when set the box also centers its value vertically.
    pub box_height: Option<Val>,
    pub box_padding: UiRect,
    pub box_background: Color,
    pub border_focused: Color,
    pub border_idle: Color,
    pub value_font_size: f32,
}

impl TextFieldStyle {
    /// Login / direct-connect fields on the title screen.
    pub fn title_screen(palette: &Palette) -> Self {
        Self {
            width: Some(Val::Percent(100.0)),
            flex_grow: 0.0,
            row_gap: 2.0,
            label_font_size: 14.0,
            label_color: palette.text_muted,
            box_height: Some(Val::Px(28.0)),
            box_padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
            box_background: Color::srgba(0.08, 0.08, 0.08, 0.65),
            border_focused: palette.border_accent,
            border_idle: palette.border_idle,
            value_font_size: 18.0,
        }
    }

    /// Standard editor-modal field (Open / Save As / New Map / Generate).
    pub fn modal(palette: &Palette) -> Self {
        Self {
            width: None,
            flex_grow: 0.0,
            row_gap: 3.0,
            label_font_size: 12.0,
            label_color: palette.text_muted,
            box_height: None,
            box_padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            box_background: Color::srgba(0.06, 0.04, 0.04, 0.90),
            border_focused: palette.border_focus,
            border_idle: palette.border_idle,
            value_font_size: 13.0,
        }
    }

    /// Full-width labeled row in the spawn-group / lighting-keyframe modals.
    pub fn editor_row(palette: &Palette) -> Self {
        Self {
            width: Some(Val::Percent(100.0)),
            flex_grow: 0.0,
            row_gap: 2.0,
            label_font_size: 11.0,
            label_color: palette.text_muted,
            box_height: None,
            box_padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            box_background: Color::srgba(0.06, 0.04, 0.04, 0.90),
            border_focused: palette.border_focus,
            border_idle: palette.border_idle,
            value_font_size: 12.0,
        }
    }

    /// Compact grid cell (rect min/max coordinates, RGB channels).
    pub fn editor_cell(palette: &Palette) -> Self {
        Self {
            width: None,
            flex_grow: 1.0,
            row_gap: 2.0,
            label_font_size: 9.0,
            label_color: palette.text_muted,
            box_height: None,
            box_padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
            box_background: Color::srgba(0.06, 0.04, 0.04, 0.90),
            border_focused: palette.border_focus,
            border_idle: palette.border_idle,
            value_font_size: 12.0,
        }
    }
}

/// Per-field render state: what to show inside the box right now.
pub struct TextFieldVisual<'a> {
    pub label: &'a str,
    /// The value to display (already masked for password fields).
    pub value: &'a str,
    pub focused: bool,
    /// Append a trailing `_` cursor while focused.
    pub show_cursor: bool,
    /// Shown (in `placeholder_color`) when the value is empty and unfocused.
    pub placeholder: &'a str,
    pub placeholder_color: Color,
    pub value_color: Color,
}

impl TextFieldVisual<'_> {
    fn display(&self) -> (String, Color) {
        if self.focused {
            let text = if self.show_cursor {
                format!("{}_", self.value)
            } else {
                self.value.to_owned()
            };
            (text, self.value_color)
        } else if self.value.is_empty() {
            (self.placeholder.to_owned(), self.placeholder_color)
        } else {
            (self.value.to_owned(), self.value_color)
        }
    }
}

/// Spawn `label` over a bordered click-to-focus box showing the field value.
///
/// `box_bundle` is inserted on the box entity (alongside `Button`) so callers
/// can attach their click-target / border markers; `text_bundle` goes on the
/// value `Text` node for sync systems (pass `()` when the tree is rebuilt on
/// change instead of synced).
pub fn spawn_text_field(
    parent: &mut ChildSpawnerCommands,
    style: &TextFieldStyle,
    visual: TextFieldVisual,
    box_bundle: impl Bundle,
    text_bundle: impl Bundle,
) {
    let (display, display_color) = visual.display();
    let border_color = if visual.focused {
        style.border_focused
    } else {
        style.border_idle
    };

    let mut container_node = Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(style.row_gap),
        flex_grow: style.flex_grow,
        ..default()
    };
    if let Some(width) = style.width {
        container_node.width = width;
    }

    let mut box_node = Node {
        width: Val::Percent(100.0),
        padding: style.box_padding,
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    };
    if let Some(height) = style.box_height {
        box_node.height = height;
        box_node.align_items = AlignItems::Center;
    }

    parent.spawn((container_node,)).with_children(|col| {
        col.spawn((
            Text::new(visual.label.to_owned()),
            TextFont {
                font_size: style.label_font_size,
                ..default()
            },
            TextColor(style.label_color),
        ));
        col.spawn((
            Button,
            box_bundle,
            box_node,
            BackgroundColor(style.box_background),
            BorderColor::all(border_color),
        ))
        .with_children(|input| {
            input.spawn((
                text_bundle,
                Text::new(display),
                TextFont {
                    font_size: style.value_font_size,
                    ..default()
                },
                TextColor(display_color),
            ));
        });
    });
}
