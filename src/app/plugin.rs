use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::prelude::*;
use bevy::window::Window;

use std::path::PathBuf;

use crate::accounts::AccountsServerPlugin;
use crate::app::asset_sync_screen::AssetSyncScreenPlugin;
use crate::app::auth_screen::AuthScreenPlugin;
use crate::app::setup::setup_camera;
use crate::app::state::ClientAppState;
use crate::app::title_screen::TitleScreenPlugin;
use crate::client_effects::ClientEffectsPlugin;
use crate::combat::CombatPlugin;
use crate::editor::EditorPlugin;
use crate::game::{GameClientPlugin, GameServerPlugin};
use crate::magic::MagicPlugin;
use crate::network::resources::TcpClientTlsConfig;
use crate::network::transport::{build_client_tls_config, load_server_tls_config};
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
    pub db_path: Option<PathBuf>,
    pub server_tls: Option<ServerTlsArgs>,
    pub client_tls: Option<ClientTlsArgs>,
}

/// CLI-supplied TLS configuration for the server side.
#[derive(Clone, Debug)]
pub struct ServerTlsArgs {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    /// When true, if either `cert_path` or `key_path` does not exist, a
    /// self-signed pair is generated at those paths. Requires the
    /// `dev-self-signed` Cargo feature at build time.
    pub generate_if_missing: bool,
}

/// CLI-supplied TLS configuration for the client side.
#[derive(Clone, Debug, Default)]
pub struct ClientTlsArgs {
    pub insecure: bool,
}

impl Plugin for GameAppPlugin {
    fn build(&self, app: &mut App) {
        crate::assets::set_xdg_overrides_enabled(matches!(self.runtime, AppRuntime::TcpClient));

        // Resolve TLS configs once, here, so failures are loud at startup
        // rather than asynchronously when a peer connects.
        let server_tls_config = self.server_tls.as_ref().map(|args| {
            #[cfg(feature = "dev-self-signed")]
            if args.generate_if_missing && (!args.cert_path.exists() || !args.key_path.exists()) {
                bevy::log::info!(
                    "generating self-signed TLS cert at {} / {}",
                    args.cert_path.display(),
                    args.key_path.display()
                );
                if let Err(err) = crate::network::transport::generate_self_signed_to_disk(
                    &args.cert_path,
                    &args.key_path,
                ) {
                    panic!("failed to generate self-signed TLS cert: {err}");
                }
            }
            #[cfg(not(feature = "dev-self-signed"))]
            if args.generate_if_missing {
                bevy::log::warn!(
                    "--generate-cert requested but binary was built without the \
                     `dev-self-signed` feature; expecting cert files to already exist"
                );
            }
            load_server_tls_config(&args.cert_path, &args.key_path).unwrap_or_else(|err| {
                panic!(
                    "failed to load TLS cert/key from {} / {}: {err}",
                    args.cert_path.display(),
                    args.key_path.display()
                )
            })
        });

        let client_tls = self.client_tls.as_ref().map(|args| {
            if args.insecure {
                bevy::log::warn!(
                    "TLS client configured with --insecure: server certificates will NOT \
                     be verified. Do not use over the public internet."
                );
            }
            let config = build_client_tls_config(args.insecure)
                .unwrap_or_else(|err| panic!("failed to build client TLS config: {err}"));
            let server_name =
                server_name_from_addr(self.server_addr.as_deref().unwrap_or("127.0.0.1:7000"));
            TcpClientTlsConfig {
                config,
                server_name,
            }
        });

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
                    AccountsServerPlugin {
                        db_path: self.db_path.clone(),
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
                    ClientEffectsPlugin,
                    ScriptingPlugin,
                    EditorPlugin,
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
                    ClientEffectsPlugin,
                    ScriptingPlugin,
                    TcpClientPlugin {
                        server_addr: self
                            .server_addr
                            .clone()
                            .unwrap_or_else(|| "127.0.0.1:7000".to_owned()),
                        tls: client_tls.clone(),
                    },
                    TitleScreenPlugin {
                        runtime: self.runtime,
                        server_addr: self.server_addr.clone(),
                    },
                    AssetSyncScreenPlugin,
                    AuthScreenPlugin,
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
                    AccountsServerPlugin {
                        db_path: self.db_path.clone(),
                    },
                    TcpServerPlugin {
                        bind_addr: self
                            .bind_addr
                            .clone()
                            .unwrap_or_else(|| "127.0.0.1:7000".to_owned()),
                        tls_config: server_tls_config,
                    },
                ));
            }
        }
    }
}

/// Extract the SNI hostname from a `host:port` style address. If `addr`
/// doesn't contain a colon (or ends with one), returns it unchanged.
fn server_name_from_addr(addr: &str) -> String {
    match addr.rsplit_once(':') {
        Some((host, _port)) if !host.is_empty() => host.to_owned(),
        _ => addr.to_owned(),
    }
}
