use std::path::PathBuf;
use std::process::ExitCode;

use bevy::prelude::*;
use mud2::app::clean_cache::{self, Invoker};
use mud2::app::plugin::{AppRuntime, GameAppPlugin, ServerTlsArgs};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = clean_cache::dispatch(&argv, Invoker::Server) {
        return code;
    }

    let mut args = argv.into_iter();
    let mut bind_addr = None;
    let mut save_path: Option<PathBuf> = None;
    let mut db_path: Option<PathBuf> = None;
    let mut tls_enabled = false;
    let mut tls_cert: Option<PathBuf> = None;
    let mut tls_key: Option<PathBuf> = None;
    let mut generate_cert = false;
    let mut admin_socket_enabled = false;
    let mut admin_socket_path: Option<PathBuf> = None;
    let mut admin_socket_mode: Option<u32> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind_addr = args.next();
            }
            "--save-path" => {
                save_path = args.next().map(PathBuf::from);
            }
            "--db-path" => {
                db_path = args.next().map(PathBuf::from);
            }
            "--tls" => {
                tls_enabled = true;
            }
            "--tls-cert" => {
                tls_cert = args.next().map(PathBuf::from);
            }
            "--tls-key" => {
                tls_key = args.next().map(PathBuf::from);
            }
            "--generate-cert" => {
                generate_cert = true;
                tls_enabled = true;
            }
            "--admin-socket" => {
                admin_socket_enabled = true;
                let next = args.clone().next();
                if let Some(value) = next {
                    if !value.starts_with("--") {
                        admin_socket_path = Some(PathBuf::from(value));
                        let _ = args.next();
                    }
                }
            }
            "--admin-socket-mode" => {
                if let Some(value) = args.next() {
                    match u32::from_str_radix(&value, 8) {
                        Ok(mode) => admin_socket_mode = Some(mode),
                        Err(err) => eprintln!(
                            "warning: --admin-socket-mode `{value}` is not a valid octal: {err}"
                        ),
                    }
                }
            }
            _ => {
                if let Some(addr) = arg.strip_prefix("--bind=") {
                    bind_addr = Some(addr.to_owned());
                } else if let Some(path) = arg.strip_prefix("--save-path=") {
                    save_path = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--db-path=") {
                    db_path = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--tls-cert=") {
                    tls_cert = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--tls-key=") {
                    tls_key = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--admin-socket=") {
                    admin_socket_enabled = true;
                    admin_socket_path = Some(PathBuf::from(path));
                } else if let Some(value) = arg.strip_prefix("--admin-socket-mode=") {
                    match u32::from_str_radix(value, 8) {
                        Ok(mode) => admin_socket_mode = Some(mode),
                        Err(err) => eprintln!(
                            "warning: --admin-socket-mode `{value}` is not a valid octal: {err}"
                        ),
                    }
                }
            }
        }
    }

    let server_tls = if tls_enabled {
        Some(ServerTlsArgs {
            cert_path: tls_cert.unwrap_or_else(|| PathBuf::from("cert.pem")),
            key_path: tls_key.unwrap_or_else(|| PathBuf::from("key.pem")),
            generate_if_missing: generate_cert,
        })
    } else {
        None
    };

    #[cfg(unix)]
    let admin_socket = if admin_socket_enabled {
        let socket_path = admin_socket_path
            .or_else(|| std::env::var("MUD2_ADMIN_SOCKET").ok().map(PathBuf::from))
            .or_else(|| mud2::app::paths::default_admin_socket_path(AppRuntime::HeadlessServer))
            .unwrap_or_else(|| PathBuf::from("admin.sock"));
        let mode = admin_socket_mode.unwrap_or(0o600);
        Some(mud2::network::AdminListenArgs { socket_path, mode })
    } else {
        None
    };

    App::new()
        .add_plugins(GameAppPlugin {
            runtime: AppRuntime::HeadlessServer,
            server_addr: None,
            bind_addr: bind_addr.or_else(|| std::env::var("MUD2_SERVER_BIND").ok()),
            save_path: save_path
                .or_else(|| std::env::var("MUD2_SAVE_PATH").ok().map(PathBuf::from)),
            db_path: db_path.or_else(|| std::env::var("MUD2_DB_PATH").ok().map(PathBuf::from)),
            asset_cache_dir: None,
            server_tls,
            client_tls: None,
            #[cfg(unix)]
            admin_socket,
        })
        .run();
    ExitCode::SUCCESS
}
