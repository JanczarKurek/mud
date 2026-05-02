//! Quest-engine adapter for the shared `world` Python API.
//!
//! Quest hooks see the same `world` module the admin console exposes,
//! routed through this `QuestApiContext`. Differences vs. admin:
//! - Admin-only verbs (`teleport`, `despawn`, `set_vitals`,
//!   `set_floor`, `reset`) raise `PermissionError`.
//! - `set_var` / `get_var` / `complete_quest` / `fail_quest` are
//!   permitted (they're the quest-only verbs).
//! - `players()` is filtered to just the caller (admin sees all).
//!
//! The systems caller (`drain_quest_commands` / `drain_quest_events`)
//! builds one `QuestApiContext` per Python invocation, installs it via
//! `crate::scripting_api::install_ctx`, and drains the queued
//! `GameCommand`s + completed/failed quest ids after Python returns.

use std::sync::Mutex;

use bevy_yarnspinner::prelude::{VariableStorage, YarnValue};

use crate::dialog::variable_storage::{PersistentVariableStorage, YarnValueDump};
use crate::game::commands::GameCommand;
use crate::scripting_api::{ApiContext, ApiError, WorldSnapshot};

#[derive(Default)]
pub struct QuestApiOutbox {
    pub commands: Vec<GameCommand>,
    pub log_lines: Vec<String>,
    pub quest_complete: Vec<String>,
    pub quest_fail: Vec<String>,
}

pub struct QuestApiContext {
    snapshot: WorldSnapshot,
    player_id: u64,
    var_store: Option<PersistentVariableStorage>,
    inner: Mutex<QuestApiOutbox>,
}

impl QuestApiContext {
    pub fn new(
        snapshot: WorldSnapshot,
        player_id: u64,
        var_store: Option<PersistentVariableStorage>,
    ) -> Self {
        Self {
            snapshot,
            player_id,
            var_store,
            inner: Mutex::new(QuestApiOutbox::default()),
        }
    }

    /// Take the queued effects out of the context. Called after the
    /// Python hook returns so the caller can replay them as
    /// `GameCommand`s and quest-state mutations.
    pub fn take_outbox(&self) -> QuestApiOutbox {
        std::mem::take(&mut *self.inner.lock().expect("quest api outbox poisoned"))
    }
}

impl ApiContext for QuestApiContext {
    fn is_admin(&self) -> bool {
        false
    }

    fn caller_player_id(&self) -> Option<u64> {
        Some(self.player_id)
    }

    fn snapshot(&self) -> &WorldSnapshot {
        &self.snapshot
    }

    fn log(&self, message: String) {
        self.inner
            .lock()
            .expect("quest api outbox poisoned")
            .log_lines
            .push(message);
    }

    fn queue_command(&self, command: GameCommand) -> Result<(), ApiError> {
        match command {
            GameCommand::AdminTeleport { .. }
            | GameCommand::AdminDespawn { .. }
            | GameCommand::AdminSetVitals { .. }
            | GameCommand::AdminSetObjectState { .. }
            | GameCommand::EditorSetFloorTile { .. } => Err(ApiError::NotPermitted(
                "this command is admin-only and cannot be issued from a quest hook",
            )),
            other => {
                self.inner
                    .lock()
                    .expect("quest api outbox poisoned")
                    .commands
                    .push(other);
                Ok(())
            }
        }
    }

    fn end_quest(&self, quest_id: &str, failed: bool) -> Result<(), ApiError> {
        let mut outbox = self.inner.lock().expect("quest api outbox poisoned");
        if failed {
            outbox.quest_fail.push(quest_id.to_owned());
        } else {
            outbox.quest_complete.push(quest_id.to_owned());
        }
        Ok(())
    }

    fn set_yarn_var(&self, name: &str, value: YarnValueDump) -> Result<(), ApiError> {
        let yarn_name = ensure_dollar(name);
        let Some(store) = self.var_store.as_ref() else {
            return Ok(());
        };
        // PersistentVariableStorage::set takes &mut self, but the underlying
        // state is Arc<RwLock<...>>; cloning is cheap. Yarn's `<<if $foo>>`
        // reads the same shared state.
        let mut store_clone = store.clone();
        let yarn_value: YarnValue = value.into();
        store_clone
            .set(yarn_name, yarn_value)
            .map_err(|err| ApiError::Invalid(format!("set_var failed: {err}")))
    }

    fn get_yarn_var(&self, name: &str) -> Result<Option<YarnValueDump>, ApiError> {
        let yarn_name = ensure_dollar(name);
        Ok(self
            .var_store
            .as_ref()
            .and_then(|store| store.snapshot().get(&yarn_name).cloned()))
    }
}

fn ensure_dollar(name: &str) -> String {
    if name.starts_with('$') {
        name.to_owned()
    } else {
        format!("${name}")
    }
}
