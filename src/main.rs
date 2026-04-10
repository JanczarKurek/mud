mod app;
mod combat;
mod game;
mod magic;
mod npc;
mod player;
mod scripting;
mod ui;
mod world;

use app::plugin::GameAppPlugin;
use app::plugin::AppRuntime;
use bevy::prelude::*;

fn main() {
    let runtime = std::env::args()
        .skip(1)
        .find_map(|arg| match arg.as_str() {
            "--server" | "server" | "--headless-server" => Some(AppRuntime::HeadlessServer),
            "--client" | "client" => Some(AppRuntime::EmbeddedClient),
            _ => None,
        })
        .unwrap_or(AppRuntime::EmbeddedClient);

    App::new().add_plugins(GameAppPlugin { runtime }).run();
}
