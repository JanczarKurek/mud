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
    forward_present_options, handle_yarn_item_commands, handle_yarn_log_write_command,
    handle_yarn_recipe_commands, handle_yarn_skill_check_command, handle_yarn_stash_commands,
    process_dialog_commands, refresh_inventory_snapshots, refresh_skill_snapshots,
    refresh_stash_snapshots,
};
use crate::game::CommandIntercept;

/// Registers bevy_yarnspinner and the server-side dialog plumbing.
///
/// Runs on both `EmbeddedClient` and `HeadlessServer`. Yarn requires
/// `AssetPlugin` to be built *before* this plugin (yarnspinner `expect`s
/// `AssetServer`); `DefaultPlugins` covers that for the embedded client, and
/// the headless runtime adds `AssetPlugin` explicitly on top of
/// `MinimalPlugins`.
pub struct DialogServerPlugin;

impl Plugin for DialogServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(YarnSpinnerPlugin::with_yarn_sources(
            validated_yarn_sources(),
        ))
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
        .add_observer(handle_yarn_log_write_command)
        .add_observer(handle_yarn_recipe_commands)
        .add_observer(handle_yarn_skill_check_command);
    }
}

/// One [`YarnFileSource`] per `.yarn` file under `assets/dialogs/` and
/// `assets/modules/` that actually parses.
///
/// Files are registered individually instead of via `YarnFileSource::folder`
/// because bevy_yarnspinner treats a parse error as "this asset never finished
/// loading": with folder sources, one broken file keeps `YarnProject` from
/// ever being inserted and every dialog in the game dies with no in-game
/// symptom. Pre-parsing here lets us exclude just the broken file with a loud
/// error and keep the rest of the project alive.
///
/// Enumeration is CWD-relative like the rest of module asset discovery
/// (`crate::assets::module_dirs_with_names`); the returned sources are
/// asset-root-relative, so the two agree when the game runs from the project
/// root.
fn validated_yarn_sources() -> Vec<YarnFileSource> {
    let mut files = Vec::new();
    for root in ["assets/dialogs", "assets/modules"] {
        collect_yarn_files(Path::new(root), &mut files);
    }
    files.sort();

    let mut sources = Vec::new();
    for path in files {
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(err) => {
                bevy::log::error!(
                    "dialog: cannot read {}: {err}; excluded from the Yarn project",
                    path.display()
                );
                continue;
            }
        };
        if let Err(diagnostics) = check_yarn_parses(&path.to_string_lossy(), &source) {
            bevy::log::error!(
                "dialog: {} failed to parse and is excluded from the Yarn project \
                 (other dialogs keep working):\n{diagnostics}",
                path.display()
            );
            continue;
        }
        let asset_path: PathBuf = path.strip_prefix("assets").unwrap_or(&path).to_path_buf();
        sources.push(YarnFileSource::file(asset_path));
    }
    if sources.is_empty() {
        bevy::log::error!("dialog: no loadable .yarn files found; all dialogs are disabled");
    }
    sources
}

pub(crate) fn collect_yarn_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yarn_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "yarn") {
            out.push(path);
        }
    }
}

/// Mirrors the `StringsOnly` compile that bevy_yarnspinner's asset loader runs
/// on every `.yarn` file: parsing happens before the strings-only early break,
/// so this catches exactly the class of error that would wedge the asset load.
fn check_yarn_parses(file_name: &str, source: &str) -> Result<(), String> {
    use yarnspinner::compiler::{CompilationType, Compiler, File};
    Compiler::new()
        .with_compilation_type(CompilationType::StringsOnly)
        .add_file(File {
            file_name: file_name.to_string(),
            source: source.to_string(),
        })
        .compile()
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `.yarn` file shipped in the repo must parse, because a single
    /// unparsable file used to disable all dialogs game-wide (and now gets
    /// excluded at startup, which still means its NPCs go mute).
    #[test]
    fn all_repo_yarn_files_parse() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        for root in ["assets/dialogs", "assets/modules"] {
            collect_yarn_files(&manifest_dir.join(root), &mut files);
        }
        files.sort();
        assert!(
            !files.is_empty(),
            "no .yarn files found under assets/ — glob rot?"
        );

        let mut failures = Vec::new();
        for path in &files {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            if let Err(diagnostics) = check_yarn_parses(&path.to_string_lossy(), &source) {
                failures.push(format!("{}:\n{diagnostics}", path.display()));
            }
        }
        assert!(
            failures.is_empty(),
            "{} yarn file(s) failed to parse:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }

    /// Yarn variables share one project-wide namespace: declaring the same
    /// `$variable` twice — anywhere, across any two files — is a hard Y001
    /// error at full compile time that kills the whole Yarn project. The
    /// `StringsOnly` pre-parse above cannot see it (it's a cross-file check),
    /// so guard it textually here.
    #[test]
    fn yarn_declares_are_unique_across_files() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        for root in ["assets/dialogs", "assets/modules"] {
            collect_yarn_files(&manifest_dir.join(root), &mut files);
        }
        files.sort();

        let mut seen: std::collections::BTreeMap<String, PathBuf> = Default::default();
        let mut failures = Vec::new();
        for path in &files {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            for line in source.lines() {
                // Skip comments so guidance like "never re-declare $x" in a
                // `//` line doesn't count as a declaration.
                let line = line.split("//").next().unwrap_or("");
                let Some(pos) = line.find("<<declare $") else {
                    continue;
                };
                let name: String = line[pos + "<<declare $".len()..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if let Some(first) = seen.get(&name) {
                    failures.push(format!(
                        "${name} declared in both {} and {}",
                        first.display(),
                        path.display()
                    ));
                } else {
                    seen.insert(name, path.clone());
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} duplicate yarn declare(s) (hard Y001 compile error at runtime):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
