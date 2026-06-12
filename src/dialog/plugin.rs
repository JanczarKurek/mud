use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_yarnspinner::prelude::*;

use crate::app::state::simulation_active;
use crate::dialog::resources::{
    CharacterVarStores, DialogSessionRegistry, PendingDialogOptions, PendingSkillCheckRequests,
    PlayerInventorySnapshots, PlayerSkillSnapshots, PlayerStashSnapshots,
};
use crate::dialog::systems::{
    drain_skill_check_requests, forward_dialogue_completed, forward_present_line,
    forward_present_options, handle_yarn_item_commands, handle_yarn_recipe_commands,
    handle_yarn_skill_check_command, handle_yarn_stash_commands, process_dialog_commands,
    refresh_inventory_snapshots, refresh_skill_snapshots, refresh_stash_snapshots,
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
        let mut yarn_plugin =
            YarnSpinnerPlugin::with_yarn_source(YarnFileSource::folder(yarn_dir));
        // Per-module dialog packs live at assets/modules/<name>/dialogs/*.yarn.
        // `YarnFileSource::folder` globs `**/*.yarn` recursively, so a single
        // source rooted at `modules` loads every module's dialog. Added only when
        // the folder exists, because the folder source errors on a missing dir.
        if Path::new("assets/modules").is_dir() {
            yarn_plugin = yarn_plugin.add_yarn_source(YarnFileSource::folder("modules"));
        }
        app.add_plugins(yarn_plugin)
        .insert_resource(DialogSessionRegistry::default())
        .insert_resource(PendingDialogOptions::default())
        .insert_resource(CharacterVarStores::default())
        .insert_resource(PlayerInventorySnapshots::default())
        .insert_resource(PlayerStashSnapshots::default())
        .insert_resource(PlayerSkillSnapshots::default())
        .insert_resource(PendingSkillCheckRequests::default())
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
        // Runs in `PreUpdate` so Yarn `has_item` queries (closures capturing
        // the snapshot Arc) see the previous frame's committed inventory.
        // Running after Update would race with mid-frame `give_item` /
        // `take_item` effects inside the same dialog turn.
        .add_systems(
            PreUpdate,
            (
                refresh_inventory_snapshots,
                refresh_stash_snapshots,
                refresh_skill_snapshots,
            )
                .run_if(simulation_active),
        )
        // Drain queued <<skill_check>> requests once per Update — after the
        // observer chain fires but before the next dialog `Continue` reads
        // `$last_skill_check_*` in an `<<if>>`.
        .add_systems(Update, drain_skill_check_requests.run_if(simulation_active))
        .add_observer(forward_present_line)
        .add_observer(forward_present_options)
        .add_observer(forward_dialogue_completed)
        .add_observer(handle_yarn_item_commands)
        .add_observer(handle_yarn_stash_commands)
        .add_observer(handle_yarn_recipe_commands)
        .add_observer(handle_yarn_skill_check_command);
    }
}
