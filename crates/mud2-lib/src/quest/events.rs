//! Gameplay-scale events that Python quests may subscribe to.
//!
//! Distinct from `GameEvent` (state-replication to clients). A `QuestEvent` is
//! a discrete thing that *happened*, surfaced so that Python quest modules
//! with a matching `subscribes_to` can react to it. Events whose `kind()` no
//! quest subscribes to are dropped by `drain_quest_events` without ever
//! crossing into Python — this is the "no firehose" guarantee from the design
//! doc.
//!
//! The kind string is the subscription key quests use; keep it stable.

use bevy::prelude::*;

#[derive(Clone, Debug)]
pub enum QuestEvent {
    /// An NPC/world object died from combat damage. `killer_player_id` is set
    /// when the killer is a player (embedded mode or remote); `None` for
    /// NPC-on-NPC kills (which we currently don't produce but might).
    ObjectKilled {
        type_id: String,
        killer_player_id: Option<u64>,
    },
}

impl QuestEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            QuestEvent::ObjectKilled { .. } => "ObjectKilled",
        }
    }
}

#[derive(Resource, Default)]
pub struct PendingQuestEvents {
    pub events: Vec<QuestEvent>,
}
