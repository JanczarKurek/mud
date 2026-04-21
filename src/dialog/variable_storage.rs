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
}

impl PersistentVariableStorage {
    pub fn new() -> Self {
        Self::default()
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
    }
}

impl VariableStorage for PersistentVariableStorage {
    fn clone_shallow(&self) -> Box<dyn VariableStorage> {
        Box::new(self.clone())
    }

    fn set(&mut self, name: String, value: YarnValue) -> Result<(), VariableStorageError> {
        Self::validate_name(&name)?;
        self.inner.write().unwrap().insert(name, value);
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
        for (name, value) in values {
            guard.entry(name).or_insert(value);
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
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
