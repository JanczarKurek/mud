use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use bevy::window::Window;

use crate::app::setup::setup_camera;
use crate::combat::CombatPlugin;
use crate::game::{GameClientPlugin, GameServerPlugin};
use crate::magic::MagicPlugin;
use crate::network::{TcpClientPlugin, TcpServerPlugin};
use crate::npc::NpcPlugin;
use crate::player::{PlayerClientPlugin, PlayerServerPlugin};
use crate::scripting::ScriptingPlugin;
use crate::ui::UiPlugin;
use crate::world::{WorldClientPlugin, WorldServerPlugin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppRuntime {
    EmbeddedClient,
    TcpClient,
    HeadlessServer,
}

pub struct GameAppPlugin {
    pub runtime: AppRuntime,
    pub server_addr: Option<String>,
    pub bind_addr: Option<String>,
}

impl Plugin for GameAppPlugin {
    fn build(&self, app: &mut App) {
        match self.runtime {
            AppRuntime::EmbeddedClient => {
                app.add_plugins((
                    GameServerPlugin,
                    WorldServerPlugin,
                    NpcPlugin,
                    PlayerServerPlugin,
                    CombatPlugin,
                    MagicPlugin,
                ));
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
            AppRuntime::TcpClient => {
                app.add_plugins(DefaultPlugins.set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Mud 2.0".into(),
                        ..default()
                    }),
                    ..default()
                }))
                .add_systems(Startup, setup_camera)
                .add_plugins((
                    GameClientPlugin,
                    WorldClientPlugin,
                    PlayerClientPlugin,
                    MagicPlugin,
                    UiPlugin,
                    ScriptingPlugin,
                    TcpClientPlugin {
                        server_addr: self
                            .server_addr
                            .clone()
                            .unwrap_or_else(|| "127.0.0.1:7000".to_owned()),
                    },
                ));
            }
            AppRuntime::HeadlessServer => {
                app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(
                    Duration::from_secs_f64(1.0 / 60.0),
                )))
                .add_plugins((
                    GameServerPlugin,
                    WorldServerPlugin,
                    NpcPlugin,
                    PlayerServerPlugin,
                    CombatPlugin,
                    MagicPlugin,
                    TcpServerPlugin {
                        bind_addr: self
                            .bind_addr
                            .clone()
                            .unwrap_or_else(|| "127.0.0.1:7000".to_owned()),
                    },
                ));
            }
        }
    }
}
