//! In-crate terminal-style text widget for Bevy: scrollable styled line
//! buffer with an optional editable input line, history, and a Tab-
//! completion event hook. Used by the project's Python console (read-write)
//! and chat panel (read-only).
//!
//! Consumers spawn a widget with [`spawn_terminal`], then either query the
//! [`Terminal`] component to push styled lines, or read [`TerminalSubmit`]
//! and [`TerminalCompletionRequest`] events from the widget's input line.
//! A single [`TerminalFocus`] resource decides which widget (if any) owns
//! the keyboard; outer gameplay systems can opt out via the exported
//! [`terminal_not_focused`] run condition.

mod systems;
mod textedit;
mod widget;

pub use systems::{
    terminal_blink_cursor, terminal_has_focus, terminal_input, terminal_not_focused,
    terminal_pin_scroll, terminal_sync_buffer, terminal_sync_input_line, terminal_wheel_input,
};
pub use textedit::{
    spawn_text_edit, spawn_text_edit_with, text_edit_blink_caret, text_edit_input, text_edit_sync,
    TextEdit, TextEditContent, TextEditPlugin, TextEditRoot, TextEditSubmit,
};
pub use widget::{
    spawn_terminal, LineStyle, Terminal, TerminalConfig, TerminalCursor, TerminalFocus,
    TerminalFocusId, TerminalInput, TerminalInputAfter, TerminalInputBefore, TerminalInputConfig,
    TerminalInputPrompt, TerminalInputRow, TerminalLine, TerminalRoot, TerminalTheme,
    TerminalViewport,
};

use bevy::ecs::message::Message;
use bevy::prelude::*;

pub struct TerminalWidgetPlugin;

impl Plugin for TerminalWidgetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerminalFocus>()
            .init_resource::<TerminalTheme>()
            .add_message::<TerminalSubmit>()
            .add_message::<TerminalCompletionRequest>()
            .add_systems(PreUpdate, (terminal_input, terminal_wheel_input))
            .add_systems(
                Update,
                (
                    terminal_sync_buffer,
                    terminal_sync_input_line,
                    terminal_blink_cursor,
                ),
            )
            .add_systems(
                PostUpdate,
                terminal_pin_scroll.after(bevy::ui::UiSystems::PostLayout),
            );
    }
}

/// Fired by the widget when the user submits the input line (presses Enter).
/// `text` is the raw input (newlines stripped). Consumers route by `terminal`.
#[derive(Message, Debug, Clone)]
pub struct TerminalSubmit {
    pub terminal: Entity,
    pub text: String,
}

/// Fired by the widget when the user presses Tab in an input line whose
/// config has `completion: true`. Consumers compute candidates from
/// `text_before_cursor` and either call [`Terminal::replace_input_token`]
/// or push a "candidates" line via [`Terminal::push`].
#[derive(Message, Debug, Clone)]
pub struct TerminalCompletionRequest {
    pub terminal: Entity,
    pub text_before_cursor: String,
}
