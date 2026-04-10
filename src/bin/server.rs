use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut bind_addr = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => {
                bind_addr = args.next();
            }
            _ => {
                if let Some(addr) = arg.strip_prefix("--bind=") {
                    bind_addr = Some(addr.to_owned());
                }
            }
        }
    }

    App::new()
        .add_plugins(GameAppPlugin {
            runtime: AppRuntime::HeadlessServer,
            server_addr: None,
            bind_addr: bind_addr.or_else(|| std::env::var("MUD2_SERVER_BIND").ok()),
        })
        .run();
}
