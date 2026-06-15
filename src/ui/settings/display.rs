//! Client-side display settings: window mode, vsync, UI scale.
//!
//! Pure presentation — this never touches `PendingGameCommands` /
//! `PendingGameEvents`, so it is byte-identical in all three runtime modes
//! and safe under the EmbeddedClient invariant. The resource is the source
//! of truth; `apply_display_settings` is the single sink that pushes it onto
//! the live `Window` and `UiScale` whenever it changes.

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use serde::{Deserialize, Serialize};

/// Discrete UI-scale steps the cycler walks through. `1.0` is the shipped
/// default and must stay in the list so a fresh install lands on it.
const UI_SCALE_STEPS: [f32; 5] = [0.75, 1.0, 1.25, 1.5, 2.0];

/// Discrete font-size multiplier steps. `1.15` is the shipped default (a slight
/// readability bump) and must stay in the list so a fresh install lands on it.
const FONT_SCALE_STEPS: [f32; 5] = [0.85, 1.0, 1.15, 1.3, 1.5];

/// Snap `current` to the nearest entry in `steps`, then return the next one
/// (wrapping). Matching on the closest step keeps a hand-edited file value
/// cycling predictably instead of getting stuck.
fn snap_and_advance(current: f32, steps: &[f32]) -> f32 {
    let i = steps
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - current).abs().total_cmp(&(**b - current).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0);
    steps[(i + 1) % steps.len()]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowModeSetting {
    #[default]
    Windowed,
    BorderlessFullscreen,
}

impl WindowModeSetting {
    const ALL: [WindowModeSetting; 2] = [Self::Windowed, Self::BorderlessFullscreen];

    fn label(self) -> &'static str {
        match self {
            Self::Windowed => "Windowed",
            Self::BorderlessFullscreen => "Borderless fullscreen",
        }
    }

    fn to_bevy(self) -> WindowMode {
        match self {
            Self::Windowed => WindowMode::Windowed,
            Self::BorderlessFullscreen => {
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            }
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// The remappable display state. `dirty` mirrors `Keybindings::dirty` — set
/// by the UI on any change, drained by `persist_settings`.
#[derive(Resource, Clone, Debug)]
pub struct DisplaySettings {
    pub window_mode: WindowModeSetting,
    pub vsync: bool,
    pub ui_scale: f32,
    /// Multiplier applied to every text's authored `font_size` by the theme's
    /// text-styling systems. Independent of `ui_scale` (which scales the whole
    /// UI); this scales fonts only.
    pub font_scale: f32,
    /// Anti-alias glyph edges. Off (the default) gives crisp pixels that match
    /// the nearest-neighbor sprite art; on softens text. Applied to every
    /// `TextFont` by the theme's text-styling systems.
    pub font_smoothing: bool,
    pub dirty: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            window_mode: WindowModeSetting::default(),
            vsync: true,
            ui_scale: 1.0,
            font_scale: 1.15,
            font_smoothing: false,
            dirty: false,
        }
    }
}

/// One configurable display knob — the unit the UI builds a row per and the
/// click handler advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOption {
    WindowMode,
    VSync,
    UiScale,
    FontScale,
    FontSmoothing,
}

impl DisplayOption {
    pub const ALL: [DisplayOption; 5] = [
        Self::WindowMode,
        Self::VSync,
        Self::UiScale,
        Self::FontScale,
        Self::FontSmoothing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::WindowMode => "Window mode",
            Self::VSync => "VSync",
            Self::UiScale => "UI scale",
            Self::FontScale => "Font size",
            Self::FontSmoothing => "Font smoothing",
        }
    }

    /// Current value rendered for the row's button.
    pub fn value_label(self, s: &DisplaySettings) -> String {
        match self {
            Self::WindowMode => s.window_mode.label().to_owned(),
            Self::VSync => if s.vsync { "On" } else { "Off" }.to_owned(),
            Self::UiScale => format!("{:.0}%", s.ui_scale * 100.0),
            Self::FontScale => format!("{:.0}%", s.font_scale * 100.0),
            Self::FontSmoothing => if s.font_smoothing { "On" } else { "Off" }.to_owned(),
        }
    }

    /// Advance to the next value (wrapping) and mark the resource dirty.
    pub fn cycle(self, s: &mut DisplaySettings) {
        match self {
            Self::WindowMode => s.window_mode = s.window_mode.next(),
            Self::VSync => s.vsync = !s.vsync,
            Self::FontSmoothing => s.font_smoothing = !s.font_smoothing,
            Self::UiScale => s.ui_scale = snap_and_advance(s.ui_scale, &UI_SCALE_STEPS),
            Self::FontScale => s.font_scale = snap_and_advance(s.font_scale, &FONT_SCALE_STEPS),
        }
        s.dirty = true;
    }
}

/// The single sink: whenever `DisplaySettings` changes, push it onto the live
/// `Window` and `UiScale`. Field-level guards keep this idempotent so it can
/// run every change without thrashing winit.
pub fn apply_display_settings(
    settings: Res<DisplaySettings>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
) {
    if !settings.is_changed() {
        return;
    }
    if let Ok(mut w) = window.single_mut() {
        let mode = settings.window_mode.to_bevy();
        if w.mode != mode {
            w.mode = mode;
        }
        let present = if settings.vsync {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        };
        if w.present_mode != present {
            w.present_mode = present;
        }
    }
    if ui_scale.0 != settings.ui_scale {
        ui_scale.0 = settings.ui_scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_mode_cycles_and_wraps() {
        let mut s = DisplaySettings::default();
        assert_eq!(s.window_mode, WindowModeSetting::Windowed);
        DisplayOption::WindowMode.cycle(&mut s);
        assert_eq!(s.window_mode, WindowModeSetting::BorderlessFullscreen);
        assert!(s.dirty);
        DisplayOption::WindowMode.cycle(&mut s);
        assert_eq!(s.window_mode, WindowModeSetting::Windowed);
    }

    #[test]
    fn ui_scale_snaps_then_advances() {
        let mut s = DisplaySettings {
            ui_scale: 1.1,
            ..Default::default()
        };
        // 1.1 is closest to the 1.0 step → next is 1.25.
        DisplayOption::UiScale.cycle(&mut s);
        assert_eq!(s.ui_scale, 1.25);
    }

    #[test]
    fn font_scale_snaps_then_advances() {
        let mut s = DisplaySettings {
            font_scale: 1.15,
            ..Default::default()
        };
        // 1.15 is a step → next is 1.3.
        DisplayOption::FontScale.cycle(&mut s);
        assert_eq!(s.font_scale, 1.3);
        assert!(s.dirty);
    }

    #[test]
    fn font_smoothing_toggles() {
        let mut s = DisplaySettings::default();
        assert!(!s.font_smoothing);
        DisplayOption::FontSmoothing.cycle(&mut s);
        assert!(s.font_smoothing);
    }

    #[test]
    fn vsync_toggles() {
        let mut s = DisplaySettings::default();
        assert!(s.vsync);
        DisplayOption::VSync.cycle(&mut s);
        assert!(!s.vsync);
    }
}
