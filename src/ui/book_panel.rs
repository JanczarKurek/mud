//! Book / tombstone / engraving reader-editor window. The server emits
//! `GameUiEvent::OpenBookPanel` with a `{ kind, title, text, author_name,
//! can_edit }` snapshot; `apply_game_ui_events` stores it in
//! `BookPanelState`; the lifecycle system spawns/despawns the
//! `MovableWindow` based on `is_open()`.
//!
//! Editing uses the shared `TextEdit` widget from `bevy_terminal` (same
//! mechanism as the log panel): clicking Edit installs a `TextEdit` in the
//! title slot (plus a body slot for books), `TerminalFocus` follows clicks,
//! and the widget itself renders the caret + grabs keyboard input. Gameplay
//! systems gated on `terminal_not_focused` automatically pause while the
//! user is typing.

use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::prelude::*;
use bevy_terminal::{spawn_text_edit_with, TerminalFocus, TextEdit, TextEditRoot, TextEditSubmit};

use crate::game::commands::{GameCommand, ItemReference};
use crate::game::resources::PendingGameCommands;
use crate::ui::movable_window::{
    spawn_movable_window, spawn_themed_close_button, val_to_px, MovableWindowDrag, MovableWindowId,
    MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::ui::{BOOK_BODY_FOCUS_ID, BOOK_TITLE_FOCUS_ID};
use crate::world::object_definitions::TextKind;

const DEFAULT_BOOK_SIZE: Vec2 = Vec2::new(440.0, 520.0);
const BOOK_TITLE_FIELD_MAX_CHARS: usize = 64;
const BOOK_BODY_FIELD_MAX_CHARS: usize = 4096;
const INSCRIPTION_FIELD_MAX_CHARS: usize = 32;

#[derive(Resource, Default)]
pub struct BookPanelState {
    pub source: Option<ItemReference>,
    pub kind: Option<TextKind>,
    /// Server-captured snapshot. Restored on Cancel.
    pub snapshot_title: String,
    pub snapshot_body: String,
    pub author_name: Option<String>,
    pub can_edit: bool,
    pub editing: bool,
    /// Bumped on every state change that should rebuild the panel layout
    /// (open / close / edit-mode toggle / save). Crucially *not* bumped
    /// while the user is typing — that's owned by the `TextEdit` widgets.
    pub revision: u64,
    /// Cached window placement so re-opening drops the next book where the
    /// user left this one.
    pub last_position: Option<Vec2>,
    pub last_size: Option<Vec2>,
}

impl BookPanelState {
    pub fn is_open(&self) -> bool {
        self.source.is_some()
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    pub fn open(
        &mut self,
        source: ItemReference,
        kind: TextKind,
        title: String,
        text: String,
        author_name: Option<String>,
        can_edit: bool,
    ) {
        self.source = Some(source);
        self.kind = Some(kind);
        self.snapshot_title = title;
        self.snapshot_body = text;
        self.author_name = author_name;
        self.can_edit = can_edit;
        self.editing = false;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn close(&mut self) {
        self.source = None;
        self.kind = None;
        self.snapshot_title.clear();
        self.snapshot_body.clear();
        self.author_name = None;
        self.can_edit = false;
        self.editing = false;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn start_editing(&mut self) {
        if !self.can_edit || self.editing {
            return;
        }
        self.editing = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn cancel_editing(&mut self) {
        if !self.editing {
            return;
        }
        self.editing = false;
        self.revision = self.revision.wrapping_add(1);
    }

    fn apply_saved(&mut self, title: String, body: String) {
        self.snapshot_title = title;
        self.snapshot_body = body;
        self.editing = false;
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Component)]
pub struct BookPanelRoot;

#[derive(Component)]
pub struct BookPanelCloseButton;

#[derive(Component)]
pub struct BookPanelEditButton;

#[derive(Component)]
pub struct BookPanelSaveButton;

#[derive(Component)]
pub struct BookPanelCancelButton;

/// Anchor entity for the title slot; `install_book_editors` swaps it for a
/// real single-line `TextEdit`. The two-step (anchor + install) idiom mirrors
/// `log_panel`: `spawn_text_edit` needs `Commands` and allocates its own
/// entity, so we can't do it from inside `with_children`.
#[derive(Component)]
pub struct BookPanelTitleEditorAnchor {
    initial_text: String,
}

/// Anchor entity for the body slot (books only). `install_book_editors`
/// promotes it into a multi-line `TextEdit`.
#[derive(Component)]
pub struct BookPanelBodyEditorAnchor {
    initial_text: String,
}

#[derive(Resource, Default)]
pub struct BookPanelRenderState {
    pub last_revision: u64,
}

#[allow(clippy::too_many_arguments)]
pub fn sync_book_window_lifecycle(
    mut commands: Commands,
    mut state: ResMut<BookPanelState>,
    mut render_state: ResMut<BookPanelRenderState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    existing: Query<(Entity, &Node), With<BookPanelRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.is_open();
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            let size = state.last_size.unwrap_or(DEFAULT_BOOK_SIZE);
            let pos = state.last_position.unwrap_or(Vec2::new(360.0, 120.0));
            let root = spawn_book_window(&mut commands, &theme, &palette, pos, size, &state);
            drag.focused = Some(root);
            // Force a rebuild on the next frame.
            render_state.last_revision = state.revision.wrapping_sub(1);
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

fn window_title_for(kind: TextKind) -> &'static str {
    match kind {
        TextKind::Book => "Book",
        TextKind::Tombstone => "Tombstone",
        TextKind::Engraving => "Engraving",
    }
}

fn spawn_book_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    position: Vec2,
    size: Vec2,
    state: &BookPanelState,
) -> Entity {
    let kind = state.kind.unwrap_or(TextKind::Book);
    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::Book,
        window_title_for(kind),
        size,
        position,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );

    commands
        .entity(spawned.root)
        .insert((BookPanelRoot, crate::ui::components::HudRoot));

    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_themed_close_button(bar, theme, BookPanelCloseButton);
    });

    spawned.root
}

fn spawn_panel_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    style: ButtonStyle,
    marker: M,
) {
    let (bg, border, text_color) = idle_colors(palette, style, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(style),
            marker,
            Node {
                min_width: px(72.0),
                min_height: px(28.0),
                padding: UiRect::axes(px(12.0), px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
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
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(text_color),
            ));
        });
}

/// Rebuild the movable-window body on revision change. Edit mode swaps
/// read-only `Text` nodes for `TextEdit` anchors that the install system
/// promotes on the same frame.
#[allow(clippy::too_many_arguments)]
pub fn sync_book_panel_body(
    mut render_state: ResMut<BookPanelRenderState>,
    state: Res<BookPanelState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    root_query: Query<Entity, With<BookPanelRoot>>,
    body_query: Query<(Entity, &crate::ui::movable_window::MovableWindowContent)>,
    mut commands: Commands,
) {
    if render_state.last_revision == state.revision {
        return;
    }
    let Ok(root) = root_query.single() else {
        return;
    };
    let Some(body_entity) = body_query
        .iter()
        .find(|(_, content)| content.owner == root)
        .map(|(e, _)| e)
    else {
        return;
    };
    render_state.last_revision = state.revision;

    let kind = state.kind.unwrap_or(TextKind::Book);
    let title_text = state.snapshot_title.clone();
    let body_text = state.snapshot_body.clone();
    let author_text = state
        .author_name
        .clone()
        .map(|name| format!("— Written by {name}"))
        .unwrap_or_default();
    let can_edit = state.can_edit;
    let is_engraving = matches!(kind, TextKind::Engraving);
    let editing = state.is_editing();
    let theme_owned = theme.clone();
    let palette_copy = *palette;

    commands.entity(body_entity).despawn_related::<Children>();
    commands.entity(body_entity).with_children(move |body| {
        // Title row.
        if editing {
            // Anchor — promoted to a real TextEdit on the same frame by
            // `install_book_editors`. Bordered to mirror the read-mode look.
            body.spawn((
                Node {
                    width: percent(100.0),
                    min_height: px(28.0),
                    margin: UiRect::bottom(px(6.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BorderColor::all(palette_copy.border_accent),
                BackgroundColor(palette_copy.surface_console_output),
                BookPanelTitleEditorAnchor {
                    initial_text: title_text.clone(),
                },
            ));
        } else {
            body.spawn((
                Node {
                    width: percent(100.0),
                    min_height: px(28.0),
                    padding: UiRect::all(px(6.0)),
                    margin: UiRect::bottom(px(6.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(palette_copy.surface_console_output),
                BorderColor::all(palette_copy.border_accent),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(if title_text.is_empty() {
                        "(no title)".to_owned()
                    } else {
                        title_text.clone()
                    }),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(palette_copy.text_primary),
                ));
            });
        }

        if !author_text.is_empty() && !editing {
            body.spawn((
                Text::new(author_text.clone()),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette_copy.text_value),
                Node {
                    margin: UiRect::bottom(px(6.0)),
                    ..default()
                },
            ));
        }

        // Body. Hidden for engravings (title doubles as the inscription).
        if !is_engraving {
            if editing {
                body.spawn((
                    Node {
                        width: percent(100.0),
                        flex_grow: 1.0,
                        min_height: px(0.0),
                        border: UiRect::all(px(1.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BorderColor::all(palette_copy.border_accent),
                    BackgroundColor(palette_copy.surface_console_output),
                    BookPanelBodyEditorAnchor {
                        initial_text: body_text.clone(),
                    },
                ));
            } else {
                body.spawn((
                    Node {
                        width: percent(100.0),
                        flex_grow: 1.0,
                        min_height: px(0.0),
                        padding: UiRect::all(px(8.0)),
                        border: UiRect::all(px(1.0)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(palette_copy.surface_console_output),
                    BorderColor::all(palette_copy.border_accent),
                ))
                .with_children(|inner| {
                    inner.spawn((
                        Text::new(if body_text.is_empty() {
                            "(blank)".to_owned()
                        } else {
                            body_text.clone()
                        }),
                        TextFont {
                            font_size: 15.0,
                            ..default()
                        },
                        TextColor(palette_copy.text_primary),
                        TextLayout::new(
                            bevy::text::Justify::Left,
                            bevy::text::LineBreak::WordBoundary,
                        ),
                    ));
                });
            }
        }

        // Buttons row.
        body.spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::FlexEnd,
            column_gap: px(8.0),
            margin: UiRect::top(px(8.0)),
            ..default()
        })
        .with_children(|row| {
            if editing {
                spawn_panel_button(
                    row,
                    &theme_owned,
                    &palette_copy,
                    "Cancel",
                    ButtonStyle::Secondary,
                    BookPanelCancelButton,
                );
                spawn_panel_button(
                    row,
                    &theme_owned,
                    &palette_copy,
                    "Save",
                    ButtonStyle::Primary,
                    BookPanelSaveButton,
                );
            } else if can_edit {
                spawn_panel_button(
                    row,
                    &theme_owned,
                    &palette_copy,
                    "Edit",
                    ButtonStyle::Primary,
                    BookPanelEditButton,
                );
            }
        });
    });
}

/// Promote freshly-spawned title/body anchors into real `TextEdit` widgets.
/// Runs on the same frame as `sync_book_panel_body` so the user never sees
/// the un-promoted anchor. Focus jumps to the title editor on install so the
/// caret appears immediately after the user clicks Edit.
pub fn install_book_editors(
    mut commands: Commands,
    title_anchors: Query<(Entity, &BookPanelTitleEditorAnchor), Added<BookPanelTitleEditorAnchor>>,
    body_anchors: Query<(Entity, &BookPanelBodyEditorAnchor), Added<BookPanelBodyEditorAnchor>>,
    mut focus: ResMut<TerminalFocus>,
) {
    for (entity, anchor) in &title_anchors {
        let initial = anchor.initial_text.clone();
        commands
            .entity(entity)
            .remove::<BookPanelTitleEditorAnchor>();
        let editor = spawn_text_edit_with(
            &mut commands,
            BOOK_TITLE_FOCUS_ID,
            initial.as_str(),
            18.0,
            true, // single-line: Enter submits → save
        );
        commands.entity(entity).add_child(editor);
        focus.focused = Some(BOOK_TITLE_FOCUS_ID);
    }
    for (entity, anchor) in &body_anchors {
        let initial = anchor.initial_text.clone();
        commands
            .entity(entity)
            .remove::<BookPanelBodyEditorAnchor>();
        let editor = spawn_text_edit_with(
            &mut commands,
            BOOK_BODY_FOCUS_ID,
            initial.as_str(),
            15.0,
            false, // multi-line: plain Enter inserts newline, Ctrl+Enter submits
        );
        commands.entity(entity).add_child(editor);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn handle_book_panel_clicks(
    mut state: ResMut<BookPanelState>,
    mut pending: ResMut<PendingGameCommands>,
    edit_q: Query<&Interaction, (Changed<Interaction>, With<BookPanelEditButton>)>,
    save_q: Query<&Interaction, (Changed<Interaction>, With<BookPanelSaveButton>)>,
    cancel_q: Query<&Interaction, (Changed<Interaction>, With<BookPanelCancelButton>)>,
    close_q: Query<&Interaction, (Changed<Interaction>, With<BookPanelCloseButton>)>,
    text_edits: Query<(&TextEditRoot, &TextEdit)>,
) {
    for interaction in &edit_q {
        if *interaction == Interaction::Pressed {
            state.start_editing();
        }
    }
    for interaction in &cancel_q {
        if *interaction == Interaction::Pressed {
            state.cancel_editing();
        }
    }
    for interaction in &save_q {
        if *interaction == Interaction::Pressed {
            submit_save(&mut state, &mut pending, &text_edits);
        }
    }
    for interaction in &close_q {
        if *interaction == Interaction::Pressed {
            state.close();
        }
    }
}

/// Save path shared between the Save button and Enter / Ctrl+Enter submits
/// from the `TextEdit` widgets. Truncates to per-field char limits and
/// pushes the appropriate `GameCommand`, then locks the panel back into
/// read mode (optimistic — the next ReadBook refresh corrects if the
/// server rejected).
fn submit_save(
    state: &mut BookPanelState,
    pending: &mut PendingGameCommands,
    text_edits: &Query<(&TextEditRoot, &TextEdit)>,
) {
    let Some(source) = state.source else {
        return;
    };
    let title_buffer = text_edits
        .iter()
        .find(|(r, _)| r.focus_id == BOOK_TITLE_FOCUS_ID)
        .map(|(_, te)| te.text())
        .unwrap_or_else(|| state.snapshot_title.clone());
    let body_buffer = text_edits
        .iter()
        .find(|(r, _)| r.focus_id == BOOK_BODY_FOCUS_ID)
        .map(|(_, te)| te.text())
        .unwrap_or_else(|| state.snapshot_body.clone());

    match state.kind {
        Some(TextKind::Book) => {
            let title = truncate_chars(&title_buffer, BOOK_TITLE_FIELD_MAX_CHARS);
            let text = truncate_chars(&body_buffer, BOOK_BODY_FIELD_MAX_CHARS);
            pending.push(GameCommand::WriteBook {
                source,
                title: title.clone(),
                text: text.clone(),
            });
            state.apply_saved(title, text);
        }
        Some(TextKind::Engraving) => {
            let inscription = truncate_chars(&title_buffer, INSCRIPTION_FIELD_MAX_CHARS);
            pending.push(GameCommand::Engrave {
                source,
                inscription: inscription.clone(),
            });
            // Engraving is one-shot — lock can_edit so the next render
            // hides the Edit button until the server confirms otherwise.
            state.apply_saved(inscription, state.snapshot_body.clone());
            state.can_edit = false;
        }
        _ => {}
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        s.chars().take(max_chars).collect()
    }
}

/// Save when the user submits from either editor (Enter on the single-line
/// title, Ctrl+Enter on the multi-line body). Same code path as the Save
/// button.
pub fn consume_book_text_edit_submits(
    mut submits: bevy::ecs::message::MessageReader<TextEditSubmit>,
    mut state: ResMut<BookPanelState>,
    mut pending: ResMut<PendingGameCommands>,
    text_edits: Query<(&TextEditRoot, &TextEdit)>,
) {
    let mut had_book_submit = false;
    for submit in submits.read() {
        if let Ok((te_root, _)) = text_edits.get(submit.text_edit) {
            if te_root.focus_id == BOOK_TITLE_FOCUS_ID || te_root.focus_id == BOOK_BODY_FOCUS_ID {
                had_book_submit = true;
            }
        }
    }
    if !had_book_submit {
        return;
    }
    submit_save(&mut state, &mut pending, &text_edits);
}

/// Click on a book `TextEdit` → focus it. Without this, keystrokes drop on
/// the floor after the user has clicked elsewhere on the panel.
pub fn handle_book_editor_focus_click(
    interactions: Query<(&Interaction, &TextEditRoot), Changed<Interaction>>,
    mut focus: ResMut<TerminalFocus>,
) {
    for (interaction, root) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if root.focus_id == BOOK_TITLE_FOCUS_ID || root.focus_id == BOOK_BODY_FOCUS_ID {
            focus.focused = Some(root.focus_id);
        }
    }
}

/// Escape from a focused book editor → cancel the edit and clear focus, so
/// gameplay hotkeys resume.
pub fn clear_book_editor_focus_on_escape(
    mut key_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    mut focus: ResMut<TerminalFocus>,
    mut state: ResMut<BookPanelState>,
) {
    let active = matches!(
        focus.focused,
        Some(BOOK_TITLE_FOCUS_ID) | Some(BOOK_BODY_FOCUS_ID)
    );
    if !active {
        return;
    }
    let mut esc = false;
    for event in key_events.read() {
        if event.state.is_pressed() && event.key_code == KeyCode::Escape {
            esc = true;
        }
    }
    if !esc {
        return;
    }
    focus.focused = None;
    state.cancel_editing();
}

/// Release book focus when the editor widgets no longer exist — either
/// because the panel is closed, or because the user left edit mode (Save /
/// Cancel despawns the `TextEdit` entities, but the focus id would
/// otherwise linger in `TerminalFocus` and keep gameplay hotkeys blocked).
pub fn release_book_focus_when_idle(
    mut focus: ResMut<TerminalFocus>,
    state: Res<BookPanelState>,
    panels: Query<(), With<BookPanelRoot>>,
) {
    let active = matches!(
        focus.focused,
        Some(BOOK_TITLE_FOCUS_ID) | Some(BOOK_BODY_FOCUS_ID)
    );
    if active && (panels.is_empty() || !state.is_editing()) {
        focus.focused = None;
    }
}
