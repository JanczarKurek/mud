use bevy::prelude::*;

use crate::ui::theme::Palette;

const DEFAULT_BAR_HEIGHT_PX: f32 = 14.0;

pub struct RetroBarStyle {
    pub fill_color: Color,
    pub height_px: f32,
    pub background: Option<Color>,
    pub border: Option<Color>,
    pub initial_fill_ratio: f32,
}

impl Default for RetroBarStyle {
    fn default() -> Self {
        Self {
            fill_color: Color::srgb(0.7, 0.7, 0.7),
            height_px: DEFAULT_BAR_HEIGHT_PX,
            background: None,
            border: None,
            initial_fill_ratio: 1.0,
        }
    }
}

impl RetroBarStyle {
    pub fn with_fill(mut self, color: Color) -> Self {
        self.fill_color = color;
        self
    }

    pub fn with_height(mut self, height_px: f32) -> Self {
        self.height_px = height_px;
        self
    }

    pub fn with_initial_ratio(mut self, ratio: f32) -> Self {
        self.initial_fill_ratio = ratio.clamp(0.0, 1.0);
        self
    }
}

/// Spawn a retro-styled bar that flex-grows to fill its parent's row width.
///
/// Structure:
///   background (pill, clips children) -> fill (rounded, carries marker)
///                                          -> gloss strip (top, absolute)
///                                          -> inner-shadow strip (bottom, absolute)
///
/// The `fill_marker` bundle is attached to the fill `Node` so existing systems
/// that mutate `node.width` via a marker query work unchanged.
pub fn spawn_retro_bar(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    style: RetroBarStyle,
    fill_marker: impl Bundle,
) -> Entity {
    spawn_retro_bar_impl(
        parent,
        palette,
        style,
        fill_marker,
        None::<((), String, f32)>,
    )
}

/// [`spawn_retro_bar`] plus a text overlay centered on the bar (the classic
/// "46/60"-on-the-health-bar readout). `label_marker` lands on the `Text`
/// entity so a sync system can rewrite the string via a marker query.
pub fn spawn_retro_bar_with_label<L: Bundle>(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    style: RetroBarStyle,
    fill_marker: impl Bundle,
    label_marker: L,
    initial_text: &str,
    font_size: f32,
) -> Entity {
    spawn_retro_bar_impl(
        parent,
        palette,
        style,
        fill_marker,
        Some((label_marker, initial_text.to_owned(), font_size)),
    )
}

fn spawn_retro_bar_impl<L: Bundle>(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    style: RetroBarStyle,
    fill_marker: impl Bundle,
    label: Option<(L, String, f32)>,
) -> Entity {
    let h = style.height_px;
    let radius = h * 0.5;
    let bg = style.background.unwrap_or(palette.surface_vital_bg);
    let border = style.border.unwrap_or(palette.border_muted);
    let ratio = style.initial_fill_ratio.clamp(0.0, 1.0);

    let mut root = parent.spawn((
        Node {
            flex_grow: 1.0,
            height: Val::Px(h),
            overflow: Overflow::clip(),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(radius)),
            ..default()
        },
        BackgroundColor(bg),
        BorderColor::all(border),
    ));

    root.with_children(|bg_node| {
        bg_node
            .spawn((
                fill_marker,
                Node {
                    width: Val::Percent(ratio * 100.0),
                    height: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    border_radius: BorderRadius::all(Val::Px(radius)),
                    ..default()
                },
                BackgroundColor(style.fill_color),
            ))
            .with_children(|fill| {
                // Convex gloss highlight along the top half.
                fill.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(45.0),
                        border_radius: BorderRadius {
                            top_left: Val::Px(radius),
                            top_right: Val::Px(radius),
                            bottom_left: Val::Px(0.0),
                            bottom_right: Val::Px(0.0),
                        },
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.18)),
                ));
                // Inner shadow along the bottom — sells the convex 3D look.
                fill.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(0.0),
                        left: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(35.0),
                        border_radius: BorderRadius {
                            top_left: Val::Px(0.0),
                            top_right: Val::Px(0.0),
                            bottom_left: Val::Px(radius),
                            bottom_right: Val::Px(radius),
                        },
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.28)),
                ));
            });

        // The overlay sits after the fill so it renders above it, and spans
        // the whole track so the text stays centered regardless of fill.
        if let Some((label_marker, text, font_size)) = label {
            bg_node
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(0.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|wrap| {
                    wrap.spawn((
                        Text::new(text),
                        label_marker,
                        TextFont {
                            font_size,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                    ));
                });
        }
    });

    root.id()
}
