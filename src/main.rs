mod app;
mod game;
mod player;
mod scripting;
mod ui;
mod world;

use app::plugin::GameAppPlugin;
use bevy::prelude::*;

fn main() {
    App::new().add_plugins(GameAppPlugin).run();
}
