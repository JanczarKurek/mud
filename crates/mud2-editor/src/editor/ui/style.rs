//! Shared look-and-feel for the map editor's chrome: the named colors every
//! panel uses, the common action-button builder, and the side-panel skeleton
//! (root + header + scrollable body) that the six togglable panels share.
//!
//! Per-panel widths (`PANEL_WIDTH_PX`) are deliberately *not* unified — each
//! panel picks the width its content needs.

use bevy::prelude::*;

// ── Named colors ─────────────────────────────────────────────────────────────

/// Background of panel roots and the top bar.
pub(crate) const PANEL_BG: Color = Color::srgba(0.06, 0.04, 0.04, 0.92);
/// Border of panel roots, header underlines, and section dividers.
pub(crate) const PANEL_BORDER: Color = Color::srgb(0.30, 0.22, 0.15);
/// Panel titles and section headers.
pub(crate) const HEADER_TEXT: Color = Color::srgb(0.96, 0.84, 0.62);
/// Background of small row-action buttons (Edit / Dup / Del / steppers).
pub(crate) const BUTTON_BG: Color = Color::srgba(0.14, 0.10, 0.08, 0.95);
/// Border of small row-action buttons.
pub(crate) const BUTTON_BORDER: Color = Color::srgb(0.40, 0.30, 0.20);
/// Label color of small row-action buttons.
pub(crate) const BUTTON_TEXT: Color = Color::srgb(0.92, 0.86, 0.74);

// ── Button color groups ──────────────────────────────────────────────────────

/// The (background, border, text) trio a bordered editor button is drawn with.
#[derive(Clone, Copy)]
pub(crate) struct ButtonColors {
    pub bg: Color,
    pub border: Color,
    pub text: Color,
}

/// Small in-row action buttons (Edit / Dup / Del / + / -).
pub(crate) const ACTION_BUTTON_COLORS: ButtonColors = ButtonColors {
    bg: BUTTON_BG,
    border: BUTTON_BORDER,
    text: BUTTON_TEXT,
};

/// Top-bar buttons and header utility buttons (e.g. Refresh).
pub(crate) const TOP_BAR_BUTTON_COLORS: ButtonColors = ButtonColors {
    bg: Color::srgba(0.12, 0.08, 0.06, 0.90),
    border: Color::srgb(0.38, 0.28, 0.18),
    text: Color::srgb(0.88, 0.84, 0.76),
};

/// Emphasized "+ Add"-style buttons.
pub(crate) const ACCENT_BUTTON_COLORS: ButtonColors = ButtonColors {
    bg: Color::srgba(0.18, 0.12, 0.06, 0.95),
    border: Color::srgb(0.55, 0.40, 0.22),
    text: Color::srgb(0.96, 0.86, 0.66),
};

// ── Button builders ──────────────────────────────────────────────────────────

/// Fully-general bordered button: `Button + marker + node`, colored with
/// `colors`, containing a single text label. Callers own the `Node` so
/// per-site layout (padding, width, alignment) is preserved exactly.
pub(crate) fn editor_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    node: Node,
    colors: ButtonColors,
    label: impl Into<String>,
    font_size: f32,
) {
    parent
        .spawn((
            Button,
            marker,
            node,
            BackgroundColor(colors.bg),
            BorderColor::all(colors.border),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.into()),
                TextFont {
                    font_size,
                    ..default()
                },
                TextColor(colors.text),
            ));
        });
}

/// The common in-row action button (Edit / Dup / Del / Place / ×): shared
/// colors, 1px border, 10pt label — only the padding varies per panel.
pub(crate) fn editor_action_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    label: impl Into<String>,
    marker: M,
    padding: UiRect,
) {
    editor_button(
        parent,
        marker,
        Node {
            padding,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        ACTION_BUTTON_COLORS,
        label,
        10.0,
    );
}

// ── Panel chrome ─────────────────────────────────────────────────────────────

/// Spawns the shared side-panel skeleton used by the six togglable panels:
///
/// - root column: fixed width, full height, 1px left border, hidden
///   (`Display::None`) until its visibility-sync system shows it;
/// - header row: 8px padding, bottom border, 14pt title in `HEADER_TEXT`;
/// - `header_extras` runs inside the header (after the title) for per-panel
///   buttons like Refresh / + Add;
/// - `body` runs inside the root, below the header — most panels call
///   [`spawn_scroll_body`] here.
pub(crate) fn spawn_panel_chrome<M: Component>(
    parent: &mut ChildSpawnerCommands,
    marker: M,
    title: &str,
    width_px: f32,
    header_extras: impl FnOnce(&mut ChildSpawnerCommands),
    body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            marker,
            Node {
                width: Val::Px(width_px),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new(title.to_owned()),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(HEADER_TEXT),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    header_extras(h);
                });
            body(panel);
        });
}

/// The standard scrollable panel body: full-width growing column with
/// `Overflow::scroll_y`. Returned as a value so panels needing extra padding
/// or row gaps (e.g. lighting) can tweak fields before spawning.
pub(crate) fn scroll_body_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        flex_grow: 1.0,
        overflow: Overflow::scroll_y(),
        ..default()
    }
}

/// Spawn the standard scroll body tagged with the panel's content marker.
pub(crate) fn spawn_scroll_body<M: Component>(panel: &mut ChildSpawnerCommands, marker: M) {
    panel.spawn((
        marker,
        scroll_body_node(),
        bevy::ui::ScrollPosition::default(),
    ));
}
