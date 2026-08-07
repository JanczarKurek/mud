//! Per-character Yarn variable storage.
//!
//! We can't use `MemoryVariableStorage` directly: Yarn's `Dialogue::new` and
//! `replace_program` call `VariableStorage::extend` with the program's initial
//! values (from `<<declare $x = default>>` lines). Per the trait contract,
//! `extend` must *overwrite* existing entries — which wipes out quest flags
//! every time we spawn a new `DialogueRunner` for the same player.
//!
//! `PersistentVariableStorage` is a thin shim over an `Arc<RwLock<HashMap>>`
//! that implements `VariableStorage` faithfully *except* for `extend`, which
//! only inserts keys that aren't already present. This preserves cross-session
//! flags while still honoring first-time default initialization.

use std::any::Any;
use std::collections::HashMap as StdHashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use bevy::platform::collections::HashMap;
use bevy_yarnspinner::prelude::{VariableStorage, YarnValue};
use serde::{Deserialize, Serialize};
use yarnspinner::runtime::VariableStorageError;

/// Serializable mirror of `YarnValue`. Yarn's own `YarnValue` gates its serde
/// impl on a feature we don't enable, and we'd rather not pull one in just for
/// a three-variant enum — any future divergence between the two is a compile
/// error via the `From` impls below.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum YarnValueDump {
    Number(f32),
    String(String),
    Boolean(bool),
}

impl From<&YarnValue> for YarnValueDump {
    fn from(value: &YarnValue) -> Self {
        match value {
            YarnValue::Number(n) => Self::Number(*n),
            YarnValue::String(s) => Self::String(s.clone()),
            YarnValue::Boolean(b) => Self::Boolean(*b),
        }
    }
}

impl From<YarnValueDump> for YarnValue {
    fn from(value: YarnValueDump) -> Self {
        match value {
            YarnValueDump::Number(n) => Self::Number(n),
            YarnValueDump::String(s) => Self::String(s),
            YarnValueDump::Boolean(b) => Self::Boolean(b),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PersistentVariableStorage {
    inner: Arc<RwLock<StdHashMap<String, YarnValue>>>,
    /// Bumped on every mutation (`set`, `clear`, `restore`, and `extend` when
    /// it actually inserts). Interior mutability means Bevy change detection
    /// can't see writes; systems poll this instead (see
    /// `quest::journal::evaluate_quest_journals`).
    generation: Arc<AtomicU64>,
}

impl PersistentVariableStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Monotonic mutation counter shared by all clones of this store.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    fn validate_name(name: &str) -> Result<(), VariableStorageError> {
        if name.starts_with('$') {
            Ok(())
        } else {
            Err(VariableStorageError::InvalidVariableName {
                name: name.to_owned(),
            })
        }
    }

    /// Snapshot the current contents for persistence. Returns a plain `HashMap`
    /// so callers can serialize it without being coupled to the `VariableStorage`
    /// trait.
    pub fn snapshot(&self) -> StdHashMap<String, YarnValueDump> {
        self.inner
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), YarnValueDump::from(v)))
            .collect()
    }

    /// Replace all stored variables with the given snapshot. Used at login to
    /// restore persisted state before any `DialogueRunner` is constructed.
    pub fn restore(&self, values: StdHashMap<String, YarnValueDump>) {
        let mut guard = self.inner.write().unwrap();
        guard.clear();
        for (name, value) in values {
            guard.insert(name, value.into());
        }
        drop(guard);
        // Restoring at login counts as a mutation so pollers re-evaluate
        // against the freshly-loaded variables.
        self.bump_generation();
    }
}

impl VariableStorage for PersistentVariableStorage {
    fn clone_shallow(&self) -> Box<dyn VariableStorage> {
        Box::new(self.clone())
    }

    fn set(&mut self, name: String, value: YarnValue) -> Result<(), VariableStorageError> {
        Self::validate_name(&name)?;
        self.inner.write().unwrap().insert(name, value);
        self.bump_generation();
        Ok(())
    }

    fn get(&self, name: &str) -> Result<YarnValue, VariableStorageError> {
        Self::validate_name(name)?;
        self.inner
            .read()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| VariableStorageError::VariableNotFound {
                name: name.to_owned(),
            })
    }

    fn extend(&mut self, values: HashMap<String, YarnValue>) -> Result<(), VariableStorageError> {
        for name in values.keys() {
            Self::validate_name(name)?;
        }
        // Only insert keys that don't exist — preserves values set by prior
        // dialog sessions when a new runner re-declares defaults.
        let mut guard = self.inner.write().unwrap();
        let mut inserted = false;
        for (name, value) in values {
            guard.entry(name).or_insert_with(|| {
                inserted = true;
                value
            });
        }
        drop(guard);
        if inserted {
            self.bump_generation();
        }
        Ok(())
    }

    fn variables(&self) -> HashMap<String, YarnValue> {
        self.inner
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn clear(&mut self) {
        self.inner.write().unwrap().clear();
        self.bump_generation();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_bumps_generation_and_clones_share_it() {
        let mut store = PersistentVariableStorage::new();
        let clone = store.clone();
        assert_eq!(store.generation(), 0);
        store
            .set("$flag".to_owned(), YarnValue::Boolean(true))
            .unwrap();
        assert_eq!(store.generation(), 1);
        assert_eq!(clone.generation(), 1);
    }

    #[test]
    fn extend_bumps_only_when_a_key_is_inserted() {
        let mut store = PersistentVariableStorage::new();
        let mut values = HashMap::default();
        values.insert("$x".to_owned(), YarnValue::Number(1.0));
        store.extend(values.clone()).unwrap();
        let after_first = store.generation();
        assert!(after_first > 0);
        // Re-declaring the same defaults inserts nothing → no bump.
        store.extend(values).unwrap();
        assert_eq!(store.generation(), after_first);
    }

    #[test]
    fn restore_and_clear_bump_generation() {
        let mut store = PersistentVariableStorage::new();
        store.restore(StdHashMap::from([(
            "$x".to_owned(),
            YarnValueDump::Number(2.0),
        )]));
        assert_eq!(store.generation(), 1);
        store.clear();
        assert_eq!(store.generation(), 2);
    }
}
