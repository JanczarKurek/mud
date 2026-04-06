mod app;
mod game;
mod player;
mod world;

use app::plugin::GameAppPlugin;
use bevy::prelude::*;

fn main() {
    App::new().add_plugins(GameAppPlugin).run();
}
