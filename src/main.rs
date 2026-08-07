use std::process::ExitCode;

use bevy::prelude::*;
use clap::Parser;

use mud2::app::clean_cache::{self, Invoker};
use mud2::app::cli::{mud2_into_plugin, Mud2Cli};

fn main() -> ExitCode {
    let cli = Mud2Cli::parse();
    if let Some(cmd) = cli.command {
        return clean_cache::run(cmd, Invoker::Mud2);
    }
    // Consume a pending "Clean game state" wipe (written by the in-app debug
    // button) before anything opens the world snapshot or accounts DB.
    clean_cache::consume_wipe_marker();
    let mut plugin = mud2_into_plugin(cli);
    // The map editor lives in its own crate (mud2-editor) so the game lib
    // doesn't depend on it; plug it into the embedded-client branch here.
    plugin.embedded_extension = Some(|app| {
        app.add_plugins(mud2_editor::editor::EditorPlugin);
    });
    App::new().add_plugins(plugin).run();
    // The "Clean game state" debug button writes a wipe marker and exits so the
    // deletion can run pre-boot. Re-exec ourselves so that fresh boot (which
    // consumes the marker and performs the wipe) happens now — the game
    // restarts automatically instead of requiring a manual relaunch.
    if clean_cache::wipe_marker_pending() {
        clean_cache::restart_process();
    }
    ExitCode::SUCCESS
}
