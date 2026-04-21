use std::collections::HashMap;

use bevy::prelude::*;
use bevy_yarnspinner::prelude::OptionId;

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
