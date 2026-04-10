use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::window::Window;

use crate::app::setup::setup_camera;
use crate::combat::CombatPlugin;
use crate::game::GamePlugin;
use crate::magic::MagicPlugin;
use crate::npc::NpcPlugin;
use crate::player::{PlayerClientPlugin, PlayerServerPlugin};
use crate::scripting::ScriptingPlugin;
use crate::ui::UiPlugin;
use crate::world::{WorldClientPlugin, WorldServerPlugin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppRuntime {
    EmbeddedClient,
    HeadlessServer,
}

pub struct GameAppPlugin {
    pub runtime: AppRuntime,
}

impl Plugin for GameAppPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            GamePlugin,
            WorldServerPlugin,
            NpcPlugin,
            PlayerServerPlugin,
            CombatPlugin,
            MagicPlugin,
        ));

        match self.runtime {
            AppRuntime::EmbeddedClient => {
                app.add_plugins(DefaultPlugins.set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Mud 2.0".into(),
                        ..default()
                    }),
                    ..default()
                }))
                .add_systems(Startup, setup_camera)
                .add_plugins((
                    WorldClientPlugin,
                    PlayerClientPlugin,
                    UiPlugin,
                    ScriptingPlugin,
                ));
            }
            AppRuntime::HeadlessServer => {
                app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(
                    Duration::from_secs_f64(1.0 / 60.0),
                )));
            }
        }
    }
}
