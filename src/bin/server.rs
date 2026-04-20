use std::path::PathBuf;

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin, ServerTlsArgs};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut bind_addr = None;
    let mut save_path = None;
    let mut db_path: Option<PathBuf> = None;
    let mut tls_enabled = false;
    let mut tls_cert: Option<PathBuf> = None;
    let mut tls_key: Option<PathBuf> = None;
    let mut generate_cert = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind_addr = args.next();
            }
            "--save-path" => {
                save_path = args.next();
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
            _ => {
                if let Some(addr) = arg.strip_prefix("--bind=") {
                    bind_addr = Some(addr.to_owned());
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

    let server_tls = if tls_enabled {
        Some(ServerTlsArgs {
            cert_path: tls_cert.unwrap_or_else(|| PathBuf::from("cert.pem")),
            key_path: tls_key.unwrap_or_else(|| PathBuf::from("key.pem")),
            generate_if_missing: generate_cert,
        })
    } else {
        None
    };

    App::new()
        .add_plugins(GameAppPlugin {
            runtime: AppRuntime::HeadlessServer,
            server_addr: None,
            bind_addr: bind_addr.or_else(|| std::env::var("MUD2_SERVER_BIND").ok()),
            save_path: save_path.or_else(|| std::env::var("MUD2_SAVE_PATH").ok()),
            db_path: db_path.or_else(|| std::env::var("MUD2_DB_PATH").ok().map(PathBuf::from)),
            server_tls,
            client_tls: None,
        })
        .run();
}
