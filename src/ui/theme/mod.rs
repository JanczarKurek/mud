pub mod assets;
pub mod palette;
mod procedural_button;
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
            .add_systems(Update, apply_themed_button_tint);
    }
}
