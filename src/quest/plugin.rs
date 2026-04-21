//! Registers the Python `QuestEngine` + its driving systems. Added to the
//! server side of the app (EmbeddedClient, HeadlessServer) — clients don't
//! need a Python VM.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::quest::engine::QuestEngine;
use crate::quest::events::PendingQuestEvents;
use crate::quest::systems::{
    drain_quest_commands, drain_quest_events, handle_yarn_quest_commands, PendingQuestCommands,
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

        app.insert_non_send_resource(engine)
            .insert_resource(PendingQuestCommands::default())
            .insert_resource(PendingQuestEvents::default())
            .add_systems(
                Update,
                (drain_quest_commands, drain_quest_events).run_if(simulation_active),
            )
            .add_observer(handle_yarn_quest_commands);
    }
}
