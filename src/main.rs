use std::path::PathBuf;
use std::process::ExitCode;

use bevy::prelude::*;
use mud2::app::clean_cache::{self, Invoker};
use mud2::app::plugin::{AppRuntime, ClientTlsArgs, GameAppPlugin, ServerTlsArgs};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = clean_cache::dispatch(&argv, Invoker::Mud2) {
        return code;
    }

    let mut runtime = AppRuntime::EmbeddedClient;
    let mut server_addr = None;
    let mut save_path: Option<PathBuf> = None;
    let mut db_path: Option<PathBuf> = None;
    let mut asset_cache_dir: Option<PathBuf> = None;
    let mut server_tls_enabled = false;
    let mut tls_cert: Option<PathBuf> = None;
    let mut tls_key: Option<PathBuf> = None;
    let mut generate_cert = false;
    let mut client_tls_enabled = false;
    let mut client_tls_insecure = false;
    let mut admin_socket_enabled = false;
    let mut admin_socket_path: Option<PathBuf> = None;
    let mut admin_socket_mode: Option<u32> = None;
    let mut args = argv.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" | "server" | "--headless-server" => runtime = AppRuntime::HeadlessServer,
            "--tcp-client" | "tcp-client" => runtime = AppRuntime::TcpClient,
            "--client" | "client" => runtime = AppRuntime::EmbeddedClient,
            "--connect" => {
                if let Some(addr) = args.next() {
                    let (stripped_addr, is_tls) = strip_tls_scheme(&addr);
                    if is_tls {
                        client_tls_enabled = true;
                    }
                    server_addr = Some(stripped_addr);
                    runtime = AppRuntime::TcpClient;
                }
            }
            "--save-path" => {
                save_path = args.next().map(PathBuf::from);
            }
            "--db-path" => {
                db_path = args.next().map(PathBuf::from);
            }
            "--asset-cache" => {
                asset_cache_dir = args.next().map(PathBuf::from);
            }
            "--tls" => {
                // Context-dependent: server `--tls` only has meaning in
                // HeadlessServer mode; client `--tls` only in TcpClient mode.
                // Set both flags; the builder will use whichever is relevant.
                server_tls_enabled = true;
                client_tls_enabled = true;
            }
            "--tls-cert" => {
                tls_cert = args.next().map(PathBuf::from);
            }
            "--tls-key" => {
                tls_key = args.next().map(PathBuf::from);
            }
            "--generate-cert" => {
                generate_cert = true;
                server_tls_enabled = true;
            }
            "--insecure" => {
                client_tls_enabled = true;
                client_tls_insecure = true;
            }
            "--admin-socket" => {
                admin_socket_enabled = true;
                // Optional value: take it only if the next arg looks like a
                // path (starts with `/` or `.` or doesn't begin with `--`).
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
                if let Some(addr) = arg.strip_prefix("--connect=") {
                    let (stripped_addr, is_tls) = strip_tls_scheme(addr);
                    if is_tls {
                        client_tls_enabled = true;
                    }
                    server_addr = Some(stripped_addr);
                    runtime = AppRuntime::TcpClient;
                } else if let Some(path) = arg.strip_prefix("--save-path=") {
                    save_path = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--db-path=") {
                    db_path = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--asset-cache=") {
                    asset_cache_dir = Some(PathBuf::from(path));
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

    let save_path = save_path.or_else(|| std::env::var("MUD2_SAVE_PATH").ok().map(PathBuf::from));
    let db_path = db_path.or_else(|| std::env::var("MUD2_DB_PATH").ok().map(PathBuf::from));
    let asset_cache_dir =
        asset_cache_dir.or_else(|| std::env::var("MUD2_ASSET_CACHE").ok().map(PathBuf::from));

    let server_tls = if server_tls_enabled && matches!(runtime, AppRuntime::HeadlessServer) {
        Some(ServerTlsArgs {
            cert_path: tls_cert
                .clone()
                .unwrap_or_else(|| PathBuf::from("cert.pem")),
            key_path: tls_key.clone().unwrap_or_else(|| PathBuf::from("key.pem")),
            generate_if_missing: generate_cert,
        })
    } else {
        None
    };

    let client_tls = if client_tls_enabled && matches!(runtime, AppRuntime::TcpClient) {
        Some(ClientTlsArgs {
            insecure: client_tls_insecure,
        })
    } else {
        None
    };

    #[cfg(unix)]
    let admin_socket = if admin_socket_enabled && matches!(runtime, AppRuntime::HeadlessServer) {
        let socket_path = admin_socket_path
            .or_else(|| std::env::var("MUD2_ADMIN_SOCKET").ok().map(PathBuf::from))
            .or_else(|| mud2::app::paths::default_admin_socket_path(runtime))
            .unwrap_or_else(|| PathBuf::from("admin.sock"));
        let mode = admin_socket_mode.unwrap_or(0o600);
        Some(mud2::network::AdminListenArgs { socket_path, mode })
    } else {
        if admin_socket_enabled && !matches!(runtime, AppRuntime::HeadlessServer) {
            eprintln!("warning: --admin-socket is only honoured in headless-server mode; ignoring");
        }
        None
    };

    App::new()
        .add_plugins(GameAppPlugin {
            runtime,
            server_addr,
            bind_addr: None,
            save_path,
            db_path,
            asset_cache_dir,
            server_tls,
            client_tls,
            #[cfg(unix)]
            admin_socket,
        })
        .run();
    ExitCode::SUCCESS
}

/// `tls://host:port` → (`host:port`, true). Any other prefix is returned as-is
/// with `false`.
fn strip_tls_scheme(addr: &str) -> (String, bool) {
    if let Some(rest) = addr.strip_prefix("tls://") {
        (rest.to_owned(), true)
    } else {
        (addr.to_owned(), false)
    }
}
