pub mod autosave;
pub mod db;
pub mod hashing;
pub mod resources;

use std::path::PathBuf;

use bevy::prelude::*;

pub use crate::accounts::autosave::{
    autosave_all_players, persist_disconnected_players, save_all_players_on_app_exit, AutosaveTimer,
};
pub use crate::accounts::db::{AccountDb, AuthError, LOCAL_ACCOUNT_ID, LOCAL_ACCOUNT_USERNAME};
pub use crate::accounts::resources::{
    default_db_path, AccountDbHandle, AccountDbPath, AutosaveConfig,
};

#[derive(Default)]
pub struct AccountsServerPlugin {
    pub db_path: Option<PathBuf>,
}

impl Plugin for AccountsServerPlugin {
    fn build(&self, app: &mut App) {
        let path = self.db_path.clone().unwrap_or_else(default_db_path);
        match AccountDb::open(&path) {
            Ok(db) => {
                info!("account database open at {}", path.display());
                app.insert_resource(AccountDbHandle::new(db));
            }
            Err(err) => {
                error!(
                    "failed to open account database at {}: {err}",
                    path.display()
                );
                // Fall back to an in-memory DB so the server still runs; a loud
                // warning is logged so the operator knows no persistence happens.
                warn!("using in-memory account DB — NOTHING WILL BE SAVED");
                let db = AccountDb::open_in_memory()
                    .expect("in-memory sqlite should never fail to open");
                app.insert_resource(AccountDbHandle::new(db));
            }
        }
        app.insert_resource(AccountDbPath(self.db_path.clone()))
            .insert_resource(AutosaveConfig::default())
            .insert_resource(AutosaveTimer::default())
            .add_systems(Update, autosave_all_players)
            .add_systems(Last, persist_disconnected_players)
            .add_systems(Last, save_all_players_on_app_exit);
    }
}
