use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::accounts::hashing::{hash_password, verify_password};
use crate::persistence::PlayerStateDump;

/// Account id reserved for the embedded single-player local character.
pub const LOCAL_ACCOUNT_ID: i64 = 0;
pub const LOCAL_ACCOUNT_USERNAME: &str = "local";

const SCHEMA_VERSION: i64 = 1;
const MAX_USERNAME_LEN: usize = 32;
const MIN_USERNAME_LEN: usize = 3;
const MIN_PASSWORD_LEN: usize = 6;

#[derive(Debug)]
pub enum AuthError {
    UsernameInvalid(&'static str),
    PasswordInvalid(&'static str),
    UsernameTaken,
    UnknownUser,
    WrongPassword,
    Database(rusqlite::Error),
    Hashing(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::UsernameInvalid(msg) => write!(f, "username invalid: {msg}"),
            AuthError::PasswordInvalid(msg) => write!(f, "password invalid: {msg}"),
            AuthError::UsernameTaken => write!(f, "username already taken"),
            AuthError::UnknownUser => write!(f, "unknown user"),
            AuthError::WrongPassword => write!(f, "wrong password"),
            AuthError::Database(err) => write!(f, "database error: {err}"),
            AuthError::Hashing(err) => write!(f, "hashing error: {err}"),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<rusqlite::Error> for AuthError {
    fn from(err: rusqlite::Error) -> Self {
        AuthError::Database(err)
    }
}

pub struct AccountDb {
    conn: Connection,
}

impl AccountDb {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let conn = Connection::open(path)?;
        let mut db = Self { conn };
        db.run_migrations()?;
        db.ensure_local_account()?;
        Ok(db)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.run_migrations()?;
        db.ensure_local_account()?;
        Ok(db)
    }

    fn run_migrations(&mut self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS accounts (
                account_id     INTEGER PRIMARY KEY,
                username       TEXT NOT NULL UNIQUE COLLATE NOCASE,
                password_hash  TEXT,
                character_name TEXT,
                state_json     TEXT,
                created_at     INTEGER NOT NULL,
                last_login_at  INTEGER,
                updated_at     INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    fn ensure_local_account(&mut self) -> rusqlite::Result<()> {
        let now = now_seconds();
        self.conn.execute(
            "INSERT OR IGNORE INTO accounts
                (account_id, username, password_hash, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, ?3)",
            params![LOCAL_ACCOUNT_ID, LOCAL_ACCOUNT_USERNAME, now],
        )?;
        Ok(())
    }

    pub fn create_account(&mut self, username: &str, password: &str) -> Result<i64, AuthError> {
        let normalized = validate_username(username)?;
        if normalized.eq_ignore_ascii_case(LOCAL_ACCOUNT_USERNAME) {
            return Err(AuthError::UsernameInvalid("this username is reserved"));
        }
        validate_password(password)?;

        let hash = hash_password(password).map_err(|e| AuthError::Hashing(e.to_string()))?;
        let now = now_seconds();

        let result = self.conn.execute(
            "INSERT INTO accounts (username, password_hash, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)",
            params![normalized, hash, now],
        );
        match result {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(AuthError::UsernameTaken)
            }
            Err(err) => Err(AuthError::Database(err)),
        }
    }

    pub fn verify_login(&mut self, username: &str, password: &str) -> Result<i64, AuthError> {
        // Login is lenient: any lookup that doesn't match a row with a password
        // returns UnknownUser, regardless of username shape. Validation happens
        // on create_account only.
        let normalized = username.trim();

        let row: Option<(i64, Option<String>)> = self
            .conn
            .query_row(
                "SELECT account_id, password_hash FROM accounts WHERE username = ?1 COLLATE NOCASE",
                params![normalized],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let Some((account_id, stored_hash)) = row else {
            return Err(AuthError::UnknownUser);
        };

        // Accounts with NULL password_hash (e.g. the reserved local account)
        // cannot be authenticated through this flow.
        let Some(stored_hash) = stored_hash else {
            return Err(AuthError::UnknownUser);
        };

        if !verify_password(&stored_hash, password) {
            return Err(AuthError::WrongPassword);
        }

        let now = now_seconds();
        self.conn.execute(
            "UPDATE accounts SET last_login_at = ?1 WHERE account_id = ?2",
            params![now, account_id],
        )?;

        Ok(account_id)
    }

    pub fn load_character(
        &self,
        account_id: i64,
    ) -> Result<Option<PlayerStateDump>, rusqlite::Error> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT state_json FROM accounts WHERE account_id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        let Some(json) = json else {
            return Ok(None);
        };

        match serde_json::from_str::<PlayerStateDump>(&json) {
            Ok(dump) => Ok(Some(dump)),
            Err(err) => {
                bevy::log::warn!(
                    "failed to deserialize stored character for account {account_id}: {err}"
                );
                Ok(None)
            }
        }
    }

    pub fn save_character(
        &self,
        account_id: i64,
        dump: &PlayerStateDump,
    ) -> Result<(), rusqlite::Error> {
        let json = serde_json::to_string(dump).map_err(|err| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(err)))
        })?;
        let now = now_seconds();
        self.conn.execute(
            "UPDATE accounts SET state_json = ?1, updated_at = ?2 WHERE account_id = ?3",
            params![json, now, account_id],
        )?;
        Ok(())
    }

    pub fn account_exists(&self, account_id: i64) -> Result<bool, rusqlite::Error> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM accounts WHERE account_id = ?1",
            params![account_id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }
}

fn validate_username(username: &str) -> Result<String, AuthError> {
    let trimmed = username.trim();
    if trimmed.is_empty() {
        return Err(AuthError::UsernameInvalid("must not be empty"));
    }
    if trimmed.len() < MIN_USERNAME_LEN {
        return Err(AuthError::UsernameInvalid("must be at least 3 characters"));
    }
    if trimmed.len() > MAX_USERNAME_LEN {
        return Err(AuthError::UsernameInvalid("must be at most 32 characters"));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AuthError::UsernameInvalid(
            "may only contain letters, digits, underscore, and hyphen",
        ));
    }
    Ok(trimmed.to_owned())
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AuthError::PasswordInvalid("must be at least 6 characters"));
    }
    Ok(())
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_verifies_account() {
        let mut db = AccountDb::open_in_memory().unwrap();
        let id = db.create_account("alice", "hunter2!").unwrap();
        assert!(id > 0);
        let id_again = db.verify_login("alice", "hunter2!").unwrap();
        assert_eq!(id, id_again);
    }

    #[test]
    fn rejects_wrong_password() {
        let mut db = AccountDb::open_in_memory().unwrap();
        db.create_account("bob", "hunter2!").unwrap();
        assert!(matches!(
            db.verify_login("bob", "nothunter2"),
            Err(AuthError::WrongPassword)
        ));
    }

    #[test]
    fn rejects_unknown_user() {
        let mut db = AccountDb::open_in_memory().unwrap();
        assert!(matches!(
            db.verify_login("ghost", "whatever"),
            Err(AuthError::UnknownUser)
        ));
    }

    #[test]
    fn username_is_case_insensitive_unique() {
        let mut db = AccountDb::open_in_memory().unwrap();
        db.create_account("Alice", "hunter2!").unwrap();
        assert!(matches!(
            db.create_account("ALICE", "hunter3!"),
            Err(AuthError::UsernameTaken)
        ));
        // But login with different casing works:
        db.verify_login("alice", "hunter2!").unwrap();
    }

    #[test]
    fn local_account_is_reserved() {
        let mut db = AccountDb::open_in_memory().unwrap();
        assert!(db.account_exists(LOCAL_ACCOUNT_ID).unwrap());
        // Cannot register as "local":
        assert!(matches!(
            db.create_account("local", "whatever1"),
            Err(AuthError::UsernameInvalid(_))
        ));
        // Cannot authenticate against it (no password hash):
        assert!(matches!(
            db.verify_login("local", "whatever"),
            Err(AuthError::UnknownUser)
        ));
    }

    #[test]
    fn character_save_load_round_trip() {
        use crate::combat::components::{AttackProfile, CombatLeash};
        use crate::player::components::{
            BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, PlayerId, VitalStats,
        };
        use crate::world::components::{SpaceId, TilePosition};

        let mut db = AccountDb::open_in_memory().unwrap();
        let id = db.create_account("carol", "hunter2!").unwrap();
        assert!(db.load_character(id).unwrap().is_none());

        let dump = PlayerStateDump {
            player_id: PlayerId(id as u64),
            object_id: 1234,
            space_id: Some(SpaceId(1)),
            tile_position: TilePosition::ground(5, 7),
            inventory: Inventory::default(),
            chat_log: ChatLog::default(),
            base_stats: BaseStats::default(),
            derived_stats: DerivedStats::default(),
            vital_stats: VitalStats::full(10.0, 5.0),
            movement_cooldown: MovementCooldown::default(),
            attack_profile: AttackProfile::melee(),
            combat_leash: CombatLeash {
                max_distance_tiles: 6,
            },
            combat_target_object_id: None,
            yarn_vars: Default::default(),
        };
        db.save_character(id, &dump).unwrap();

        let loaded = db.load_character(id).unwrap().unwrap();
        assert_eq!(loaded.player_id, PlayerId(id as u64));
        assert_eq!(loaded.object_id, 1234);
        assert_eq!(loaded.tile_position, TilePosition::ground(5, 7));
    }

    #[test]
    fn rejects_short_password() {
        let mut db = AccountDb::open_in_memory().unwrap();
        assert!(matches!(
            db.create_account("dave", "short"),
            Err(AuthError::PasswordInvalid(_))
        ));
    }

    #[test]
    fn rejects_bad_username() {
        let mut db = AccountDb::open_in_memory().unwrap();
        assert!(matches!(
            db.create_account("bad name", "hunter2!"),
            Err(AuthError::UsernameInvalid(_))
        ));
        assert!(matches!(
            db.create_account("ab", "hunter2!"),
            Err(AuthError::UsernameInvalid(_))
        ));
    }
}
