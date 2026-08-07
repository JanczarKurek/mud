#[cfg(feature = "server-sim")]
pub mod admin_host;
#[cfg(feature = "server-sim")]
pub mod python;
pub mod resources;
#[cfg(feature = "server-sim")]
pub mod systems;

#[cfg(feature = "server-sim")]
use bevy::prelude::*;
#[cfg(feature = "server-sim")]
use bevy_terminal::TerminalWidgetPlugin;

#[cfg(feature = "server-sim")]
use crate::app::state::ClientAppState;
#[cfg(feature = "server-sim")]
use crate::scripting::python::PythonConsoleHost;
#[cfg(feature = "server-sim")]
use crate::scripting::resources::{PythonConsoleState, PythonHistoryPersist};
#[cfg(feature = "server-sim")]
use crate::scripting::systems::{
    handle_python_console_completion, handle_python_console_maximize_button,
    handle_python_console_restart_button, handle_python_console_submissions,
    load_python_console_history, persist_python_console_history,
    sync_python_console_maximize_label, toggle_python_console,
};

#[cfg(feature = "server-sim")]
pub use crate::scripting::admin_host::{AdminExecResult, AdminReplHost, CompileOutcome};

// The Python console only exists where the authoritative world lives in the
// same App (embedded mode), so the whole plugin is sim-gated.
#[cfg(feature = "server-sim")]
pub struct ScriptingPlugin;

#[cfg(feature = "server-sim")]
impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TerminalWidgetPlugin>() {
            app.add_plugins(TerminalWidgetPlugin);
        }
        app.insert_resource(PythonConsoleState::default())
            .insert_resource(PythonHistoryPersist::default())
            .insert_non_send_resource(PythonConsoleHost::new())
            // toggle_python_console runs in PreUpdate before
            // `bevy_terminal::terminal_input` so a backtick press that
            // opens the console doesn't also get inserted as input on the
            // newly focused terminal.
            .add_systems(
                PreUpdate,
                toggle_python_console
                    .in_set(crate::scripting::resources::PythonConsoleToggleSet)
                    .before(bevy_terminal::terminal_input)
                    .run_if(in_state(ClientAppState::InGame)),
            )
            .add_systems(
                Update,
                (
                    load_python_console_history,
                    handle_python_console_submissions,
                    persist_python_console_history,
                    handle_python_console_completion,
                    handle_python_console_restart_button,
                    handle_python_console_maximize_button,
                    sync_python_console_maximize_label,
                )
                    .run_if(in_state(ClientAppState::InGame)),
            );
    }
}
