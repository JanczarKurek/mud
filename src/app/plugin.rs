use bevy::prelude::*;
use bevy::window::Window;

use crate::app::setup::setup_camera;
use crate::game::GamePlugin;
use crate::npc::NpcPlugin;
use crate::player::PlayerPlugin;
use crate::scripting::ScriptingPlugin;
use crate::ui::UiPlugin;
use crate::world::WorldPlugin;

pub struct GameAppPlugin;

impl Plugin for GameAppPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Mud 2.0".into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup_camera)
        .add_plugins((
            GamePlugin,
            WorldPlugin,
            NpcPlugin,
            PlayerPlugin,
            UiPlugin,
            ScriptingPlugin,
        ));
    }
}
