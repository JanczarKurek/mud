use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bevy::prelude::*;
use bevy_yarnspinner::prelude::OptionId;

use crate::dialog::variable_storage::PersistentVariableStorage;

#[derive(Resource, Default)]
pub struct DialogSessionRegistry {
    pub next_session_id: u64,
    pub by_id: HashMap<u64, DialogSessionHandle>,
}

#[derive(Clone, Copy)]
pub struct DialogSessionHandle {
    pub runner_entity: Entity,
    pub player_id: u64,
    pub npc_object_id: u64,
}

impl DialogSessionRegistry {
    pub fn allocate(&mut self) -> u64 {
        self.next_session_id = self.next_session_id.wrapping_add(1);
        if self.next_session_id == 0 {
            self.next_session_id = 1;
        }
        self.next_session_id
    }
}

/// Last options presented to each session, recorded in the order we emitted
/// `GameUiEvent::DialogOptions`. A `GameCommand::DialogChoose { option_idx }`
/// is resolved by indexing into this vec to recover the Yarn `OptionId`.
#[derive(Resource, Default)]
pub struct PendingDialogOptions {
    pub by_session: HashMap<u64, Vec<OptionId>>,
}

/// Per-character Yarn variable stores. `PersistentVariableStorage` clones
/// share the same backing `Arc<RwLock<HashMap>>`, so every `DialogueRunner` we
/// spawn for the same player sees the same `$vars`. Unlike
/// `MemoryVariableStorage`, it keeps existing values on `extend` so
/// `<<declare $x = default>>` in a second runner doesn't clobber a flag set
/// by the first.
#[derive(Resource, Default)]
pub struct CharacterVarStores {
    pub by_player: HashMap<u64, PersistentVariableStorage>,
}

impl CharacterVarStores {
    pub fn get_or_insert(&mut self, player_id: u64) -> PersistentVariableStorage {
        self.by_player
            .entry(player_id)
            .or_insert_with(PersistentVariableStorage::new)
            .clone()
    }

    /// Returns the player's current variable snapshot, or an empty map if
    /// they've never opened a dialog this session.
    pub fn snapshot_for(&self, player_id: u64) -> HashMap<String, crate::dialog::variable_storage::YarnValueDump> {
        self.by_player
            .get(&player_id)
            .map(|store| store.snapshot())
            .unwrap_or_default()
    }

    /// Install a persisted variable snapshot for the given player, replacing
    /// any existing state. Called at login.
    pub fn restore(
        &mut self,
        player_id: u64,
        values: HashMap<String, crate::dialog::variable_storage::YarnValueDump>,
    ) {
        let store = self
            .by_player
            .entry(player_id)
            .or_insert_with(PersistentVariableStorage::new);
        store.restore(values);
    }
}

/// Shared snapshot of each player's inventory aggregated by object type_id.
/// Written each Update by `refresh_inventory_snapshots`, read from Yarn
/// functions registered on `DialogueRunner`s. An `Arc<RwLock<...>>` rather
/// than a Bevy `Res<_>` because Yarn functions are plain closures without
/// access to the ECS world.
#[derive(Resource, Default, Clone)]
pub struct PlayerInventorySnapshots {
    pub by_player: Arc<RwLock<HashMap<u64, HashMap<String, u32>>>>,
}
