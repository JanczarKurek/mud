//! Global UI font setup + per-entity text styling.
//!
//! Every `TextFont { ..default() }` site relies on Bevy's default font handle,
//! so we swap the whole UI font by overwriting the asset at
//! `AssetId::<Font>::default()`. Per-entity tweaks — a global size bump, bold
//! button labels, and the smoothing toggle — are applied by `style_new_text`
//! as text spawns, with `reapply_font_smoothing` covering live toggles.

use bevy::asset::AssetId;
use bevy::prelude::*;
use bevy::text::{Font, FontSmoothing};
use bevy_terminal::TerminalTheme;

use crate::ui::settings::DisplaySettings;

// --- Font choices -----------------------------------------------------------
// To try a different look, point these at another file in `assets/fonts/` and
// rebuild. Bundled options:
//   Proportional: PixelOperator.ttf, FairfaxHD.ttf, FairfaxSMHD.ttf
//   Monospace:    PixelOperatorMono.ttf
//   (FairfaxHD-BoundsHack.ttf fixes overly tall line spacing.)

/// Global UI font — replaces Bevy's default everywhere text uses `..default()`.
const UI_FONT_DATA: &[u8] = include_bytes!("../../../../../assets/fonts/PixelOperator.ttf");

/// Bold variant, used for button labels.
const BOLD_FONT_DATA: &[u8] = include_bytes!("../../../../../assets/fonts/PixelOperator-Bold.ttf");

/// Monospace font for the in-game terminals only (console + chat).
const MONO_FONT_DATA: &[u8] = include_bytes!("../../../../../assets/fonts/PixelOperatorMono.ttf");

/// Max ancestor hops to search for a `Button` when deciding whether a piece of
/// text is a button label (and so should render bold).
const BUTTON_SEARCH_DEPTH: usize = 4;

/// Handles for non-default UI fonts that need to be referenced after load.
#[derive(Resource, Default)]
pub struct UiFonts {
    pub bold: Handle<Font>,
}

/// The authored `font_size` of a text entity, recorded before the global font
/// scale is applied. Lets `reapply_text_settings` recompute `base * scale` from
/// scratch when the scale changes, instead of compounding multiplications.
#[derive(Component)]
pub struct BaseFontSize(pub f32);

/// Install fonts at `Startup`:
/// - Overwrite Bevy's embedded default (`AssetId::<Font>::default()`) with the
///   proportional UI font, so every `TextFont { ..default() }` site picks it up.
/// - Load the bold font into `UiFonts` for button labels.
/// - Load the mono font and point `TerminalTheme::font` at it.
///
/// `Startup` runs after `TextPlugin::build` seeds the stock FiraMono (so our font
/// wins) and before any text is rasterized (first `PostUpdate`).
pub fn setup_fonts(
    mut fonts: ResMut<Assets<Font>>,
    mut ui_fonts: ResMut<UiFonts>,
    terminal_theme: Option<ResMut<TerminalTheme>>,
) {
    match Font::try_from_bytes(UI_FONT_DATA.to_vec()) {
        Ok(font) => {
            let _ = fonts.insert(AssetId::<Font>::default(), font);
        }
        Err(err) => warn!("failed to load UI font: {err}"),
    }
    match Font::try_from_bytes(BOLD_FONT_DATA.to_vec()) {
        Ok(font) => ui_fonts.bold = fonts.add(font),
        Err(err) => warn!("failed to load bold UI font: {err}"),
    }
    if let Some(mut theme) = terminal_theme {
        match Font::try_from_bytes(MONO_FONT_DATA.to_vec()) {
            Ok(font) => theme.font = fonts.add(font),
            Err(err) => warn!("failed to load terminal mono font: {err}"),
        }
    }
}

/// Style each newly spawned text entity: record its authored size and scale it
/// by the configured `font_scale`, apply the configured smoothing, and render
/// button labels in bold. `Added<TextFont>` fires once per entity.
pub fn style_new_text(
    mut commands: Commands,
    mut text: Query<(Entity, &mut TextFont), Added<TextFont>>,
    parents: Query<&ChildOf>,
    buttons: Query<(), With<Button>>,
    ui_fonts: Res<UiFonts>,
    display: Res<DisplaySettings>,
) {
    let smoothing = smoothing_for(&display);
    for (entity, mut font) in &mut text {
        let base = font.font_size;
        commands.entity(entity).insert(BaseFontSize(base));
        font.font_size = scaled_size(base, display.font_scale);
        if font.font_smoothing != smoothing {
            font.font_smoothing = smoothing;
        }
        if has_button_ancestor(entity, &parents, &buttons) {
            font.font = ui_fonts.bold.clone();
        }
    }
}

/// When display settings change, re-apply font smoothing (all text) and the font
/// scale (recomputed from each entity's recorded `BaseFontSize`).
pub fn reapply_text_settings(
    display: Res<DisplaySettings>,
    mut text: Query<(Option<&BaseFontSize>, &mut TextFont)>,
) {
    if !display.is_changed() {
        return;
    }
    let smoothing = smoothing_for(&display);
    for (base, mut font) in &mut text {
        if font.font_smoothing != smoothing {
            font.font_smoothing = smoothing;
        }
        if let Some(base) = base {
            let target = scaled_size(base.0, display.font_scale);
            if font.font_size != target {
                font.font_size = target;
            }
        }
    }
}

fn scaled_size(base: f32, scale: f32) -> f32 {
    (base * scale).round().max(1.0)
}

fn smoothing_for(display: &DisplaySettings) -> FontSmoothing {
    if display.font_smoothing {
        FontSmoothing::AntiAliased
    } else {
        FontSmoothing::None
    }
}

/// Walk up to `BUTTON_SEARCH_DEPTH` parents looking for a `Button` ancestor.
fn has_button_ancestor(
    mut entity: Entity,
    parents: &Query<&ChildOf>,
    buttons: &Query<(), With<Button>>,
) -> bool {
    for _ in 0..BUTTON_SEARCH_DEPTH {
        if buttons.contains(entity) {
            return true;
        }
        let Ok(parent) = parents.get(entity) else {
            return false;
        };
        entity = parent.0;
    }
    false
}
