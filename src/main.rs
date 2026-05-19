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
    App::new().add_plugins(mud2_into_plugin(cli)).run();
    ExitCode::SUCCESS
}
