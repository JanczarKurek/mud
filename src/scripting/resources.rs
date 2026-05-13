use bevy::prelude::*;

/// Tiny presence flag for the Python console. Everything else — output,
/// input, history, scroll position — lives on the terminal widget's
/// `Terminal` component now. Read by the player input systems to gate
/// movement keys when the console is focused.
#[derive(Resource, Default)]
pub struct PythonConsoleState {
    pub is_open: bool,
}
