pub mod admin_host;
pub mod python;
pub mod resources;
pub mod systems;

use bevy::prelude::*;
use bevy_terminal::TerminalWidgetPlugin;

use crate::app::state::ClientAppState;
use crate::scripting::python::PythonConsoleHost;
use crate::scripting::resources::PythonConsoleState;
use crate::scripting::systems::{
    handle_python_console_completion, handle_python_console_restart_button,
    handle_python_console_submissions, toggle_python_console,
};

pub use crate::scripting::admin_host::{AdminExecResult, AdminReplHost, CompileOutcome};

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TerminalWidgetPlugin>() {
            app.add_plugins(TerminalWidgetPlugin);
        }
        app.insert_resource(PythonConsoleState::default())
            .insert_non_send_resource(PythonConsoleHost::new())
            // toggle_python_console runs in PreUpdate before
            // `bevy_terminal::terminal_input` so a backtick press that
            // opens the console doesn't also get inserted as input on the
            // newly focused terminal.
            .add_systems(
                PreUpdate,
                toggle_python_console
                    .before(bevy_terminal::terminal_input)
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    handle_python_console_submissions,
                    handle_python_console_completion,
                    handle_python_console_restart_button,
                )
                    .run_if(in_state(ClientAppState::InGame)),
            );
    }
}
