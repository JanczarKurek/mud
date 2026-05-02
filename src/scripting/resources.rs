use bevy::prelude::*;

const CONSOLE_VISIBLE_OUTPUT_LINES: usize = 12;
const CONSOLE_WRAP_WIDTH_CHARS: usize = 56;

#[derive(Resource)]
pub struct PythonConsoleState {
    pub is_open: bool,
    pub input: String,
    pub output_lines: Vec<String>,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub visible_output_lines: usize,
    pub scroll_offset: usize,
    pub max_stored_output_lines: usize,
}

impl Default for PythonConsoleState {
    fn default() -> Self {
        Self {
            is_open: false,
            input: String::new(),
            output_lines: vec![
                "[System] Press ` to toggle the Python console.".to_owned(),
                "[Hint] world.player(), world.objects(), world.spawn(type, x, y), world.give(type, n)."
                    .to_owned(),
            ],
            history: Vec::new(),
            history_index: None,
            visible_output_lines: CONSOLE_VISIBLE_OUTPUT_LINES,
            scroll_offset: 0,
            max_stored_output_lines: 512,
        }
    }
}

impl PythonConsoleState {
    pub fn push_output(&mut self, line: impl Into<String>) {
        let line = line.into();
        for segment in line.lines() {
            self.output_lines.push(segment.to_owned());
        }

        if self.output_lines.len() > self.max_stored_output_lines {
            let overflow = self.output_lines.len() - self.max_stored_output_lines;
            self.output_lines.drain(0..overflow);
        }

        self.scroll_offset = 0;
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let max_scroll = self.max_scroll_offset();
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn rendered_output(&self) -> String {
        let wrapped_lines = self.wrapped_output_lines();
        let total_lines = wrapped_lines.len();
        if total_lines == 0 {
            return String::new();
        }

        let end = total_lines.saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(self.visible_output_lines);
        wrapped_lines[start..end].join("\n")
    }

    pub fn scrollbar_metrics(&self) -> (f32, f32) {
        let total_lines = self.wrapped_output_lines().len();
        if total_lines == 0 || total_lines <= self.visible_output_lines {
            return (1.0, 0.0);
        }

        let total_lines = total_lines as f32;
        let visible_lines = self.visible_output_lines as f32;
        let max_scroll = self.max_scroll_offset() as f32;

        let thumb_fraction = (visible_lines / total_lines).clamp(0.12, 1.0);
        let progress = if max_scroll <= 0.0 {
            0.0
        } else {
            1.0 - (self.scroll_offset as f32 / max_scroll)
        };

        (thumb_fraction, progress.clamp(0.0, 1.0))
    }

    fn max_scroll_offset(&self) -> usize {
        self.wrapped_output_lines()
            .len()
            .saturating_sub(self.visible_output_lines)
    }

    fn wrapped_output_lines(&self) -> Vec<String> {
        let mut wrapped_lines = Vec::new();

        for line in &self.output_lines {
            wrap_line(line, CONSOLE_WRAP_WIDTH_CHARS, &mut wrapped_lines);
        }

        wrapped_lines
    }
}

fn wrap_line(line: &str, max_chars: usize, output: &mut Vec<String>) {
    if line.is_empty() {
        output.push(String::new());
        return;
    }

    let mut current = String::new();

    for word in line.split(' ') {
        if current.is_empty() {
            if word.chars().count() <= max_chars {
                current.push_str(word);
            } else {
                push_long_word(word, max_chars, output);
            }
            continue;
        }

        let prospective_len = current.chars().count() + 1 + word.chars().count();
        if prospective_len <= max_chars {
            current.push(' ');
            current.push_str(word);
            continue;
        }

        output.push(std::mem::take(&mut current));
        if word.chars().count() <= max_chars {
            current.push_str(word);
        } else {
            push_long_word(word, max_chars, output);
        }
    }

    if !current.is_empty() {
        output.push(current);
    }
}

fn push_long_word(word: &str, max_chars: usize, output: &mut Vec<String>) {
    let mut chunk = String::new();

    for character in word.chars() {
        chunk.push(character);
        if chunk.chars().count() >= max_chars {
            output.push(std::mem::take(&mut chunk));
        }
    }

    if !chunk.is_empty() {
        output.push(chunk);
    }
}
