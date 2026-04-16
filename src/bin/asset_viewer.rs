use bevy::prelude::*;
use mud2::asset_viewer::plugin::AssetViewerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Asset Viewer".into(),
                resolution: (1280_u32, 800_u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(AssetViewerPlugin)
        .run();
}
