//! Per-character Log panel: vertical bookmark tabs on the right, a list of
//! subentries in the left column, and an entry view (read-only body for
//! engine entries + an editable text-edit area) in the right column.
//!
//! Opens / closes on `KeyL`. The pattern matches `recipe_book.rs` — a
//! `MovableWindow` toggled by a hotkey, no docked variant.
//!
//! Layout is built once on open as a tree of stable "slots" (marker
//! components on container nodes). Rebuilds repopulate slot contents
//! without despawning the body's `TextEdit`, so click-to-focus, the
//! caret, and the in-progress edit buffer all survive ticks where the
//! `ClientGameState` resource changes (which is most of them).

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy_terminal::{
    spawn_text_edit, spawn_text_edit_with, TerminalFocus, TextEdit, TextEditRoot, TextEditSubmit,
};

use crate::app::state::{simulation_active, ClientAppState};
use crate::game::commands::GameCommand;
use crate::game::resources::{ClientGameState, PendingGameCommands};
use crate::log::{LogEntry, LogOwner, LogState, NOTES_SECTION, QUESTS_SECTION};
use crate::ui::components::HudRoot;
use crate::ui::movable_window::{
    find_window_by_id, spawn_movable_window, spawn_movable_window_close_button, MovableWindow,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::ui::{LOG_NOTES_FOCUS_ID, LOG_TITLE_FOCUS_ID};

const PANEL_SIZE: Vec2 = Vec2::new(560.0, 420.0);
const PANEL_INITIAL_POS: Vec2 = Vec2::new(140.0, 80.0);
const BOOKMARK_WIDTH: f32 = 110.0;
const SUBENTRY_LIST_WIDTH: f32 = 160.0;

/// Marker on the log window root. Holds the currently selected section /
/// subsection plus transient editor state (whether the title is in
/// click-to-edit mode, and which entry the body editor's buffer was last
/// seeded for).
#[derive(Component, Default, Clone, Debug)]
pub struct LogPanelRoot {
    pub selected_section: Option<String>,
    pub selected_subsection: Option<String>,
    /// Tracks the (section, subsection) that the body editor's buffer was
    /// last seeded for. Used to know when to re-seed across selections.
    pub editor_loaded_for: Option<(String, String)>,
    /// True while the user has clicked the title to edit it.
    pub editing_title: bool,
}

#[derive(Component)]
pub struct LogPanelBody;

// === Slot markers (stable parents that get repopulated in place) ===

#[derive(Component)]
struct LogPanelSubentryListSlot;

#[derive(Component)]
struct LogPanelBookmarkColumnSlot;

#[derive(Component)]
struct LogPanelEntryViewSlot;

#[derive(Component)]
struct LogPanelTitleSlot;

#[derive(Component)]
struct LogPanelBodyDisplaySlot;

#[derive(Component)]
struct LogPanelEditorSlot;

#[derive(Component)]
struct LogPanelButtonsSlot;

/// Sticks on the editor slot once we've installed the body `TextEdit` so
/// we don't double-spawn on subsequent frames.
#[derive(Component)]
struct LogPanelBodyEditorInstalled;

/// Marker for the title click-to-edit `TextEdit` so we can route Ctrl+Enter
/// submits and look up its buffer.
#[derive(Component)]
struct LogPanelTitleEditor;

/// Marker on the title `Text` button when not editing — distinguishes a
/// "click to edit" hit from any other text in the panel.
#[derive(Component)]
struct LogPanelTitleLabel;

#[derive(Component)]
pub struct LogPanelBookmark {
    pub section: String,
}

#[derive(Component)]
pub struct LogPanelSubentryButton {
    pub section: String,
    pub subsection: String,
}

#[derive(Component)]
pub struct LogPanelNewNoteButton;

#[derive(Component)]
pub struct LogPanelSaveButton;

#[derive(Component)]
pub struct LogPanelDeleteButton;

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LogPanelSystemSet {
    Process,
}

pub fn register(app: &mut App) {
    app.add_systems(
        Update,
        toggle_log_panel_on_keybind
            .run_if(in_state(ClientAppState::InGame))
            .run_if(simulation_active)
            .run_if(bevy_terminal::terminal_not_focused)
            .in_set(LogPanelSystemSet::Process),
    )
    .add_systems(
        Update,
        (
            install_body_editor,
            rebuild_log_panel_contents,
            install_title_editors,
            handle_bookmark_clicks,
            handle_subentry_clicks,
            handle_new_note_click,
            handle_save_click,
            handle_delete_click,
            handle_editor_focus_click,
            handle_title_click,
        )
            .chain()
            // Force text_edit_sync to observe the body editor's reseed
            // (and the freshly-installed title editor) on the same
            // frame as the slot rebuild — otherwise the reseeded text
            // lags one frame behind the rest of the panel and the user
            // sees a flash of the previous entry's text.
            .before(bevy_terminal::text_edit_sync)
            .in_set(LogPanelSystemSet::Process)
            .run_if(in_state(ClientAppState::InGame))
            .run_if(simulation_active),
    )
    .add_systems(
        Update,
        consume_text_edit_submits.run_if(in_state(ClientAppState::InGame)),
    )
    .add_systems(
        bevy::prelude::PreUpdate,
        clear_editor_focus_on_escape
            .before(bevy_terminal::text_edit_input)
            .run_if(in_state(ClientAppState::InGame)),
    )
    .add_systems(
        Update,
        release_focus_when_panel_gone.run_if(in_state(ClientAppState::InGame)),
    );
}

/// `KeyL` toggles the log panel — opens if closed, closes if open.
fn toggle_log_panel_on_keybind(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    theme: Option<Res<UiThemeAssets>>,
    palette: Option<Res<Palette>>,
    windows: Query<(Entity, &MovableWindow)>,
) {
    if !keyboard.just_pressed(KeyCode::KeyL) {
        return;
    }
    toggle_log_window(
        &mut commands,
        theme.as_deref(),
        palette.as_deref(),
        &windows,
    );
}

/// Toggle the log window — open if no window exists, otherwise despawn the
/// existing one. Shared between the `KeyL` hotkey and the View menu entry.
pub fn toggle_log_window(
    commands: &mut Commands,
    theme: Option<&UiThemeAssets>,
    palette: Option<&Palette>,
    windows: &Query<(Entity, &MovableWindow)>,
) {
    if let Some(existing) = find_window_by_id(windows, MovableWindowId::Log) {
        commands.entity(existing).despawn();
        return;
    }
    let Some(theme) = theme else {
        return;
    };
    let Some(palette) = palette else {
        return;
    };
    spawn_log_panel(commands, theme, palette);
}

fn spawn_log_panel(commands: &mut Commands, theme: &UiThemeAssets, palette: &Palette) {
    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::Log,
        "Log",
        PANEL_SIZE,
        PANEL_INITIAL_POS,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );
    commands
        .entity(spawned.root)
        .insert((LogPanelRoot::default(), HudRoot));
    commands.entity(spawned.body).insert(LogPanelBody);
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_movable_window_close_button(bar, theme, palette, spawned.root);
    });

    let palette = *palette;
    // `min_width: 0` and `overflow: clip` on every flex container in the
    // chain — without it, `min-width: auto` (CSS default for flex items)
    // lets a long unbreakable token in the body editor push the panel's
    // entry-view column wider, which in turn shoves the bookmark column
    // off the right edge of the panel. Once any flex item along the way
    // has `min-width: auto`, the constraint propagates outward.
    commands
        .entity(spawned.body)
        .insert(Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(8.0)),
            min_height: Val::Px(0.0),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|body| {
            // Left + center column container.
            body.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(8.0),
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|cols| {
                cols.spawn((
                    Node {
                        width: Val::Px(SUBENTRY_LIST_WIDTH),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        padding: UiRect::all(Val::Px(4.0)),
                        border: UiRect::right(Val::Px(1.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BorderColor::all(palette.border_slot),
                    BackgroundColor(palette.surface_panel),
                    LogPanelSubentryListSlot,
                ));

                cols.spawn((
                    Node {
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        padding: UiRect::all(Val::Px(4.0)),
                        min_width: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    LogPanelEntryViewSlot,
                ))
                .with_children(|view| {
                    view.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        LogPanelTitleSlot,
                    ));
                    view.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(4.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        LogPanelBodyDisplaySlot,
                    ));
                    view.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            flex_grow: 1.0,
                            min_height: Val::Px(80.0),
                            border: UiRect::all(Val::Px(1.0)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BorderColor::all(palette.border_slot),
                        BackgroundColor(palette.surface_raised),
                        LogPanelEditorSlot,
                    ));
                    view.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            column_gap: Val::Px(8.0),
                            flex_direction: FlexDirection::Row,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        LogPanelButtonsSlot,
                    ));
                });
            });

            body.spawn((
                Node {
                    width: Val::Px(BOOKMARK_WIDTH),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(4.0)),
                    border: UiRect::left(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(palette.border_slot),
                BackgroundColor(palette.surface_panel),
                LogPanelBookmarkColumnSlot,
            ));
        });
}

/// Spawn the body `TextEdit` once into its slot. Marker keeps us from
/// double-installing on subsequent frames. Runs ahead of the rebuild so
/// the rebuild's editor-buffer reseed always finds a target.
fn install_body_editor(
    mut commands: Commands,
    slots: Query<
        Entity,
        (
            With<LogPanelEditorSlot>,
            Without<LogPanelBodyEditorInstalled>,
        ),
    >,
) {
    for slot in &slots {
        let editor = spawn_text_edit(&mut commands, LOG_NOTES_FOCUS_ID, "", 14.0);
        commands.entity(slot).add_child(editor);
        commands.entity(slot).insert(LogPanelBodyEditorInstalled);
    }
}

/// Cheap-ish key for "did anything that affects what we render change?".
/// Tracks: which sections/subsections exist, the title shown in the
/// subentry list, the engine body text, and the engine player_notes
/// (since those are what the body editor can be reseeded from).
#[derive(PartialEq, Eq)]
struct RebuildSignature {
    selected_section: Option<String>,
    selected_subsection: Option<String>,
    editing_title: bool,
    sections: Vec<(String, Vec<(String, String, LogOwner, String, String)>)>,
}

fn compute_signature(log: &LogState, root: &LogPanelRoot) -> RebuildSignature {
    let mut sections = Vec::with_capacity(log.sections.len());
    for (name, section) in &log.sections {
        let mut subs = Vec::with_capacity(section.subsections.len());
        for (key, entry) in &section.subsections {
            subs.push((
                key.clone(),
                entry.title.clone(),
                entry.owner,
                entry.body.clone(),
                entry.player_notes.clone(),
            ));
        }
        sections.push((name.clone(), subs));
    }
    RebuildSignature {
        selected_section: root.selected_section.clone(),
        selected_subsection: root.selected_subsection.clone(),
        editing_title: root.editing_title,
        sections,
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_log_panel_contents(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    palette: Option<Res<Palette>>,
    mut roots: Query<(Entity, &mut LogPanelRoot)>,
    subentry_list_slots: Query<Entity, With<LogPanelSubentryListSlot>>,
    bookmark_column_slots: Query<Entity, With<LogPanelBookmarkColumnSlot>>,
    title_slots: Query<Entity, With<LogPanelTitleSlot>>,
    body_display_slots: Query<Entity, With<LogPanelBodyDisplaySlot>>,
    buttons_slots: Query<Entity, With<LogPanelButtonsSlot>>,
    body_editors: Query<(Entity, &TextEditRoot), Without<LogPanelTitleEditor>>,
    mut text_edits: Query<&mut TextEdit>,
    mut last_signature: Local<Option<RebuildSignature>>,
) {
    let Some(palette) = palette.as_deref() else {
        return;
    };
    let Ok((_root_entity, mut root)) = roots.single_mut() else {
        // Panel is closed — drop the cached signature so next open rebuilds.
        *last_signature = None;
        return;
    };

    // Make sure selection is consistent with the current data.
    let log = &client_state.log_state;
    let sections = section_list(log);
    if !sections.is_empty()
        && root
            .selected_section
            .as_ref()
            .is_none_or(|s| !sections.contains(s))
    {
        root.selected_section = Some(sections[0].clone());
        root.selected_subsection = None;
    }
    if let Some(section) = root.selected_section.clone() {
        if let Some(sub) = root.selected_subsection.clone() {
            if log.entry(&section, &sub).is_none() {
                root.selected_subsection = None;
            }
        }
        if root.selected_subsection.is_none() {
            if let Some(s) = log.section(&section) {
                if let Some(first) = s.subsections.keys().next() {
                    root.selected_subsection = Some(first.clone());
                }
            }
        }
    }

    let signature = compute_signature(log, &root);
    let needs_rebuild = last_signature.as_ref() != Some(&signature);
    if !needs_rebuild {
        return;
    }
    *last_signature = Some(signature);

    let current_entry = match (&root.selected_section, &root.selected_subsection) {
        (Some(section), Some(sub)) => log.entry(section, sub).cloned(),
        _ => None,
    };
    let current_key = root
        .selected_section
        .as_ref()
        .zip(root.selected_subsection.as_ref())
        .map(|(s, sub)| (s.clone(), sub.clone()));

    // Reseed the body editor when selection changes. The editor entity
    // itself is preserved across rebuilds, so this is the only path that
    // touches its buffer.
    if root.editor_loaded_for != current_key {
        let target_text = match current_entry.as_ref() {
            Some(entry) => match entry.owner {
                LogOwner::Engine => entry.player_notes.clone(),
                LogOwner::Player => entry.body.clone(),
            },
            None => String::new(),
        };
        for (te_entity, te_root) in body_editors.iter() {
            if te_root.focus_id == LOG_NOTES_FOCUS_ID {
                if let Ok(mut state) = text_edits.get_mut(te_entity) {
                    state.set_text(&target_text);
                }
            }
        }
        root.editor_loaded_for = current_key.clone();
        // Selection changed — drop title-edit mode without committing.
        root.editing_title = false;
    }

    let palette_copy = *palette;
    let log_clone = log.clone();
    let selected_section = root.selected_section.clone();
    let selected_subsection = root.selected_subsection.clone();
    let editing_title = root.editing_title;

    if let Ok(slot) = subentry_list_slots.single() {
        commands.entity(slot).despawn_related::<Children>();
        commands.entity(slot).with_children(|list| {
            populate_subentry_list(
                list,
                &palette_copy,
                selected_section.as_deref(),
                selected_subsection.as_deref(),
                &log_clone,
            );
        });
    }

    if let Ok(slot) = bookmark_column_slots.single() {
        commands.entity(slot).despawn_related::<Children>();
        commands.entity(slot).with_children(|col| {
            populate_bookmark_column(col, &palette_copy, selected_section.as_deref());
        });
    }

    if let Ok(slot) = title_slots.single() {
        commands.entity(slot).despawn_related::<Children>();
        let entry_clone = current_entry.clone();
        commands.entity(slot).with_children(|t| {
            populate_title_slot(t, &palette_copy, entry_clone.as_ref(), editing_title);
        });
    }

    if let Ok(slot) = body_display_slots.single() {
        commands.entity(slot).despawn_related::<Children>();
        let entry_clone = current_entry.clone();
        commands.entity(slot).with_children(|b| {
            populate_body_display(b, &palette_copy, entry_clone.as_ref());
        });
    }

    if let Ok(slot) = buttons_slots.single() {
        commands.entity(slot).despawn_related::<Children>();
        let entry_clone = current_entry.clone();
        commands.entity(slot).with_children(|row| {
            populate_buttons(row, &palette_copy, entry_clone.as_ref());
        });
    }
}

fn section_list(log: &LogState) -> Vec<String> {
    let mut sections: Vec<String> = log.sections.keys().cloned().collect();
    let pinned = [QUESTS_SECTION, NOTES_SECTION];
    let mut out: Vec<String> = pinned.iter().map(|s| s.to_string()).collect();
    for s in sections.drain(..) {
        if !pinned.contains(&s.as_str()) {
            out.push(s);
        }
    }
    out
}

fn populate_bookmark_column(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    selected_section: Option<&str>,
) {
    for section in [QUESTS_SECTION, NOTES_SECTION] {
        let is_selected = selected_section == Some(section);
        let bg = if is_selected {
            palette.surface_raised
        } else {
            palette.surface_panel
        };
        parent
            .spawn((
                Button,
                LogPanelBookmark {
                    section: section.to_owned(),
                },
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(bg),
                BorderColor::all(if is_selected {
                    palette.border_accent
                } else {
                    palette.border_slot
                }),
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new(section.to_owned()),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(if is_selected {
                        palette.text_accent
                    } else {
                        palette.text_primary
                    }),
                ));
            });
    }
}

fn populate_subentry_list(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    selected_section: Option<&str>,
    selected_subsection: Option<&str>,
    log: &LogState,
) {
    let Some(section) = selected_section else {
        return;
    };
    let Some(section_data) = log.section(section) else {
        parent.spawn((
            Text::new("(no entries yet)".to_owned()),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(palette.text_muted),
        ));
        if section == NOTES_SECTION {
            spawn_new_note_button(parent, palette);
        }
        return;
    };
    for (sub_key, entry) in &section_data.subsections {
        let is_selected = selected_subsection == Some(sub_key.as_str());
        let bg = if is_selected {
            palette.surface_raised
        } else {
            palette.surface_panel
        };
        parent
            .spawn((
                Button,
                LogPanelSubentryButton {
                    section: section.to_owned(),
                    subsection: sub_key.clone(),
                },
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(bg),
                BorderColor::all(if is_selected {
                    palette.border_accent
                } else {
                    palette.border_slot
                }),
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new(if entry.title.is_empty() {
                        sub_key.clone()
                    } else {
                        entry.title.clone()
                    }),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(if is_selected {
                        palette.text_accent
                    } else {
                        palette.text_primary
                    }),
                ));
            });
    }
    if section == NOTES_SECTION {
        spawn_new_note_button(parent, palette);
    }
}

fn spawn_new_note_button(parent: &mut ChildSpawnerCommands, palette: &Palette) {
    parent
        .spawn((
            Button,
            LogPanelNewNoteButton,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(palette.surface_raised),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("+ New note".to_owned()),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette.text_accent),
            ));
        });
}

fn populate_title_slot(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    entry: Option<&LogEntry>,
    editing_title: bool,
) {
    let Some(entry) = entry else {
        parent.spawn((
            Text::new("Select an entry, or start a new note.".to_owned()),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(palette.text_muted),
        ));
        return;
    };

    let player_owned = matches!(entry.owner, LogOwner::Player);

    if editing_title && player_owned {
        // The wrapper itself doubles as the anchor — `install_title_editors`
        // promotes it into a real `TextEdit` parent on the same frame.
        // Putting the anchor on the wrapper (rather than a separate child)
        // keeps the spawned editor properly laid out under a Node.
        parent.spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(28.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(palette.border_accent),
            BackgroundColor(palette.surface_raised),
            LogPanelTitleEditorAnchor {
                initial_text: entry.title.clone(),
            },
        ));
        return;
    }

    // Default label render. Player titles are wrapped in a Button so we
    // can detect a click into edit mode; engine titles are plain text.
    if player_owned {
        parent
            .spawn((
                Button,
                LogPanelTitleLabel,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(2.0), Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(if entry.title.is_empty() {
                        "(click to set title)".to_owned()
                    } else {
                        entry.title.clone()
                    }),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(if entry.title.is_empty() {
                        palette.text_muted
                    } else {
                        palette.text_accent
                    }),
                ));
            });
    } else {
        parent.spawn((
            Text::new(entry.title.clone()),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            TextColor(palette.text_accent),
        ));
    }
}

/// Anchor entity that the install system swaps for a real `TextEdit`.
/// Same idiom as the body editor's install path — needed because
/// `spawn_text_edit` allocates its own entity, which we can't do from
/// inside `with_children`.
#[derive(Component)]
struct LogPanelTitleEditorAnchor {
    initial_text: String,
}

fn populate_body_display(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    entry: Option<&LogEntry>,
) {
    let Some(entry) = entry else {
        return;
    };
    if !matches!(entry.owner, LogOwner::Engine) {
        return;
    }
    parent.spawn((
        Text::new(entry.body.clone()),
        TextFont {
            font_size: 13.0,
            ..default()
        },
        TextColor(palette.text_primary),
        Node {
            width: Val::Percent(100.0),
            ..default()
        },
    ));
    parent.spawn((
        Text::new("My notes".to_owned()),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(palette.text_muted),
    ));
}

fn populate_buttons(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    entry: Option<&LogEntry>,
) {
    let Some(entry) = entry else {
        return;
    };
    parent
        .spawn((
            Button,
            LogPanelSaveButton,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(palette.surface_panel),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("Save".to_owned()),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));
        });

    if matches!(entry.owner, LogOwner::Player) {
        parent
            .spawn((
                Button,
                LogPanelDeleteButton,
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(palette.surface_panel),
                BorderColor::all(palette.border_slot),
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new("Delete".to_owned()),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(palette.text_muted),
                ));
            });
    }
}

/// Promote `LogPanelTitleEditorAnchor` into an actual `TextEdit` widget
/// (single-line). Runs on every frame; only fires for newly-spawned
/// anchors that don't yet have a child editor.
fn install_title_editors(
    mut commands: Commands,
    anchors: Query<(Entity, &LogPanelTitleEditorAnchor), Added<LogPanelTitleEditorAnchor>>,
    mut focus: ResMut<TerminalFocus>,
) {
    for (entity, anchor) in &anchors {
        let initial = anchor.initial_text.clone();
        commands
            .entity(entity)
            .remove::<LogPanelTitleEditorAnchor>();
        let editor = spawn_text_edit_with(
            &mut commands,
            LOG_TITLE_FOCUS_ID,
            initial.as_str(),
            16.0,
            true, // single-line: Enter submits
        );
        commands.entity(editor).insert(LogPanelTitleEditor);
        commands.entity(entity).add_child(editor);
        // When the anchor first appears we just transitioned from label
        // to editor, so move keyboard focus over to it.
        focus.focused = Some(LOG_TITLE_FOCUS_ID);
    }
}

fn handle_bookmark_clicks(
    interactions: Query<(&Interaction, &LogPanelBookmark), Changed<Interaction>>,
    mut roots: Query<&mut LogPanelRoot>,
) {
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if root.selected_section.as_deref() != Some(button.section.as_str()) {
            root.selected_section = Some(button.section.clone());
            root.selected_subsection = None;
            root.editing_title = false;
        }
    }
}

fn handle_subentry_clicks(
    interactions: Query<(&Interaction, &LogPanelSubentryButton), Changed<Interaction>>,
    mut roots: Query<&mut LogPanelRoot>,
) {
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        root.selected_section = Some(button.section.clone());
        root.selected_subsection = Some(button.subsection.clone());
        root.editing_title = false;
    }
}

/// Click on the title label puts the panel into title-edit mode (only
/// for player-owned entries — engine titles render as plain text and
/// don't have a `LogPanelTitleLabel` button).
fn handle_title_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<LogPanelTitleLabel>)>,
    mut roots: Query<&mut LogPanelRoot>,
) {
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    for interaction in &interactions {
        if *interaction == Interaction::Pressed && !root.editing_title {
            root.editing_title = true;
        }
    }
}

/// "+ New note" — create a fresh, player-owned entry in the Notes section
/// with an auto-generated id and an empty body. Selection jumps to it so
/// the player can edit immediately.
fn handle_new_note_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<LogPanelNewNoteButton>)>,
    mut pending: ResMut<PendingGameCommands>,
    client_state: Res<ClientGameState>,
    mut roots: Query<&mut LogPanelRoot>,
) {
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    for interaction in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let next_id = next_note_id(&client_state.log_state);
        let subsection = format!("note_{next_id}");
        pending.push(GameCommand::UpsertLogEntry {
            section: NOTES_SECTION.to_owned(),
            subsection: subsection.clone(),
            title: format!("Note {next_id}"),
            body: String::new(),
            owner: LogOwner::Player,
        });
        root.selected_section = Some(NOTES_SECTION.to_owned());
        root.selected_subsection = Some(subsection);
        root.editing_title = false;
    }
}

fn next_note_id(log: &LogState) -> u32 {
    let Some(section) = log.section(NOTES_SECTION) else {
        return 1;
    };
    let mut max_id = 0u32;
    for key in section.subsections.keys() {
        if let Some(suffix) = key.strip_prefix("note_") {
            if let Ok(n) = suffix.parse::<u32>() {
                if n > max_id {
                    max_id = n;
                }
            }
        }
    }
    max_id + 1
}

fn handle_save_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<LogPanelSaveButton>)>,
    mut pending: ResMut<PendingGameCommands>,
    client_state: Res<ClientGameState>,
    mut roots: Query<&mut LogPanelRoot>,
    text_edits: Query<(&TextEditRoot, &TextEdit)>,
) {
    if interactions
        .iter()
        .all(|interaction| *interaction != Interaction::Pressed)
    {
        return;
    }
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    submit_pending_save(&mut pending, &client_state, &mut root, &text_edits);
}

/// Same flow as the Save button — driven by `Ctrl+Enter` on the body
/// editor or plain Enter on the (single-line) title editor.
fn consume_text_edit_submits(
    mut submits: bevy::ecs::message::MessageReader<TextEditSubmit>,
    mut pending: ResMut<PendingGameCommands>,
    client_state: Res<ClientGameState>,
    mut roots: Query<&mut LogPanelRoot>,
    text_edits: Query<(&TextEditRoot, &TextEdit)>,
) {
    let mut had_log_submit = false;
    for submit in submits.read() {
        if let Ok((te_root, _)) = text_edits.get(submit.text_edit) {
            if te_root.focus_id == LOG_NOTES_FOCUS_ID || te_root.focus_id == LOG_TITLE_FOCUS_ID {
                had_log_submit = true;
            }
        }
    }
    if !had_log_submit {
        return;
    }
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    submit_pending_save(&mut pending, &client_state, &mut root, &text_edits);
}

fn submit_pending_save(
    pending: &mut PendingGameCommands,
    client_state: &ClientGameState,
    root: &mut LogPanelRoot,
    text_edits: &Query<(&TextEditRoot, &TextEdit)>,
) {
    let (Some(section), Some(subsection)) = (
        root.selected_section.as_ref(),
        root.selected_subsection.as_ref(),
    ) else {
        return;
    };
    let Some(entry) = client_state.log_state.entry(section, subsection) else {
        return;
    };
    let body_buffer = text_edits
        .iter()
        .find(|(r, _)| r.focus_id == LOG_NOTES_FOCUS_ID)
        .map(|(_, state)| state.text())
        .unwrap_or_default();
    // Title from the title editor if it exists; otherwise fall back to
    // the entry's stored title (i.e. user didn't enter title-edit mode).
    let title_buffer = text_edits
        .iter()
        .find(|(r, _)| r.focus_id == LOG_TITLE_FOCUS_ID)
        .map(|(_, state)| state.text())
        .unwrap_or_else(|| entry.title.clone());

    match entry.owner {
        LogOwner::Engine => {
            pending.push(GameCommand::SetQuestPlayerNotes {
                quest_name: subsection.clone(),
                text: body_buffer,
            });
        }
        LogOwner::Player => {
            pending.push(GameCommand::UpsertLogEntry {
                section: section.clone(),
                subsection: subsection.clone(),
                title: title_buffer,
                body: body_buffer,
                owner: LogOwner::Player,
            });
        }
    }
    // Either way, exit title-edit mode after a save so the panel
    // returns to the label view. This drops the title editor entity on
    // the next rebuild.
    root.editing_title = false;
}

fn handle_delete_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<LogPanelDeleteButton>)>,
    mut pending: ResMut<PendingGameCommands>,
    client_state: Res<ClientGameState>,
    mut roots: Query<&mut LogPanelRoot>,
) {
    if interactions
        .iter()
        .all(|interaction| *interaction != Interaction::Pressed)
    {
        return;
    }
    let Ok(mut root) = roots.single_mut() else {
        return;
    };
    let (Some(section), Some(subsection)) = (
        root.selected_section.clone(),
        root.selected_subsection.clone(),
    ) else {
        return;
    };
    let Some(entry) = client_state.log_state.entry(&section, &subsection) else {
        return;
    };
    if !matches!(entry.owner, LogOwner::Player) {
        return;
    }
    pending.push(GameCommand::DeleteLogEntry {
        section,
        subsection,
    });
    root.selected_subsection = None;
    root.editing_title = false;
}

/// Escape clears the active log editor's focus so `KeyL` can toggle the
/// panel again. For the title editor it also drops edit mode (revert
/// without saving), mirroring how a typical desktop "rename" cancels on
/// Esc.
fn clear_editor_focus_on_escape(
    mut key_events: bevy::ecs::message::MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut focus: ResMut<TerminalFocus>,
    mut roots: Query<&mut LogPanelRoot>,
) {
    let active = matches!(
        focus.focused,
        Some(LOG_NOTES_FOCUS_ID) | Some(LOG_TITLE_FOCUS_ID)
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
    let was_title = focus.focused == Some(LOG_TITLE_FOCUS_ID);
    focus.focused = None;
    if was_title {
        if let Ok(mut root) = roots.single_mut() {
            root.editing_title = false;
        }
    }
}

/// If the panel has been closed (e.g. via the title bar's X button) but
/// the body/title editor was focused at the time, clear the focus so
/// that gameplay systems gated on `terminal_not_focused` can resume.
fn release_focus_when_panel_gone(
    mut focus: ResMut<TerminalFocus>,
    panels: Query<(), With<LogPanelRoot>>,
) {
    let active = matches!(
        focus.focused,
        Some(LOG_NOTES_FOCUS_ID) | Some(LOG_TITLE_FOCUS_ID)
    );
    if active && panels.is_empty() {
        focus.focused = None;
    }
}

/// Click into the body or title editor → focus it. Without this the
/// `TerminalFocus` resource never points at the editor and keystrokes
/// drop on the floor.
fn handle_editor_focus_click(
    interactions: Query<(&Interaction, &TextEditRoot), Changed<Interaction>>,
    mut focus: ResMut<TerminalFocus>,
) {
    for (interaction, root) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if root.focus_id == LOG_NOTES_FOCUS_ID || root.focus_id == LOG_TITLE_FOCUS_ID {
            focus.focused = Some(root.focus_id);
        }
    }
}
