pub mod assets;
mod font;
pub mod palette;
mod procedural_button;
pub mod text_field;
pub mod widgets;

use bevy::prelude::*;

pub use assets::UiThemeAssets;
pub use palette::Palette;
pub use widgets::{
    apply_themed_button_tint, colors_for, idle_colors, spawn_themed_button, spawn_themed_panel,
    ButtonStyle, ThemedButton, ThemedPanel,
};

/// Registers the global palette, loads placeholder 9-slice textures, and
/// wires up the shared hover/press recolor system for every `ThemedButton`.
pub struct UiThemePlugin;

impl Plugin for UiThemePlugin {
    fn build(&self, app: &mut App) {
        let asset_server = app
            .world()
            .get_resource::<AssetServer>()
            .expect("AssetServer must be initialized before UiThemePlugin")
            .clone();
        let assets = {
            let mut images = app
                .world_mut()
                .get_resource_mut::<Assets<Image>>()
                .expect("Assets<Image> must be initialized before UiThemePlugin");
            UiThemeAssets::load(&asset_server, &mut images)
        };
        app.insert_resource(Palette::default())
            .insert_resource(assets)
            .init_resource::<font::UiFonts>()
            .add_systems(Startup, font::setup_fonts)
            .add_systems(Update, apply_themed_button_tint)
            // Run text styling in `PostUpdate` *before* UI text measurement
            // (`UiSystems::Content`), so text spawned during `Update` is scaled
            // in the same frame it appears — never rendered at its unscaled size.
            // Running a frame late (in `Update`) makes panels that respawn their
            // text each frame flicker between the base and scaled sizes.
            .add_systems(
                PostUpdate,
                (font::style_new_text, font::reapply_text_settings)
                    .before(bevy::ui::UiSystems::Content),
            );
    }
}
