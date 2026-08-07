//! Registers the Python `QuestEngine` + its driving systems. Added to the
//! server side of the app (EmbeddedClient, HeadlessServer) — clients don't
//! need a Python VM.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::quest::engine::QuestEngine;
use crate::quest::events::PendingQuestEvents;
use crate::quest::journal::{evaluate_quest_journals, load_quest_journals};
use crate::quest::systems::{
    drain_quest_commands, drain_quest_events, handle_yarn_quest_commands,
    mirror_quest_state_to_stash, restore_quest_state_on_player_added, PendingQuestCommands,
};

#[derive(Default)]
pub struct QuestPlugin {
    /// Directory to load `.py` files from. Defaults to `assets/quests/`.
    pub quest_dir: Option<PathBuf>,
}

impl Plugin for QuestPlugin {
    fn build(&self, app: &mut App) {
        let mut engine = QuestEngine::new();
        let dir = self
            .quest_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("assets/quests"));
        engine.load_from(&dir);
        // Per-module quest packs: assets/modules/<name>/quests/*.py. Each quest
        // registers under `<name>/<stem>` so its id matches the qualified
        // `<<start_quest>>` / `<<complete_quest>>` arguments build-module emits.
        // `load_from*` accumulates into the engine (rebuilding subscriptions each
        // call), overlaying module quests on top of the global ones.
        for (module, module_quests) in crate::assets::module_dirs_with_names("quests") {
            engine.load_from_with_prefix(&module_quests, &format!("{module}/"));
        }

        app.insert_non_send_resource(engine)
            .insert_resource(PendingQuestCommands::default())
            .insert_resource(PendingQuestEvents::default())
            .insert_resource(load_quest_journals())
            // Chained, and ahead of `CommandIntercept`: the drains and the
            // journal evaluator push `GameCommand`s (log upserts, script
            // outbox commands) that `process_log_commands` must see this
            // frame — anything still queued when `process_game_commands`
            // runs is dropped.
            .add_systems(
                Update,
                (
                    restore_quest_state_on_player_added,
                    drain_quest_commands,
                    drain_quest_events,
                    evaluate_quest_journals,
                )
                    .chain()
                    .before(crate::game::CommandIntercept)
                    .run_if(simulation_active),
            )
            // Run in `Last`, ahead of `persist_disconnected_players` /
            // `autosave_all_players`, so the stash carries the freshest
            // snapshot of every quest's Python `state` dict.
            .add_systems(Last, mirror_quest_state_to_stash)
            .add_observer(handle_yarn_quest_commands);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Quest ids that exist on disk, matching the registration convention:
    /// `assets/quests/<stem>.py` → `<stem>`,
    /// `assets/modules/<m>/quests/<stem>.py` → `<m>/<stem>`.
    fn on_disk_quest_ids(manifest_dir: &Path) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        let push_stems = |dir: &Path, prefix: &str, ids: &mut BTreeSet<String>| {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("py") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.insert(format!("{prefix}{stem}"));
                }
            }
        };
        push_stems(&manifest_dir.join("assets/quests"), "", &mut ids);
        let modules_root = manifest_dir.join("assets/modules");
        if let Ok(modules) = std::fs::read_dir(&modules_root) {
            for module in modules.flatten() {
                let Ok(name) = module.file_name().into_string() else {
                    continue;
                };
                push_stems(&module.path().join("quests"), &format!("{name}/"), &mut ids);
            }
        }
        ids
    }

    /// Extract `(command, quest_id)` pairs from every quest command in a yarn
    /// source. A marker whose first argument isn't a quoted string is
    /// reported as a malformed id so it fails the test loudly.
    fn yarn_quest_references(source: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for command in ["start_quest", "complete_quest", "quest_command"] {
            let marker = format!("<<{command}");
            for (pos, _) in source.match_indices(&marker) {
                let rest = source[pos + marker.len()..].trim_start();
                let id = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split('"').next())
                    .unwrap_or("<malformed: first arg not a quoted string>");
                out.push((command.to_owned(), id.to_owned()));
            }
        }
        out
    }

    /// Every quest id referenced by `<<start_quest>>` / `<<complete_quest>>` /
    /// `<<quest_command>>` in shipped yarn must have a `.py` script on disk —
    /// otherwise the command warns server-side and silently no-ops in game
    /// (the Hollow Bell launch bug: three quests were "accepted" in dialog
    /// but never existed in the engine).
    #[test]
    fn yarn_quest_ids_have_registered_scripts() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let known_ids = on_disk_quest_ids(manifest_dir);
        assert!(
            !known_ids.is_empty(),
            "no quest .py files found under assets/ — glob rot?"
        );

        let mut files = Vec::new();
        for root in ["assets/dialogs", "assets/modules"] {
            crate::dialog::plugin::collect_yarn_files(&manifest_dir.join(root), &mut files);
        }
        files.sort();

        let mut failures = Vec::new();
        for path in &files {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            for (command, quest_id) in yarn_quest_references(&source) {
                if !known_ids.contains(&quest_id) {
                    failures.push(format!(
                        "{}: <<{command}>> references quest '{quest_id}' with no .py script",
                        path.display()
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} dangling yarn quest reference(s):\n{}\nknown quest ids: {:?}",
            failures.len(),
            failures.join("\n"),
            known_ids
        );
    }
}
