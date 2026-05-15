//! Plugin wiring for the Log system. Two plugins:
//! - [`LogServerPlugin`]: server-side command processing.
//! - [`LogClientPlugin`]: client-side UI registration.

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::game::CommandIntercept;
use crate::log::commands::process_log_commands;

pub struct LogServerPlugin;

impl Plugin for LogServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            process_log_commands
                .in_set(CommandIntercept)
                .run_if(simulation_active),
        );
    }
}

pub struct LogClientPlugin;

impl Plugin for LogClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy_terminal::TextEditPlugin);
        crate::ui::log_panel::register(app);
    }
}
