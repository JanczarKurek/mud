use std::collections::VecDeque;

use bevy::color::palettes::tailwind;
use bevy::prelude::*;
use bevy::ui::{Overflow, ScrollPosition};

/// Style/severity of a single line in the terminal buffer. Lines pick their
/// color from [`TerminalTheme::colors`]; the discriminant is used as the
/// array index, so the declaration order must match [`TerminalTheme::default`].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineStyle {
    #[default]
    Stdout,
    Stderr,
    Prompt,
    Traceback,
    System,
    ChatSay,
    ChatWhisper,
    ChatSystem,
}

impl LineStyle {
    pub const COUNT: usize = 8;
    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug)]
pub struct TerminalLine {
    pub text: String,
    pub style: LineStyle,
}

impl TerminalLine {
    pub fn new(text: impl Into<String>, style: LineStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

/// Identifier used to decide which terminal widget is currently focused.
/// The outer app picks IDs and stores them in [`TerminalFocus`] when the
/// user opens or clicks into a widget.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct TerminalFocusId(pub u32);

#[derive(Resource, Default, Debug)]
pub struct TerminalFocus {
    pub focused: Option<TerminalFocusId>,
}

#[derive(Resource, Clone, Debug)]
pub struct TerminalTheme {
    pub colors: [Color; LineStyle::COUNT],
    pub background: Color,
    pub prompt_color: Color,
    pub input_color: Color,
    pub cursor: Color,
    pub scrollbar_track: Color,
    pub scrollbar_thumb: Color,
    pub font_size: f32,
    pub line_gap: f32,
    /// Approximate per-character advance in pixels for the default font.
    /// Used to position the caret on the input line; not critical to be
    /// exact (caret will look slightly off-center for non-monospace fonts
    /// but stays within a few pixels of the right glyph).
    pub glyph_advance: f32,
}

impl Default for TerminalTheme {
    fn default() -> Self {
        let stdout = Color::srgb(0.88, 0.88, 0.88);
        let stderr = Color::Srgba(tailwind::RED_300);
        let prompt = Color::Srgba(tailwind::AMBER_300);
        let traceback = Color::Srgba(tailwind::RED_400);
        let system = Color::Srgba(tailwind::SLATE_400);
        let chat_say = Color::srgb(0.85, 0.85, 0.85);
        let chat_whisper = Color::Srgba(tailwind::INDIGO_300);
        let chat_system = Color::Srgba(tailwind::SLATE_400);

        let mut colors = [stdout; LineStyle::COUNT];
        colors[LineStyle::Stdout.index()] = stdout;
        colors[LineStyle::Stderr.index()] = stderr;
        colors[LineStyle::Prompt.index()] = prompt;
        colors[LineStyle::Traceback.index()] = traceback;
        colors[LineStyle::System.index()] = system;
        colors[LineStyle::ChatSay.index()] = chat_say;
        colors[LineStyle::ChatWhisper.index()] = chat_whisper;
        colors[LineStyle::ChatSystem.index()] = chat_system;

        Self {
            colors,
            background: Color::srgba(0.06, 0.07, 0.09, 0.96),
            prompt_color: prompt,
            input_color: Color::srgb(0.95, 0.95, 0.95),
            cursor: Color::srgba(0.95, 0.95, 0.95, 0.85),
            scrollbar_track: Color::srgba(0.15, 0.17, 0.20, 0.7),
            scrollbar_thumb: Color::srgba(0.45, 0.48, 0.55, 0.85),
            font_size: 16.0,
            line_gap: 2.0,
            glyph_advance: 8.6,
        }
    }
}

impl TerminalTheme {
    pub fn color_for(&self, style: LineStyle) -> Color {
        self.colors[style.index()]
    }
}

/// Per-instance configuration for the editable input line. `None` on
/// [`TerminalConfig::input`] means the widget is read-only (no input row).
#[derive(Clone, Debug, Default)]
pub struct TerminalInputConfig {
    pub prompt: String,
    pub completion: bool,
}

#[derive(Clone, Debug)]
pub struct TerminalConfig {
    pub initial_lines: Vec<TerminalLine>,
    pub capacity: usize,
    pub input: Option<TerminalInputConfig>,
    pub focus_id: TerminalFocusId,
    pub width: Val,
    pub height: Val,
    pub background: Option<Color>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            initial_lines: Vec::new(),
            capacity: 512,
            input: None,
            focus_id: TerminalFocusId(0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            background: None,
        }
    }
}

#[derive(Debug)]
pub struct TerminalInput {
    pub enabled: bool,
    pub prompt: String,
    pub buffer: String,
    /// Cursor position as a *char* index into `buffer`.
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub completion: bool,
}

impl TerminalInput {
    fn from_config(cfg: Option<TerminalInputConfig>) -> Self {
        match cfg {
            Some(cfg) => Self {
                enabled: true,
                prompt: cfg.prompt,
                buffer: String::new(),
                cursor: 0,
                history: Vec::new(),
                history_index: None,
                completion: cfg.completion,
            },
            None => Self {
                enabled: false,
                prompt: String::new(),
                buffer: String::new(),
                cursor: 0,
                history: Vec::new(),
                history_index: None,
                completion: false,
            },
        }
    }

    pub fn char_len(&self) -> usize {
        self.buffer.chars().count()
    }

    fn byte_offset(&self, char_idx: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.buffer.len())
    }

    pub fn insert_str(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        let byte = self.byte_offset(self.cursor);
        self.buffer.insert_str(byte, value);
        self.cursor += value.chars().count();
        self.history_index = None;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_byte = self.byte_offset(self.cursor - 1);
        let cur_byte = self.byte_offset(self.cursor);
        self.buffer.drain(prev_byte..cur_byte);
        self.cursor -= 1;
        self.history_index = None;
    }

    pub fn delete(&mut self) {
        let len = self.char_len();
        if self.cursor >= len {
            return;
        }
        let from = self.byte_offset(self.cursor);
        let to = self.byte_offset(self.cursor + 1);
        self.buffer.drain(from..to);
        self.history_index = None;
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        let len = self.char_len();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.char_len();
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_index {
            Some(0) => 0,
            Some(i) => i - 1,
            None => self.history.len() - 1,
        };
        self.history_index = Some(next);
        self.buffer = self.history[next].clone();
        self.cursor = self.char_len();
    }

    pub fn history_down(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        let next = idx + 1;
        if next >= self.history.len() {
            self.history_index = None;
            self.buffer.clear();
            self.cursor = 0;
            return;
        }
        self.history_index = Some(next);
        self.buffer = self.history[next].clone();
        self.cursor = self.char_len();
    }

    pub fn submit(&mut self) -> String {
        let submitted = std::mem::take(&mut self.buffer);
        self.cursor = 0;
        self.history_index = None;
        if !submitted.is_empty() {
            self.history.push(submitted.clone());
        }
        submitted
    }

    pub fn text_before_cursor(&self) -> String {
        let byte = self.byte_offset(self.cursor);
        self.buffer[..byte].to_owned()
    }
}

/// Main widget component. Mutating `buffer` (e.g. via [`Terminal::push`])
/// bumps `revision`, which the sync system uses to trigger a re-render.
#[derive(Component, Debug)]
pub struct Terminal {
    pub buffer: VecDeque<TerminalLine>,
    pub capacity: usize,
    pub input: TerminalInput,
    pub focus_id: TerminalFocusId,
    /// Set to `true` whenever new content is pushed; the post-layout system
    /// pins the viewport to the bottom and clears the flag.
    pub auto_pin_bottom: bool,
    pub revision: u64,
    pub(crate) last_rendered_revision: u64,
    pub(crate) last_input_revision: u64,
    pub(crate) input_revision: u64,
}

impl Terminal {
    pub fn push(&mut self, text: impl Into<String>, style: LineStyle) {
        let text = text.into();
        for segment in text.split('\n') {
            self.buffer
                .push_back(TerminalLine::new(segment.to_owned(), style));
        }
        while self.buffer.len() > self.capacity {
            self.buffer.pop_front();
        }
        self.revision = self.revision.wrapping_add(1);
        self.auto_pin_bottom = true;
    }

    pub fn push_lines<I>(&mut self, lines: I)
    where
        I: IntoIterator<Item = (String, LineStyle)>,
    {
        for (text, style) in lines {
            self.push(text, style);
        }
    }

    pub fn clear(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        self.buffer.clear();
        self.revision = self.revision.wrapping_add(1);
        self.auto_pin_bottom = true;
    }

    pub fn set_input(&mut self, text: String) {
        if !self.input.enabled {
            return;
        }
        let cursor = text.chars().count();
        self.input.buffer = text;
        self.input.cursor = cursor;
        self.input.history_index = None;
        self.input_revision = self.input_revision.wrapping_add(1);
    }

    /// Replace the last `prefix_len` chars before the cursor with
    /// `replacement`. Used by completion handlers.
    pub fn replace_input_token(&mut self, prefix_len: usize, replacement: &str) {
        if !self.input.enabled {
            return;
        }
        let start_char = self.input.cursor.saturating_sub(prefix_len);
        let start_byte = self.input.byte_offset(start_char);
        let end_byte = self.input.byte_offset(self.input.cursor);
        self.input.buffer.drain(start_byte..end_byte);
        self.input.buffer.insert_str(start_byte, replacement);
        self.input.cursor = start_char + replacement.chars().count();
        self.input.history_index = None;
        self.input_revision = self.input_revision.wrapping_add(1);
    }

    pub(crate) fn bump_input(&mut self) {
        self.input_revision = self.input_revision.wrapping_add(1);
    }
}

#[derive(Component)]
pub struct TerminalRoot {
    pub focus_id: TerminalFocusId,
}

#[derive(Component)]
pub struct TerminalViewport;

#[derive(Component)]
pub struct TerminalContent;

#[derive(Component)]
pub struct TerminalInputRow;

/// Prompt segment of the input row (e.g. `">>> "`). Static after spawn.
#[derive(Component)]
pub struct TerminalInputPrompt;

/// The portion of the user's buffer *before* the caret. Updated each frame.
#[derive(Component)]
pub struct TerminalInputBefore;

/// The portion of the user's buffer *after* the caret. Updated each frame.
#[derive(Component)]
pub struct TerminalInputAfter;

#[derive(Component)]
pub struct TerminalCursor;

/// Spawn a terminal widget rooted at a new entity and return it. The
/// caller normally inserts an outer-marker component (e.g.
/// `PythonConsoleTerminal`) on the returned entity for query
/// disambiguation.
pub fn spawn_terminal(commands: &mut Commands, cfg: TerminalConfig) -> Entity {
    let TerminalConfig {
        initial_lines,
        capacity,
        input,
        focus_id,
        width,
        height,
        background,
    } = cfg;

    let mut buffer = VecDeque::with_capacity(capacity);
    for line in initial_lines {
        buffer.push_back(line);
    }
    let has_input = input.is_some();
    let input_state = TerminalInput::from_config(input);

    let root = commands
        .spawn((
            Node {
                width,
                height,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(background.unwrap_or(Color::NONE)),
            TerminalRoot { focus_id },
            Terminal {
                buffer,
                capacity,
                input: input_state,
                focus_id,
                auto_pin_bottom: true,
                revision: 1,
                last_rendered_revision: 0,
                last_input_revision: 0,
                input_revision: 1,
            },
        ))
        .id();

    commands.entity(root).with_children(|root_children| {
        // Row holding viewport + scrollbar.
        root_children
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|row| {
                row.spawn((
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                        overflow: Overflow::scroll_y(),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    ScrollPosition::default(),
                    TerminalViewport,
                ))
                .with_children(|viewport| {
                    viewport.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        TerminalContent,
                    ));
                });

                // Static scrollbar track. The thumb position is currently
                // driven by Bevy's own scroll position via the viewport's
                // ScrollPosition; we don't sync a manual thumb in v1 — the
                // viewport handles wheel input and clipping. This narrow
                // track is purely a visual gutter.
                row.spawn((
                    Node {
                        width: Val::Px(6.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ));
            });

        if has_input {
            root_children
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(28.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        flex_direction: FlexDirection::Row,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                    TerminalInputRow,
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(""),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        TerminalInputPrompt,
                    ));
                    row.spawn((
                        Text::new(""),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        TerminalInputBefore,
                    ));
                    row.spawn((
                        Node {
                            width: Val::Px(2.0),
                            height: Val::Px(18.0),
                            ..default()
                        },
                        BackgroundColor(Color::WHITE),
                        TerminalCursor,
                    ));
                    row.spawn((
                        Text::new(""),
                        TextLayout::new(Justify::Left, LineBreak::NoWrap),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        TerminalInputAfter,
                    ));
                });
        }
    });

    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_terminal(input: bool) -> Terminal {
        Terminal {
            buffer: VecDeque::new(),
            capacity: 4,
            input: TerminalInput::from_config(input.then(TerminalInputConfig::default)),
            focus_id: TerminalFocusId(0),
            auto_pin_bottom: false,
            revision: 0,
            last_rendered_revision: 0,
            last_input_revision: 0,
            input_revision: 0,
        }
    }

    #[test]
    fn push_splits_on_newlines_and_enforces_capacity() {
        let mut t = make_terminal(false);
        t.push("a\nb\nc\nd\ne", LineStyle::Stdout);
        assert_eq!(t.buffer.len(), 4);
        assert_eq!(t.buffer.front().unwrap().text, "b");
        assert_eq!(t.buffer.back().unwrap().text, "e");
        assert!(t.auto_pin_bottom);
        assert!(t.revision > 0);
    }

    #[test]
    fn input_cursor_insert_and_delete() {
        let mut t = make_terminal(true);
        t.input.insert_str("hello");
        assert_eq!(t.input.buffer, "hello");
        assert_eq!(t.input.cursor, 5);
        t.input.move_home();
        t.input.insert_str(">");
        assert_eq!(t.input.buffer, ">hello");
        assert_eq!(t.input.cursor, 1);
        t.input.move_end();
        t.input.backspace();
        assert_eq!(t.input.buffer, ">hell");
        t.input.move_home();
        t.input.delete();
        assert_eq!(t.input.buffer, "hell");
    }

    #[test]
    fn input_cursor_handles_multibyte_chars() {
        let mut t = make_terminal(true);
        t.input.insert_str("héllo");
        assert_eq!(t.input.cursor, 5);
        t.input.move_home();
        t.input.move_right();
        t.input.delete();
        assert_eq!(t.input.buffer, "hllo");
    }

    #[test]
    fn history_browsing() {
        let mut t = make_terminal(true);
        t.input.history.push("first".into());
        t.input.history.push("second".into());
        t.input.history_up();
        assert_eq!(t.input.buffer, "second");
        t.input.history_up();
        assert_eq!(t.input.buffer, "first");
        t.input.history_up();
        assert_eq!(t.input.buffer, "first");
        t.input.history_down();
        assert_eq!(t.input.buffer, "second");
        t.input.history_down();
        assert_eq!(t.input.buffer, "");
    }

    #[test]
    fn replace_input_token_swaps_prefix() {
        let mut t = make_terminal(true);
        t.input.insert_str("wor");
        t.replace_input_token(3, "world");
        assert_eq!(t.input.buffer, "world");
        assert_eq!(t.input.cursor, 5);
    }
}
