//! Minimal multi-line text editor widget.
//!
//! Buffer is `Vec<String>` (one entry per line); caret is `(line, col)` in
//! character indices. Sibling to [`Terminal`](crate::Terminal) — shares the
//! [`TerminalFocus`](crate::TerminalFocus) resource so only one widget consumes
//! keys at a time.
//!
//! Keybindings (when focused):
//! - arrows: caret movement
//! - home/end: line start / end
//! - backspace / delete: erase a char (joining adjacent lines when needed)
//! - enter: insert a newline
//! - ctrl+enter: emit [`TextEditSubmit`]
//! - escape: clear focus (handled by the outer app; the widget itself
//!   doesn't trample focus)
//! - any other character key: insert
//!
//! Click-to-focus is owned by the outer app: set
//! `TerminalFocus::focused = Some(<text_edit_id>)` when the user clicks
//! the widget. The widget renders a caret while focused.

use std::time::Duration;

use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::prelude::*;
use bevy::time::Time;

use crate::widget::{TerminalFocus, TerminalFocusId};

#[derive(Component, Debug)]
pub struct TextEdit {
    pub lines: Vec<String>,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub focus_id: TerminalFocusId,
    pub font_size: f32,
    /// When true, plain Enter emits [`TextEditSubmit`] instead of inserting
    /// a newline. Use for single-line input fields like titles.
    pub single_line: bool,
    pub revision: u64,
    pub(crate) last_rendered_revision: u64,
}

impl TextEdit {
    pub fn new(focus_id: TerminalFocusId, font_size: f32) -> Self {
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
            focus_id,
            font_size,
            single_line: false,
            revision: 1,
            last_rendered_revision: 0,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(str::to_owned).collect()
        };
        // Cap the caret to the new buffer.
        self.cursor_line = self.lines.len() - 1;
        self.cursor_col = char_len(&self.lines[self.cursor_line]);
        self.bump();
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.is_empty())
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        for (i, segment) in text.split('\n').enumerate() {
            if i > 0 {
                self.insert_newline();
            }
            if !segment.is_empty() {
                self.insert_inline(segment);
            }
        }
    }

    pub fn insert_inline(&mut self, segment: &str) {
        let line = &mut self.lines[self.cursor_line];
        let byte = char_byte_offset(line, self.cursor_col);
        line.insert_str(byte, segment);
        self.cursor_col += segment.chars().count();
        self.bump();
    }

    pub fn insert_newline(&mut self) {
        let line = &mut self.lines[self.cursor_line];
        let byte = char_byte_offset(line, self.cursor_col);
        let tail = line.split_off(byte);
        self.lines.insert(self.cursor_line + 1, tail);
        self.cursor_line += 1;
        self.cursor_col = 0;
        self.bump();
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_line];
            let prev_byte = char_byte_offset(line, self.cursor_col - 1);
            let curr_byte = char_byte_offset(line, self.cursor_col);
            line.drain(prev_byte..curr_byte);
            self.cursor_col -= 1;
            self.bump();
        } else if self.cursor_line > 0 {
            let prev_len = char_len(&self.lines[self.cursor_line - 1]);
            let current = self.lines.remove(self.cursor_line);
            self.lines[self.cursor_line - 1].push_str(&current);
            self.cursor_line -= 1;
            self.cursor_col = prev_len;
            self.bump();
        }
    }

    pub fn delete_forward(&mut self) {
        let line_len = char_len(&self.lines[self.cursor_line]);
        if self.cursor_col < line_len {
            let line = &mut self.lines[self.cursor_line];
            let from = char_byte_offset(line, self.cursor_col);
            let to = char_byte_offset(line, self.cursor_col + 1);
            line.drain(from..to);
            self.bump();
        } else if self.cursor_line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&next);
            self.bump();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = char_len(&self.lines[self.cursor_line]);
        }
        self.bump();
    }

    pub fn move_right(&mut self) {
        let line_len = char_len(&self.lines[self.cursor_line]);
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
        self.bump();
    }

    pub fn move_up(&mut self) {
        if self.cursor_line == 0 {
            self.cursor_col = 0;
        } else {
            self.cursor_line -= 1;
            self.cursor_col = self.cursor_col.min(char_len(&self.lines[self.cursor_line]));
        }
        self.bump();
    }

    pub fn move_down(&mut self) {
        if self.cursor_line + 1 >= self.lines.len() {
            self.cursor_col = char_len(&self.lines[self.cursor_line]);
        } else {
            self.cursor_line += 1;
            self.cursor_col = self.cursor_col.min(char_len(&self.lines[self.cursor_line]));
        }
        self.bump();
    }

    pub fn move_home(&mut self) {
        self.cursor_col = 0;
        self.bump();
    }

    pub fn move_end(&mut self) {
        self.cursor_col = char_len(&self.lines[self.cursor_line]);
        self.bump();
    }

    pub(crate) fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

fn char_len(line: &str) -> usize {
    line.chars().count()
}

fn char_byte_offset(line: &str, char_idx: usize) -> usize {
    line.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(line.len())
}

/// Marker on the root entity of a text-edit widget. Lets the sync system
/// resolve back from a focus id to the right root.
#[derive(Component)]
pub struct TextEditRoot {
    pub focus_id: TerminalFocusId,
}

/// Marker on the inner text-content node (rendered text lives here).
#[derive(Component)]
pub struct TextEditContent;

/// Fired when the user presses Ctrl+Enter inside a focused text edit.
/// Consumers route by `text_edit` entity. The widget itself does **not**
/// clear its buffer or focus after submit — that's the consumer's call.
#[derive(bevy::ecs::message::Message, Debug, Clone)]
pub struct TextEditSubmit {
    pub text_edit: Entity,
    pub text: String,
}

/// Spawn the widget rooted at a new entity and return it. The caller is
/// expected to parent it under a UI ancestor (the parent decides size and
/// position). The widget itself sets `width: 100%`, `height: 100%` and
/// fills whatever box it lives in.
pub fn spawn_text_edit(
    commands: &mut Commands,
    focus_id: TerminalFocusId,
    initial_text: &str,
    font_size: f32,
) -> Entity {
    spawn_text_edit_with(commands, focus_id, initial_text, font_size, false)
}

/// Same as [`spawn_text_edit`] but with `single_line` configurable. When
/// `single_line` is true, plain Enter submits instead of inserting a
/// newline — use for one-line input fields like titles.
pub fn spawn_text_edit_with(
    commands: &mut Commands,
    focus_id: TerminalFocusId,
    initial_text: &str,
    font_size: f32,
    single_line: bool,
) -> Entity {
    let mut state = TextEdit::new(focus_id, font_size);
    state.single_line = single_line;
    state.set_text(initial_text);

    let root = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                height: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(6.0)),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.5)),
            TextEditRoot { focus_id },
            state,
            Button, // so the outer click-to-focus handler can hit it
        ))
        .id();

    commands.entity(root).with_children(|root_children| {
        root_children.spawn((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::NONE),
            TextEditContent,
        ));
    });

    root
}

pub struct TextEditPlugin;

impl Plugin for TextEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<TextEditSubmit>()
            .add_systems(PreUpdate, text_edit_input)
            .add_systems(Update, (text_edit_sync, text_edit_blink_caret));
    }
}

/// Drain keyboard events into the focused text edit. Runs in `PreUpdate`
/// alongside `terminal_input`. Note: terminal and text-edit widgets share
/// `TerminalFocus`, so they can never both be focused simultaneously.
#[allow(clippy::too_many_arguments)]
pub fn text_edit_input(
    mut focus: ResMut<TerminalFocus>,
    mut key_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut submit_events: bevy::ecs::message::MessageWriter<TextEditSubmit>,
    mut text_edits: Query<(Entity, &TextEditRoot, &mut TextEdit)>,
) {
    let absorbed_key = focus.absorbed_key;
    let Some(focused_id) = focus.focused else {
        return;
    };

    let mut target = None;
    for (entity, root, state) in text_edits.iter_mut() {
        if root.focus_id == focused_id {
            target = Some((entity, state));
            break;
        }
    }
    let Some((root_entity, mut state)) = target else {
        return;
    };
    // Take the absorbed key now that we know we matched a widget.
    focus.absorbed_key = None;

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        if Some(event.key_code) == absorbed_key {
            continue;
        }
        match event.key_code {
            KeyCode::Enter => {
                if ctrl || state.single_line {
                    submit_events.write(TextEditSubmit {
                        text_edit: root_entity,
                        text: state.text(),
                    });
                } else {
                    state.insert_newline();
                }
            }
            KeyCode::Backspace => state.backspace(),
            KeyCode::Delete => state.delete_forward(),
            KeyCode::ArrowLeft => state.move_left(),
            KeyCode::ArrowRight => state.move_right(),
            KeyCode::ArrowUp => state.move_up(),
            KeyCode::ArrowDown => state.move_down(),
            KeyCode::Home => state.move_home(),
            KeyCode::End => state.move_end(),
            _ => {
                if event.repeat && matches!(event.logical_key, Key::Character(_)) {
                    continue;
                }
                match &event.logical_key {
                    Key::Character(c) => state.insert_str(c.as_str()),
                    Key::Space => state.insert_str(" "),
                    _ => {}
                }
            }
        }
    }
}

/// Rebuild the rendered text lines when the buffer revision advances.
pub fn text_edit_sync(
    mut commands: Commands,
    focus: Res<TerminalFocus>,
    mut text_edits: Query<(Entity, &TextEditRoot, &mut TextEdit)>,
    content_q: Query<(Entity, &ChildOf), With<TextEditContent>>,
) {
    for (root_entity, root, mut state) in &mut text_edits {
        let focused_here = focus.focused == Some(root.focus_id);
        // Re-render whenever the buffer changed OR the focus state flipped
        // (so the caret marker appears / disappears).
        let want_revision = state
            .revision
            .wrapping_add(if focused_here { 1 } else { 0 });
        if want_revision == state.last_rendered_revision {
            continue;
        }
        let Some(content_entity) = content_q
            .iter()
            .find(|(_, child_of)| child_of.0 == root_entity)
            .map(|(e, _)| e)
        else {
            continue;
        };
        state.last_rendered_revision = want_revision;

        let font_size = state.font_size;
        let lines: Vec<String> = if focused_here {
            // Insert an ASCII pipe '|' at the caret position. Bevy's
            // default font ships without box-drawing glyphs, so any
            // U+2502-style vertical bar renders as a missing-glyph
            // tofu. Pipe is in every font.
            state
                .lines
                .iter()
                .enumerate()
                .map(|(line_idx, line)| {
                    if line_idx == state.cursor_line {
                        let byte = char_byte_offset(line, state.cursor_col);
                        let mut s = String::with_capacity(line.len() + 1);
                        s.push_str(&line[..byte]);
                        s.push('|');
                        s.push_str(&line[byte..]);
                        s
                    } else {
                        line.clone()
                    }
                })
                .collect()
        } else {
            state.lines.clone()
        };

        commands
            .entity(content_entity)
            .despawn_related::<Children>();
        commands.entity(content_entity).with_children(move |c| {
            for text in lines {
                c.spawn((
                    Text::new(if text.is_empty() {
                        " ".to_owned()
                    } else {
                        text
                    }),
                    TextLayout::new(Justify::Left, LineBreak::WordBoundary),
                    TextFont {
                        font_size,
                        ..default()
                    },
                    TextColor(Color::srgb(0.92, 0.92, 0.92)),
                    Node {
                        // Width pinned to the parent. `min_width: 0`
                        // overrides flex's default `min-width: auto`,
                        // which would otherwise let the longest token
                        // push the column (and the editor) wider as
                        // the user types — that's the visible
                        // "box gets bigger then unbreaks" oscillation.
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        ..default()
                    },
                ));
            }
        });
    }
}

/// Force a re-render every 500ms so the caret blinks. Cheap when the
/// buffer isn't changing because we only despawn/respawn children of the
/// focused widget.
pub fn text_edit_blink_caret(
    time: Res<Time>,
    focus: Res<TerminalFocus>,
    mut blink: Local<BlinkState>,
    mut text_edits: Query<(&TextEditRoot, &mut TextEdit)>,
) {
    if blink.timer.duration().is_zero() {
        blink.timer = Timer::new(Duration::from_millis(500), TimerMode::Repeating);
    }
    if !blink.timer.tick(time.delta()).just_finished() {
        return;
    }
    let Some(id) = focus.focused else {
        return;
    };
    for (root, mut state) in &mut text_edits {
        if root.focus_id == id {
            state.bump();
        }
    }
}

#[derive(Default)]
pub struct BlinkState {
    timer: Timer,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> TextEdit {
        TextEdit::new(TerminalFocusId(99), 14.0)
    }

    #[test]
    fn insert_str_appends_in_place() {
        let mut t = fresh();
        t.insert_str("hello");
        assert_eq!(t.text(), "hello");
        assert_eq!((t.cursor_line, t.cursor_col), (0, 5));
    }

    #[test]
    fn newline_splits_line() {
        let mut t = fresh();
        t.insert_str("abc");
        t.move_left();
        t.insert_newline();
        assert_eq!(t.lines, vec!["ab".to_owned(), "c".to_owned()]);
        assert_eq!((t.cursor_line, t.cursor_col), (1, 0));
    }

    #[test]
    fn backspace_joins_lines() {
        let mut t = fresh();
        t.insert_str("first\nsecond");
        // caret at end of "second"
        t.move_home();
        t.backspace();
        assert_eq!(t.lines, vec!["firstsecond".to_owned()]);
        assert_eq!((t.cursor_line, t.cursor_col), (0, 5));
    }

    #[test]
    fn delete_forward_at_eol_joins() {
        let mut t = fresh();
        t.insert_str("first\nsecond");
        t.cursor_line = 0;
        t.cursor_col = char_len("first");
        t.delete_forward();
        assert_eq!(t.lines, vec!["firstsecond".to_owned()]);
    }

    #[test]
    fn arrow_up_clamps_column() {
        let mut t = fresh();
        t.insert_str("hi\nworld");
        // cursor at end of "world"
        t.move_up();
        assert_eq!((t.cursor_line, t.cursor_col), (0, 2));
    }

    #[test]
    fn set_text_resets_caret_to_end() {
        let mut t = fresh();
        t.set_text("alpha\nbeta");
        assert_eq!((t.cursor_line, t.cursor_col), (1, 4));
        assert_eq!(t.text(), "alpha\nbeta");
    }

    #[test]
    fn multibyte_chars_round_trip() {
        let mut t = fresh();
        t.insert_str("héllo");
        t.move_home();
        t.move_right();
        t.delete_forward();
        assert_eq!(t.text(), "hllo");
    }
}
