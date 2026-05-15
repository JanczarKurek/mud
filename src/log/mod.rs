//! Per-character Log: quests, notes, and future free-form sections.
//!
//! Data lives in `CharacterStash` under the key [`LOG_STASH_KEY`], serialized
//! as JSON. The autosave path already round-trips `CharacterStash`, so no
//! separate persistence wiring is needed. Helpers in this module read/write
//! the typed view ([`LogState`]) from the stash.

pub mod commands;
pub mod plugin;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::crafting::CharacterStash;

pub use plugin::{LogClientPlugin, LogServerPlugin};

/// Stash key under which `LogState` is serialized.
pub const LOG_STASH_KEY: &str = "log";

/// Section name reserved for entries written by the quest engine.
pub const QUESTS_SECTION: &str = "Quests";
/// Default user-owned section for free-form notes.
pub const NOTES_SECTION: &str = "Notes";

/// Server-side caps. Enforced by command handlers to keep the per-character
/// JSON payload bounded.
pub const MAX_SECTION_KEY_LEN: usize = 64;
pub const MAX_SUBSECTION_KEY_LEN: usize = 64;
pub const MAX_TITLE_LEN: usize = 200;
pub const MAX_BODY_LEN: usize = 8 * 1024;
pub const MAX_PLAYER_NOTES_LEN: usize = 4 * 1024;
pub const MAX_SECTIONS_PER_PLAYER: usize = 32;
pub const MAX_SUBENTRIES_PER_SECTION: usize = 256;

#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum LogOwner {
    /// Player-owned: created and editable from the UI.
    #[default]
    Player,
    /// Engine-owned: `body` is read-only to the player; only `player_notes`
    /// is editable.
    Engine,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct LogEntry {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub player_notes: String,
    #[serde(default)]
    pub owner: LogOwner,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct LogSection {
    pub subsections: BTreeMap<String, LogEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct LogState {
    pub sections: BTreeMap<String, LogSection>,
}

impl LogState {
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// Parse a `LogState` from `stash["log"]`. Missing / malformed entries
    /// yield an empty state so a corrupted save doesn't lock the player out.
    pub fn from_stash(stash: &CharacterStash) -> Self {
        match stash.get(LOG_STASH_KEY) {
            Some(value) => serde_json::from_value(value.clone()).unwrap_or_default(),
            None => Self::default(),
        }
    }

    /// Serialize `self` back into `stash["log"]`. Empty states are stored
    /// as `null` to avoid pinning an empty object in the JSON.
    pub fn write_to_stash(&self, stash: &mut CharacterStash) {
        if self.is_empty() {
            stash.delete(LOG_STASH_KEY);
            return;
        }
        match serde_json::to_value(self) {
            Ok(value) => stash.set(LOG_STASH_KEY, value),
            Err(err) => bevy::log::warn!("log: failed to serialize LogState: {err}"),
        }
    }

    pub fn section(&self, section: &str) -> Option<&LogSection> {
        self.sections.get(section)
    }

    pub fn entry(&self, section: &str, subsection: &str) -> Option<&LogEntry> {
        self.sections
            .get(section)
            .and_then(|s| s.subsections.get(subsection))
    }

    pub fn entry_mut(&mut self, section: &str, subsection: &str) -> Option<&mut LogEntry> {
        self.sections
            .get_mut(section)
            .and_then(|s| s.subsections.get_mut(subsection))
    }

    /// Insert or replace an entry. Caller is responsible for length
    /// validation and owner-gating.
    pub fn upsert(&mut self, section: String, subsection: String, entry: LogEntry) {
        self.sections
            .entry(section)
            .or_default()
            .subsections
            .insert(subsection, entry);
    }

    /// Remove `subsection` from `section`. Returns the entry if it existed.
    /// Cleans up the section when it becomes empty so we don't accumulate
    /// dangling sections.
    pub fn remove(&mut self, section: &str, subsection: &str) -> Option<LogEntry> {
        let section_entry = self.sections.get_mut(section)?;
        let removed = section_entry.subsections.remove(subsection);
        if section_entry.subsections.is_empty() {
            self.sections.remove(section);
        }
        removed
    }

    /// Total number of subentries across all sections — used to enforce
    /// growth caps on the wire path.
    pub fn subentry_count(&self) -> usize {
        self.sections.values().map(|s| s.subsections.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_through_stash() {
        let mut log = LogState::default();
        log.upsert(
            NOTES_SECTION.to_owned(),
            "Strategy".to_owned(),
            LogEntry {
                title: "Strategy".to_owned(),
                body: "Try the side door".to_owned(),
                player_notes: String::new(),
                owner: LogOwner::Player,
            },
        );
        log.upsert(
            QUESTS_SECTION.to_owned(),
            "demo_hunter".to_owned(),
            LogEntry {
                title: "Hunt the Goblin".to_owned(),
                body: "Travel north".to_owned(),
                player_notes: "remember the cave".to_owned(),
                owner: LogOwner::Engine,
            },
        );

        let mut stash = CharacterStash::default();
        log.write_to_stash(&mut stash);
        let restored = LogState::from_stash(&stash);
        assert_eq!(restored, log);
    }

    #[test]
    fn malformed_stash_entry_falls_back_to_empty() {
        let mut stash = CharacterStash::default();
        stash.set(LOG_STASH_KEY, serde_json::json!("not a log"));
        assert!(LogState::from_stash(&stash).is_empty());
    }

    #[test]
    fn empty_state_clears_stash_key() {
        let mut stash = CharacterStash::default();
        stash.set(
            LOG_STASH_KEY,
            serde_json::json!({"sections": {"X": {"subsections": {}}}}),
        );
        let empty = LogState::default();
        empty.write_to_stash(&mut stash);
        assert!(!stash.has(LOG_STASH_KEY));
    }

    #[test]
    fn remove_drops_empty_section() {
        let mut log = LogState::default();
        log.upsert(
            NOTES_SECTION.to_owned(),
            "n1".to_owned(),
            LogEntry::default(),
        );
        log.remove(NOTES_SECTION, "n1");
        assert!(log.section(NOTES_SECTION).is_none());
    }
}
