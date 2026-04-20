use std::path::PathBuf;

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, ClientTlsArgs, GameAppPlugin, ServerTlsArgs};

fn main() {
    let mut runtime = AppRuntime::EmbeddedClient;
    let mut server_addr = None;
    let mut save_path = None;
    let mut db_path: Option<PathBuf> = None;
    let mut server_tls_enabled = false;
    let mut tls_cert: Option<PathBuf> = None;
    let mut tls_key: Option<PathBuf> = None;
    let mut generate_cert = false;
    let mut client_tls_enabled = false;
    let mut client_tls_insecure = false;
    let mut args = std::env::args().skip(1);

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
                save_path = args.next();
            }
            "--db-path" => {
                db_path = args.next().map(PathBuf::from);
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
            _ => {
                if let Some(addr) = arg.strip_prefix("--connect=") {
                    let (stripped_addr, is_tls) = strip_tls_scheme(addr);
                    if is_tls {
                        client_tls_enabled = true;
                    }
                    server_addr = Some(stripped_addr);
                    runtime = AppRuntime::TcpClient;
                } else if let Some(path) = arg.strip_prefix("--save-path=") {
                    save_path = Some(path.to_owned());
                } else if let Some(path) = arg.strip_prefix("--db-path=") {
                    db_path = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--tls-cert=") {
                    tls_cert = Some(PathBuf::from(path));
                } else if let Some(path) = arg.strip_prefix("--tls-key=") {
                    tls_key = Some(PathBuf::from(path));
                }
            }
        }
    }

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

    App::new()
        .add_plugins(GameAppPlugin {
            runtime,
            server_addr,
            bind_addr: None,
            save_path,
            db_path,
            server_tls,
            client_tls,
        })
        .run();
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
