use bevy::prelude::*;

/// Central color palette for the UI. All HUD widgets, the title screen, the
/// menu bar, and the editor modal read their colors from here so they can be
/// retuned in one place.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Palette {
    // Surface tints — applied to `ImageNode.color` on 9-slice frames, or to
    // `BackgroundColor` when no texture is used.
    pub surface_panel: Color,
    pub surface_raised: Color,
    pub surface_title_bar: Color,
    pub surface_sidebar: Color,
    pub surface_chat: Color,
    pub surface_minimap_bg: Color,
    pub surface_console_output: Color,
    pub surface_console_input: Color,
    pub surface_scrollbar_track: Color,
    pub surface_scrollbar_thumb: Color,
    pub surface_resize_handle: Color,
    pub surface_vital_bg: Color,
    pub surface_overlay_dim: Color,
    pub surface_overlay_strong: Color,

    // Button surfaces (base, hover, pressed).
    pub button_primary_bg: Color,
    pub button_primary_bg_hover: Color,
    pub button_primary_bg_pressed: Color,
    pub button_secondary_bg: Color,
    pub button_secondary_bg_hover: Color,
    pub button_secondary_bg_pressed: Color,
    pub button_danger_bg: Color,
    pub button_danger_bg_hover: Color,
    pub button_danger_bg_pressed: Color,
    pub button_slot_bg: Color,
    pub button_slot_bg_hover: Color,
    pub button_slot_bg_selected: Color,
    pub button_ghost_bg: Color,
    pub button_ghost_bg_hover: Color,

    // Borders.
    pub border_idle: Color,
    pub border_hover: Color,
    pub border_pressed: Color,
    pub border_muted: Color,
    pub border_accent: Color,
    pub border_focus: Color,
    pub border_danger: Color,
    pub border_slot: Color,
    pub border_divider: Color,

    // Text.
    pub text_primary: Color,
    pub text_muted: Color,
    pub text_accent: Color,
    pub text_value: Color,
    pub text_placeholder: Color,
    pub text_danger: Color,
    pub text_label_slot: Color,
    pub text_quantity: Color,

    // Vitals.
    pub vital_health_fill: Color,
    pub vital_mana_fill: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            // Surfaces.
            surface_panel: Color::srgba(0.10, 0.10, 0.12, 0.92),
            surface_raised: Color::srgba(0.14, 0.10, 0.10, 0.94),
            surface_title_bar: Color::srgb(0.13, 0.12, 0.10),
            surface_sidebar: Color::srgba(0.06, 0.06, 0.08, 0.88),
            surface_chat: Color::srgba(0.07, 0.08, 0.10, 0.90),
            surface_minimap_bg: Color::srgb(0.04, 0.04, 0.05),
            surface_console_output: Color::srgba(0.04, 0.05, 0.07, 0.92),
            surface_console_input: Color::srgba(0.11, 0.10, 0.09, 0.96),
            surface_scrollbar_track: Color::srgba(0.10, 0.10, 0.11, 0.95),
            surface_scrollbar_thumb: Color::srgb(0.66, 0.60, 0.38),
            surface_resize_handle: Color::srgb(0.18, 0.16, 0.12),
            surface_vital_bg: Color::srgb(0.18, 0.18, 0.20),
            surface_overlay_dim: Color::srgba(0.0, 0.0, 0.0, 0.5),
            surface_overlay_strong: Color::srgba(0.0, 0.0, 0.0, 0.72),

            // Buttons.
            button_primary_bg: Color::srgba(0.18, 0.12, 0.10, 0.96),
            button_primary_bg_hover: Color::srgb(0.34, 0.18, 0.10),
            button_primary_bg_pressed: Color::srgb(0.62, 0.32, 0.14),
            button_secondary_bg: Color::srgb(0.18, 0.15, 0.11),
            button_secondary_bg_hover: Color::srgb(0.28, 0.22, 0.14),
            button_secondary_bg_pressed: Color::srgb(0.44, 0.32, 0.18),
            button_danger_bg: Color::srgb(0.22, 0.11, 0.10),
            button_danger_bg_hover: Color::srgb(0.36, 0.16, 0.13),
            button_danger_bg_pressed: Color::srgb(0.58, 0.22, 0.18),
            button_slot_bg: Color::srgb(0.16, 0.15, 0.12),
            button_slot_bg_hover: Color::srgb(0.24, 0.22, 0.16),
            button_slot_bg_selected: Color::srgb(0.28, 0.16, 0.08),
            button_ghost_bg: Color::NONE,
            button_ghost_bg_hover: Color::srgb(0.20, 0.18, 0.14),

            // Borders.
            border_idle: Color::srgb(0.48, 0.36, 0.22),
            border_hover: Color::srgb(0.90, 0.75, 0.50),
            border_pressed: Color::srgb(1.0, 0.88, 0.64),
            border_muted: Color::srgb(0.30, 0.28, 0.22),
            border_accent: Color::srgb(0.70, 0.55, 0.28),
            border_focus: Color::srgb(0.90, 0.72, 0.40),
            border_danger: Color::srgb(0.52, 0.30, 0.20),
            border_slot: Color::srgb(0.38, 0.34, 0.22),
            border_divider: Color::srgb(0.20, 0.14, 0.10),

            // Text.
            text_primary: Color::srgb(0.95, 0.89, 0.72),
            text_muted: Color::srgb(0.75, 0.70, 0.62),
            text_accent: Color::srgb(0.96, 0.84, 0.62),
            text_value: Color::srgb(0.96, 0.92, 0.80),
            text_placeholder: Color::srgb(0.45, 0.42, 0.38),
            text_danger: Color::srgb(1.0, 0.45, 0.30),
            text_label_slot: Color::srgb(0.80, 0.77, 0.69),
            text_quantity: Color::srgb(1.0, 1.0, 0.7),

            // Vitals.
            vital_health_fill: Color::srgb(0.70, 0.16, 0.18),
            vital_mana_fill: Color::srgb(0.14, 0.35, 0.78),
        }
    }
}
