use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::prelude::*;
use bevy::window::Window;

use crate::app::setup::setup_camera;
use crate::app::state::ClientAppState;
use crate::app::title_screen::TitleScreenPlugin;
use crate::combat::CombatPlugin;
use crate::game::{GameClientPlugin, GameServerPlugin};
use crate::magic::MagicPlugin;
use crate::network::{TcpClientPlugin, TcpServerPlugin};
use crate::npc::NpcPlugin;
use crate::persistence::{PersistenceServerPlugin, PersistenceStartupSet};
use crate::player::setup::spawn_embedded_player_authoritative;
use crate::player::{PlayerClientPlugin, PlayerServerPlugin};
use crate::scripting::ScriptingPlugin;
use crate::ui::UiPlugin;
use crate::world::setup::WorldStartupSet;
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
    pub save_path: Option<String>,
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
                    PersistenceServerPlugin {
                        save_path: self.save_path.clone(),
                    },
                ));
                app.add_plugins(DefaultPlugins.set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Mud 2.0".into(),
                        ..default()
                    }),
                    ..default()
                }))
                .init_state::<ClientAppState>()
                .add_systems(Startup, setup_camera)
                .add_systems(
                    Startup,
                    spawn_embedded_player_authoritative
                        .after(PersistenceStartupSet::LoadSnapshot)
                        .after(WorldStartupSet::InitializeRuntimeSpaces),
                )
                .add_plugins((
                    WorldClientPlugin,
                    PlayerClientPlugin,
                    UiPlugin,
                    ScriptingPlugin,
                    TitleScreenPlugin {
                        runtime: self.runtime,
                        server_addr: self.server_addr.clone(),
                    },
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
                .init_state::<ClientAppState>()
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
                    TitleScreenPlugin {
                        runtime: self.runtime,
                        server_addr: self.server_addr.clone(),
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
                    TerminalCtrlCHandlerPlugin,
                    PersistenceServerPlugin {
                        save_path: self.save_path.clone(),
                    },
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
