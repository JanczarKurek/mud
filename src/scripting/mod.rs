pub mod python;
pub mod resources;
pub mod systems;

use bevy::prelude::*;

use crate::scripting::python::PythonConsoleHost;
use crate::scripting::resources::PythonConsoleState;
use crate::scripting::systems::{handle_python_console_input, refresh_python_console_ui};

pub struct ScriptingPlugin;

impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PythonConsoleState::default())
            .insert_non_send_resource(PythonConsoleHost::new())
            .add_systems(
                Update,
                (handle_python_console_input, refresh_python_console_ui),
            );
    }
}
