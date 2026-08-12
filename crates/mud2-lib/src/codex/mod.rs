//! Per-character codex knowledge: how much the player has learned about each
//! NPC type (the People dossiers) and each creature type (the Bestiary).
//!
//! This module holds only the *numeric* knowledge — tier counters and kill
//! tallies, keyed by `definition_id`. The readable prose lives in
//! [`crate::log::LogState`] as engine-owned entries, composed server-side by
//! [`updates::apply_codex_updates`]. Splitting it this way lets the Log window
//! render codex entries with no extra data source, and gives the player a
//! `player_notes` box on every dossier for free.
//!
//! Data lives in `CharacterStash` under [`CODEX_STASH_KEY`], exactly like
//! `LogState` under `log::LOG_STASH_KEY`, so the autosave path round-trips it
//! with no extra persistence wiring.
//!
//! Knowledge is **monotonic**: tiers only ever rise, never skip, and never
//! regress. The `raise_*` methods return `true` only when the stored value
//! actually moved, which is what keeps the log from being re-upserted (and
//! therefore re-replicated in full) on every failed observation roll.

// Prose composition and the apply system are server-only: they read the
// definition catalogue and write the authoritative log. `CodexState` itself
// stays ungated so persistence and tests can name the type anywhere.
#[cfg(feature = "server-sim")]
pub mod compose;
#[cfg(feature = "server-sim")]
pub mod updates;

use std::collections::BTreeMap;

use bevy::prelude::SystemSet;
use serde::{Deserialize, Serialize};

use crate::crafting::CharacterStash;

/// Ordering handles for the codex pipeline. Declared here (and configured once
/// in `GameServerPlugin`) because the members live in three different plugins:
/// `Reveal` in `NpcPlugin`, `Apply` in `GameServerPlugin`, and the consumer
/// (`log::commands::process_log_commands`) in `LogServerPlugin`. A plain
/// `.before(some_fn)` across a plugin boundary is silently dropped when the
/// target plugin isn't present — a set edge holds regardless.
#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CodexSet {
    /// Systems that discover knowledge and queue [`CodexUpdate`]s.
    Reveal,
    /// The single writer that folds the queue into the stash and the log.
    Apply,
}

#[cfg(feature = "server-sim")]
pub use updates::{CodexUpdate, PendingCodexKills, PendingCodexUpdates};

/// Stash key under which [`CodexState`] is serialized.
pub const CODEX_STASH_KEY: &str = "codex";

/// Highest reachable tier in either ladder.
pub const MAX_TIER: u8 = 4;

/// Per-character knowledge counters.
///
/// Keyed by **`definition_id`**, so knowledge is per NPC/creature *type*, not
/// per placed instance: reading one `townsfolk` fills in the dossier for all
/// of them. `BTreeMap` (not `HashMap`) for deterministic serialization — the
/// same reasoning as `CharacterStash::learned_recipes`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct CodexState {
    /// definition_id -> highest People dossier tier reached (1..=3).
    #[serde(default)]
    pub npc_tier: BTreeMap<String, u8>,
    /// definition_id -> highest Bestiary tier reached (1..=[`MAX_TIER`]).
    #[serde(default)]
    pub mob_tier: BTreeMap<String, u8>,
    /// definition_id -> lifetime kills credited to this character.
    #[serde(default)]
    pub kills: BTreeMap<String, u32>,
}

impl CodexState {
    pub fn is_empty(&self) -> bool {
        self.npc_tier.is_empty() && self.mob_tier.is_empty() && self.kills.is_empty()
    }

    /// Parse a `CodexState` from `stash["codex"]`. Missing / malformed entries
    /// yield an empty state so a corrupted save doesn't lock the player out.
    pub fn from_stash(stash: &CharacterStash) -> Self {
        match stash.get(CODEX_STASH_KEY) {
            Some(value) => serde_json::from_value(value.clone()).unwrap_or_default(),
            None => Self::default(),
        }
    }

    /// Serialize `self` back into `stash["codex"]`. Empty states are removed
    /// rather than stored, to avoid pinning an empty object in the JSON.
    pub fn write_to_stash(&self, stash: &mut CharacterStash) {
        if self.is_empty() {
            stash.delete(CODEX_STASH_KEY);
            return;
        }
        match serde_json::to_value(self) {
            Ok(value) => stash.set(CODEX_STASH_KEY, value),
            Err(err) => bevy::log::warn!("codex: failed to serialize CodexState: {err}"),
        }
    }

    pub fn npc_tier(&self, definition_id: &str) -> u8 {
        self.npc_tier.get(definition_id).copied().unwrap_or(0)
    }

    pub fn mob_tier(&self, definition_id: &str) -> u8 {
        self.mob_tier.get(definition_id).copied().unwrap_or(0)
    }

    pub fn kills_of(&self, definition_id: &str) -> u32 {
        self.kills.get(definition_id).copied().unwrap_or(0)
    }

    /// Raise the People tier for `definition_id`. Returns `true` iff the
    /// stored tier actually rose — callers use this to decide whether to
    /// re-compose the log entry.
    pub fn raise_npc_tier(&mut self, definition_id: &str, tier: u8) -> bool {
        raise(&mut self.npc_tier, definition_id, tier)
    }

    /// Raise the Bestiary tier for `definition_id`. Returns `true` iff the
    /// stored tier actually rose.
    pub fn raise_mob_tier(&mut self, definition_id: &str, tier: u8) -> bool {
        raise(&mut self.mob_tier, definition_id, tier)
    }

    /// Credit one kill of `definition_id` to this character; returns the new
    /// running total.
    pub fn record_kill(&mut self, definition_id: &str) -> u32 {
        let slot = self.kills.entry(definition_id.to_owned()).or_insert(0);
        *slot = slot.saturating_add(1);
        *slot
    }
}

/// Shared monotonic-raise helper for both ladders. Clamps to [`MAX_TIER`] and
/// ignores a zero or regressing target.
fn raise(map: &mut BTreeMap<String, u8>, definition_id: &str, tier: u8) -> bool {
    let tier = tier.min(MAX_TIER);
    if tier == 0 {
        return false;
    }
    let slot = map.entry(definition_id.to_owned()).or_insert(0);
    if *slot >= tier {
        return false;
    }
    *slot = tier;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_stash() {
        let mut codex = CodexState::default();
        codex.raise_npc_tier("villager", 2);
        codex.raise_mob_tier("wolf", 3);
        codex.record_kill("wolf");
        codex.record_kill("wolf");

        let mut stash = CharacterStash::default();
        codex.write_to_stash(&mut stash);
        assert_eq!(CodexState::from_stash(&stash), codex);
    }

    #[test]
    fn malformed_stash_entry_falls_back_to_empty() {
        let mut stash = CharacterStash::default();
        stash.set(CODEX_STASH_KEY, serde_json::json!("not a codex"));
        assert!(CodexState::from_stash(&stash).is_empty());
    }

    #[test]
    fn empty_state_clears_stash_key() {
        let mut stash = CharacterStash::default();
        stash.set(
            CODEX_STASH_KEY,
            serde_json::json!({ "kills": { "rat": 1 } }),
        );
        CodexState::default().write_to_stash(&mut stash);
        assert!(!stash.has(CODEX_STASH_KEY));
    }

    #[test]
    fn raise_tier_is_monotonic() {
        let mut codex = CodexState::default();
        assert!(codex.raise_mob_tier("wolf", 2));
        // Same tier again: no change, so no log re-upsert.
        assert!(!codex.raise_mob_tier("wolf", 2));
        // Regression is ignored outright.
        assert!(!codex.raise_mob_tier("wolf", 1));
        assert_eq!(codex.mob_tier("wolf"), 2);
        assert!(codex.raise_mob_tier("wolf", 3));
        assert_eq!(codex.mob_tier("wolf"), 3);
    }

    #[test]
    fn raise_clamps_to_max_and_ignores_zero() {
        let mut codex = CodexState::default();
        assert!(!codex.raise_npc_tier("judge", 0));
        assert!(codex.raise_npc_tier("judge", 99));
        assert_eq!(codex.npc_tier("judge"), MAX_TIER);
    }

    #[test]
    fn kills_accumulate_per_definition() {
        let mut codex = CodexState::default();
        assert_eq!(codex.record_kill("rat"), 1);
        assert_eq!(codex.record_kill("rat"), 2);
        assert_eq!(codex.record_kill("wolf"), 1);
        assert_eq!(codex.kills_of("rat"), 2);
        assert_eq!(codex.kills_of("goblin"), 0);
    }
}
