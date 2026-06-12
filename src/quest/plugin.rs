//! Registers the Python `QuestEngine` + its driving systems. Added to the
//! server side of the app (EmbeddedClient, HeadlessServer) — clients don't
//! need a Python VM.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::quest::engine::QuestEngine;
use crate::quest::events::PendingQuestEvents;
use crate::quest::systems::{
    drain_quest_commands, drain_quest_events, handle_yarn_quest_commands,
    mirror_quest_state_to_stash, restore_quest_state_on_player_added, PendingQuestCommands,
};

pub struct QuestPlugin {
    /// Directory to load `.py` files from. Defaults to `assets/quests/`.
    pub quest_dir: Option<PathBuf>,
}

impl Default for QuestPlugin {
    fn default() -> Self {
        Self { quest_dir: None }
    }
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
            .add_systems(
                Update,
                (
                    restore_quest_state_on_player_added,
                    drain_quest_commands,
                    drain_quest_events,
                )
                    .run_if(simulation_active),
            )
            // Run in `Last`, ahead of `persist_disconnected_players` /
            // `autosave_all_players`, so the stash carries the freshest
            // snapshot of every quest's Python `state` dict.
            .add_systems(Last, mirror_quest_state_to_stash)
            .add_observer(handle_yarn_quest_commands);
    }
}
