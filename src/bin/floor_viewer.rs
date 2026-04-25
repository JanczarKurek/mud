use bevy::prelude::*;
use mud2::floor_viewer::plugin::FloorViewerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Floor Viewer".into(),
                resolution: (1280_u32, 800_u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FloorViewerPlugin)
        .run();
}
