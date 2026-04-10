use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};

fn main() {
    let mut runtime = AppRuntime::EmbeddedClient;
    let mut server_addr = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" | "server" | "--headless-server" => runtime = AppRuntime::HeadlessServer,
            "--tcp-client" | "tcp-client" => runtime = AppRuntime::TcpClient,
            "--client" | "client" => runtime = AppRuntime::EmbeddedClient,
            "--connect" => {
                if let Some(addr) = args.next() {
                    server_addr = Some(addr);
                    runtime = AppRuntime::TcpClient;
                }
            }
            _ => {
                if let Some(addr) = arg.strip_prefix("--connect=") {
                    server_addr = Some(addr.to_owned());
                    runtime = AppRuntime::TcpClient;
                }
            }
        }
    }

    App::new()
        .add_plugins(GameAppPlugin {
            runtime,
            server_addr,
            bind_addr: None,
        })
        .run();
}
