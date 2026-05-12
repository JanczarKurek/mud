//! Generic per-character JSON stash. Any subsystem (crafting, quests, future
//! reputation/discovery features) can read and write here without touching
//! `PlayerStateDump`. The component is serialized as part of `PlayerStateDump`
//! so entries round-trip through the accounts DB automatically.
//!
//! Keys are namespaced strings (e.g. `recipes:known`, `quest:hunter:state`).
//! Values are `serde_json::Value` so any JSON-shaped state fits.

use std::collections::{BTreeSet, HashMap};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const LEARNED_RECIPES_KEY: &str = "recipes:known";

#[derive(Component, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct CharacterStash {
    pub entries: HashMap<String, serde_json::Value>,
}

impl CharacterStash {
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.entries.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.entries.insert(key.into(), value);
    }

    pub fn has(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn delete(&mut self, key: &str) -> Option<serde_json::Value> {
        self.entries.remove(key)
    }

    /// `BTreeSet` for deterministic wire ordering — the projection diff
    /// compares serialized sets and a `HashSet` would produce spurious
    /// "changed" events on iteration-order flips.
    pub fn learned_recipes(&self) -> BTreeSet<String> {
        match self.entries.get(LEARNED_RECIPES_KEY) {
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => BTreeSet::new(),
        }
    }

    /// Returns `true` iff `recipe_id` was newly added (i.e. the caller should
    /// emit `RecipeLearned`). Idempotent on repeated calls.
    pub fn add_learned_recipe(&mut self, recipe_id: &str) -> bool {
        let mut set = self.learned_recipes();
        if !set.insert(recipe_id.to_owned()) {
            return false;
        }
        let array: Vec<serde_json::Value> =
            set.into_iter().map(serde_json::Value::String).collect();
        self.entries.insert(
            LEARNED_RECIPES_KEY.to_owned(),
            serde_json::Value::Array(array),
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_learned_recipe_is_idempotent() {
        let mut stash = CharacterStash::default();
        assert!(stash.add_learned_recipe("torch"));
        assert!(!stash.add_learned_recipe("torch"));
        assert!(stash.add_learned_recipe("fishing_rod"));

        let set = stash.learned_recipes();
        assert_eq!(set.len(), 2);
        assert!(set.contains("torch"));
        assert!(set.contains("fishing_rod"));
    }

    #[test]
    fn round_trips_through_json() {
        let mut stash = CharacterStash::default();
        stash.add_learned_recipe("torch");
        stash.set("quest:hunter:state", serde_json::json!({ "rats": 2 }));

        let json = serde_json::to_string(&stash).unwrap();
        let restored: CharacterStash = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, stash);
    }

    #[test]
    fn malformed_recipes_entry_returns_empty_set() {
        let mut stash = CharacterStash::default();
        stash.set(LEARNED_RECIPES_KEY, serde_json::json!("not an array"));
        assert!(stash.learned_recipes().is_empty());
    }
}
