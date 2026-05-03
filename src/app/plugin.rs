use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::prelude::*;
use bevy::window::Window;

use std::path::PathBuf;

use crate::accounts::AccountsServerPlugin;
use crate::app::asset_sync_screen::AssetSyncScreenPlugin;
use crate::app::auth_screen::AuthScreenPlugin;
use crate::app::paths::{client_paths, embedded_paths, server_paths};
use crate::app::setup::setup_camera;
use crate::app::state::ClientAppState;
use crate::app::title_screen::TitleScreenPlugin;
use crate::client_effects::ClientEffectsPlugin;
use crate::combat::CombatPlugin;
use crate::dialog::DialogServerPlugin;
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
use crate::quest::QuestPlugin;
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
    /// Override for the world-snapshot path. `None` = use the per-role default
    /// from `crate::app::paths`.
    pub save_path: Option<PathBuf>,
    /// Override for the accounts DB path. `None` = use the per-role default.
    pub db_path: Option<PathBuf>,
    /// Override for the TcpClient asset-sync cache directory. `None` = use the
    /// default from `crate::app::paths::client_paths`.
    pub asset_cache_dir: Option<PathBuf>,
    pub server_tls: Option<ServerTlsArgs>,
    pub client_tls: Option<ClientTlsArgs>,
    /// Admin Python REPL listener config. `Some` ⇒ attach `AdminReplPlugin`
    /// in HeadlessServer mode (other modes ignore it). `#[cfg(unix)]` only.
    #[cfg(unix)]
    pub admin_socket: Option<crate::network::AdminListenArgs>,
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
        // Install the process-wide XDG asset overlay root. Only TcpClient
        // consults it; other roles load bundled assets exclusively.
        let xdg_asset_root = match self.runtime {
            AppRuntime::TcpClient => Some(
                self.asset_cache_dir
                    .clone()
                    .unwrap_or_else(|| client_paths().asset_cache_dir),
            ),
            AppRuntime::EmbeddedClient | AppRuntime::HeadlessServer => None,
        };
        crate::assets::init_xdg_asset_root(xdg_asset_root);

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
                let defaults = embedded_paths();
                let save_path = self.save_path.clone().unwrap_or(defaults.world_snapshot);
                let db_path = self.db_path.clone().unwrap_or(defaults.accounts_db);
                app.add_plugins((
                    GameServerPlugin,
                    WorldServerPlugin,
                    NpcPlugin,
                    PlayerServerPlugin,
                    CombatPlugin,
                    MagicPlugin,
                    PersistenceServerPlugin { save_path },
                    AccountsServerPlugin { db_path },
                ));
                app.add_plugins(
                    DefaultPlugins
                        // Pixel-art floors/sprites read from packed atlases —
                        // bilinear sampling reads neighbouring atlas cells at
                        // mask boundaries, causing visible bleed. Same fix
                        // commit `2ecfbe5` applied to `floor_viewer`.
                        .set(ImagePlugin::default_nearest())
                        .set(WindowPlugin {
                            primary_window: Some(Window {
                                title: "Mud 2.0".into(),
                                ..default()
                            }),
                            ..default()
                        }),
                )
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
                    // Added after DefaultPlugins so YarnSpinnerPlugin can see
                    // AssetServer for `.yarn` compilation.
                    DialogServerPlugin,
                    QuestPlugin::default(),
                    TitleScreenPlugin {
                        runtime: self.runtime,
                        server_addr: self.server_addr.clone(),
                    },
                ));
            }
            AppRuntime::TcpClient => {
                app.add_plugins(
                    DefaultPlugins
                        .set(ImagePlugin::default_nearest())
                        .set(WindowPlugin {
                            primary_window: Some(Window {
                                title: "Mud 2.0".into(),
                                ..default()
                            }),
                            ..default()
                        }),
                )
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
                let defaults = server_paths();
                let save_path = self.save_path.clone().unwrap_or(defaults.world_snapshot);
                let db_path = self.db_path.clone().unwrap_or(defaults.accounts_db);
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
                    PersistenceServerPlugin { save_path },
                    AccountsServerPlugin { db_path },
                    TcpServerPlugin {
                        bind_addr: self
                            .bind_addr
                            .clone()
                            .unwrap_or_else(|| "127.0.0.1:7000".to_owned()),
                        tls_config: server_tls_config,
                    },
                    QuestPlugin::default(),
                ));

                // Quest systems read/write yarn variable stores even when no
                // full dialog runtime is attached (HeadlessServer skips
                // `DialogServerPlugin` because YarnSpinner needs `AssetPlugin`,
                // which `MinimalPlugins` does not provide). Insert the bare
                // resource so `drain_quest_events` / `drain_quest_commands`
                // can run as no-ops.
                app.insert_resource(crate::dialog::resources::CharacterVarStores::default());

                #[cfg(unix)]
                if let Some(args) = self.admin_socket.clone() {
                    app.add_plugins(crate::network::AdminReplPlugin { args });
                }
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
