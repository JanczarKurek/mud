use bevy::prelude::*;

/// Tiny presence flag for the Python console. Everything else — output,
/// input, history, scroll position — lives on the terminal widget's
/// `Terminal` component now. Read by the player input systems to gate
/// movement keys when the console is focused.
#[derive(Resource, Default)]
pub struct PythonConsoleState {
    pub is_open: bool,
    /// When open, expand the panel to cover most of the screen. Toggled by
    /// the console header's Maximize/Restore button; persists across opens.
    pub maximized: bool,
}

/// Tracks the persistent (on-disk) Python console history so it can be seeded
/// into a freshly-spawned terminal once and appended to as commands run.
#[derive(Resource, Default)]
pub struct PythonHistoryPersist {
    /// Console-terminal entity we've already seeded history into (re-seed if
    /// the HUD is torn down and rebuilt, which spawns a new terminal entity).
    pub loaded_into: Option<Entity>,
    /// Last line written to disk, to suppress consecutive duplicates.
    pub last_written: Option<String>,
}
