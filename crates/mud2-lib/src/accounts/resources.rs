use std::path::PathBuf;
#[cfg(feature = "server-sim")]
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

#[cfg(feature = "server-sim")]
use crate::accounts::db::AccountDb;

/// Shared handle to the account database. Cloneable across systems; operations
/// take the `Mutex` briefly on connect/disconnect/autosave — not per frame.
#[cfg(feature = "server-sim")]
#[derive(Resource, Clone)]
pub struct AccountDbHandle {
    inner: Arc<Mutex<AccountDb>>,
}

#[cfg(feature = "server-sim")]
impl AccountDbHandle {
    pub fn new(db: AccountDb) -> Self {
        Self {
            inner: Arc::new(Mutex::new(db)),
        }
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, AccountDb> {
        self.inner
            .lock()
            .expect("account DB mutex poisoned — this is a bug, likely a panic in a prior access")
    }
}

/// Path to the account database file. `None` in this resource means the
/// currently-running binary did not open a DB (only applicable to test/dummy
/// setups); production code always inserts `Some(path)` via `GameAppPlugin`,
/// with the path computed by `crate::app::paths` per runtime role.
#[derive(Resource, Clone, Debug, Default)]
pub struct AccountDbPath(pub Option<PathBuf>);

#[derive(Resource, Clone, Debug)]
pub struct AutosaveConfig {
    pub interval_seconds: f64,
}

impl Default for AutosaveConfig {
    fn default() -> Self {
        Self {
            interval_seconds: 60.0,
        }
    }
}
