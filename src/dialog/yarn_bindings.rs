//! Rust-implemented Yarn commands and functions. Dialog authors call these
//! from `.yarn` files; each one is a Bevy system (commands) or a pure
//! function closure (functions).
//!
//! Phase 1 surface is intentionally small and stubby. Quest-engine-driven
//! bindings will arrive in Phase 2.

use bevy::prelude::*;
use bevy_yarnspinner::prelude::*;

/// Registers our custom commands / functions on a freshly created
/// `DialogueRunner`. Takes `&mut Commands` so Bevy-system-backed handlers can
/// be turned into `SystemId`s via `register_system`.
pub fn install(runner: &mut DialogueRunner, commands: &mut Commands) {
    let log_id = commands.register_system(log_command);
    let set_flag_id = commands.register_system(set_flag_command);
    let clear_flag_id = commands.register_system(clear_flag_command);
    runner
        .commands_mut()
        .add_command("log", log_id)
        .add_command("set_flag", set_flag_id)
        .add_command("clear_flag", clear_flag_id);

    runner.library_mut().add_function("has_flag", has_flag_fn);
}

fn log_command(In(message): In<String>) {
    bevy::log::info!("yarn: {message}");
}

fn set_flag_command(In(_name): In<String>) {
    // Variable-storage backed flags arrive with the quest engine in Phase 2;
    // this is a no-op stub so dialogs can already reference the command.
}

fn clear_flag_command(In(_name): In<String>) {}

fn has_flag_fn(_name: String) -> bool {
    false
}
