use std::path::PathBuf;

use bevy::prelude::*;
use bevy_yarnspinner::prelude::*;

use crate::app::state::simulation_active;
use crate::dialog::resources::{DialogSessionRegistry, PendingDialogOptions};
use crate::dialog::systems::{
    forward_dialogue_completed, forward_present_line, forward_present_options,
    process_dialog_commands,
};
use crate::game::CommandIntercept;

/// Registers bevy_yarnspinner and the server-side dialog plumbing.
///
/// Intentionally *not* added to `HeadlessServer` yet: Yarn requires
/// `AssetPlugin` to compile `.yarn` files, and the headless runtime uses
/// `MinimalPlugins`. Dialog support for networked play is Phase 3.
pub struct DialogServerPlugin;

impl Plugin for DialogServerPlugin {
    fn build(&self, app: &mut App) {
        let yarn_dir: PathBuf = PathBuf::from("dialogs");
        app.add_plugins(YarnSpinnerPlugin::with_yarn_source(YarnFileSource::folder(
            yarn_dir,
        )))
        .insert_resource(DialogSessionRegistry::default())
        .insert_resource(PendingDialogOptions::default())
        // `CommandIntercept` is a `SystemSet` configured by `GameServerPlugin`
        // to sit between `tick_player_movement_cooldowns` and
        // `process_game_commands`. Binding to the set (rather than `.before(fn)`)
        // is necessary because function-identity ordering across plugins was
        // silently dropped in practice.
        .add_systems(
            Update,
            process_dialog_commands
                .in_set(CommandIntercept)
                .run_if(simulation_active),
        )
        .add_observer(forward_present_line)
        .add_observer(forward_present_options)
        .add_observer(forward_dialogue_completed);
    }
}
