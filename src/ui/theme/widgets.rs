use bevy::prelude::*;

use crate::ui::theme::assets::UiThemeAssets;
use crate::ui::theme::palette::Palette;

/// Visual role of a themed button. Drives both the 9-slice frame tint and the
/// hover/press recolor.
#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonStyle {
    /// Accented call-to-action (Take, Confirm, server "Connect", …).
    Primary,
    /// Default dark button (context menu items, zoom controls, take-partial +/-).
    Secondary,
    /// Close buttons, destructive actions.
    Danger,
    /// Inventory / equipment slots. Has a selected variant.
    Slot,
    /// Text-like button with no background at rest — menu-bar items, dropdown entries.
    Ghost,
}

/// Attach to any `Button` to hook into the shared hover/press recolor.
/// `selected` is used by `Slot` / `Ghost` buttons to show an active highlight.
#[derive(Component, Clone, Copy, Debug)]
pub struct ThemedButton {
    pub style: ButtonStyle,
    pub selected: bool,
}

impl ThemedButton {
    pub const fn new(style: ButtonStyle) -> Self {
        Self {
            style,
            selected: false,
        }
    }
}

/// Marker for panel frames.
#[derive(Component, Clone, Copy, Debug)]
pub struct ThemedPanel;

/// Spawn a themed button with a text label as its only child. Extra marker
/// components or custom children can be attached by the caller using
/// `parent.spawn(...)` directly if this helper is too restrictive.
pub fn spawn_themed_button<M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    style: ButtonStyle,
    node: Node,
    label: &str,
    font_size: f32,
    marker: M,
) {
    let (bg, border, text) = idle_colors(palette, style, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(style),
            marker,
            node,
            ImageNode::new(theme.button_frame.clone())
                .with_mode(theme.button_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

/// Spawn a themed panel frame. The caller populates its contents inside
/// `spawn_body`. Uses the 9-slice panel frame.
pub fn spawn_themed_panel(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    node: Node,
    spawn_body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            ThemedPanel,
            node,
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(palette.surface_panel),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_slot),
        ))
        .with_children(spawn_body);
}

/// Per-frame recolor of every `ThemedButton`. One system covers every themed
/// surface in the project.
pub fn apply_themed_button_tint(
    palette: Res<Palette>,
    mut query: Query<(
        &Interaction,
        &ThemedButton,
        &mut ImageNode,
        &mut BorderColor,
    )>,
) {
    for (interaction, themed, mut image, mut border) in &mut query {
        let (bg, border_color, _text) =
            colors_for(&palette, themed.style, themed.selected, *interaction);
        image.color = bg;
        *border = BorderColor::all(border_color);
    }
}

/// Resolve the (background-tint, border-color, text-color) triple for a given
/// style + interaction + selected flag. `text` is exposed so callers who spawn
/// text after the fact can keep colors consistent.
pub fn colors_for(
    palette: &Palette,
    style: ButtonStyle,
    selected: bool,
    interaction: Interaction,
) -> (Color, Color, Color) {
    match style {
        ButtonStyle::Primary => match interaction {
            Interaction::Pressed => (
                palette.button_primary_bg_pressed,
                palette.border_pressed,
                palette.text_primary,
            ),
            Interaction::Hovered => (
                palette.button_primary_bg_hover,
                palette.border_hover,
                palette.text_primary,
            ),
            Interaction::None => (
                palette.button_primary_bg,
                palette.border_accent,
                palette.text_primary,
            ),
        },
        ButtonStyle::Secondary => match interaction {
            Interaction::Pressed => (
                palette.button_secondary_bg_pressed,
                palette.border_pressed,
                palette.text_primary,
            ),
            Interaction::Hovered => (
                palette.button_secondary_bg_hover,
                palette.border_hover,
                palette.text_primary,
            ),
            Interaction::None => (
                palette.button_secondary_bg,
                palette.border_idle,
                palette.text_primary,
            ),
        },
        ButtonStyle::Danger => match interaction {
            Interaction::Pressed => (
                palette.button_danger_bg_pressed,
                palette.border_pressed,
                palette.text_primary,
            ),
            Interaction::Hovered => (
                palette.button_danger_bg_hover,
                palette.border_hover,
                palette.text_primary,
            ),
            Interaction::None => (
                palette.button_danger_bg,
                palette.border_danger,
                palette.text_primary,
            ),
        },
        ButtonStyle::Slot => {
            let (bg_idle, border) = if selected {
                (palette.button_slot_bg_selected, palette.border_accent)
            } else {
                (palette.button_slot_bg, palette.border_slot)
            };
            match interaction {
                Interaction::Pressed => (
                    palette.button_slot_bg_selected,
                    palette.border_pressed,
                    palette.text_primary,
                ),
                Interaction::Hovered => (
                    palette.button_slot_bg_hover,
                    palette.border_hover,
                    palette.text_primary,
                ),
                Interaction::None => (bg_idle, border, palette.text_primary),
            }
        }
        ButtonStyle::Ghost => {
            let hovered_bg = palette.button_ghost_bg_hover;
            match (interaction, selected) {
                (Interaction::Pressed, _) | (Interaction::Hovered, _) => {
                    (hovered_bg, Color::NONE, palette.text_primary)
                }
                (Interaction::None, true) => (hovered_bg, Color::NONE, palette.text_primary),
                (Interaction::None, false) => {
                    (palette.button_ghost_bg, Color::NONE, palette.text_primary)
                }
            }
        }
    }
}

/// Idle colors for a given style, used at spawn time so the first frame
/// renders the correct colors before the system runs.
pub fn idle_colors(palette: &Palette, style: ButtonStyle, selected: bool) -> (Color, Color, Color) {
    colors_for(palette, style, selected, Interaction::None)
}
