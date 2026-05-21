use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::accounts::hashing::{hash_password, verify_password};
use crate::persistence::PlayerStateDump;
use crate::player::classes::Class;
use crate::player::components::{validate_point_buy, AttributeSet};

/// Account id reserved for the embedded single-player local account. Note
/// `PlayerId` now derives from `character_id`, not `account_id`, so this only
/// identifies the *owner* of local characters — they get normal character ids.
pub const LOCAL_ACCOUNT_ID: i64 = 0;
pub const LOCAL_ACCOUNT_USERNAME: &str = "local";

const SCHEMA_VERSION: i64 = 2;
const MAX_USERNAME_LEN: usize = 32;
const MIN_USERNAME_LEN: usize = 3;
const MIN_PASSWORD_LEN: usize = 6;
const MAX_CHARACTER_NAME_LEN: usize = 24;
const MIN_CHARACTER_NAME_LEN: usize = 3;

#[derive(Debug)]
pub enum AuthError {
    UsernameInvalid(&'static str),
    PasswordInvalid(&'static str),
    UsernameTaken,
    UnknownUser,
    WrongPassword,
    CharacterNameInvalid(&'static str),
    CharacterNameTaken,
    CharacterNotFound,
    PointBuyInvalid(String),
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
            AuthError::CharacterNameInvalid(msg) => write!(f, "character name invalid: {msg}"),
            AuthError::CharacterNameTaken => write!(f, "character name already taken"),
            AuthError::CharacterNotFound => write!(f, "character not found"),
            AuthError::PointBuyInvalid(msg) => write!(f, "attributes invalid: {msg}"),
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

#[derive(Clone, Debug)]
pub struct CharacterSummary {
    pub character_id: i64,
    pub name: String,
    pub class: Class,
    pub level: u32,
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
        // Alpha: no migration of pre-v2 data. If we find an older schema,
        // drop the old single-character columns by dropping the accounts row
        // payloads entirely and recreating the characters table from scratch.
        let prior_version: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);
        let prior_version = prior_version
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

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
            );
            CREATE TABLE IF NOT EXISTS characters (
                character_id   INTEGER PRIMARY KEY,
                account_id     INTEGER NOT NULL,
                character_name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                class          TEXT NOT NULL,
                state_json     TEXT,
                created_at     INTEGER NOT NULL,
                last_played_at INTEGER,
                updated_at     INTEGER NOT NULL,
                FOREIGN KEY(account_id) REFERENCES accounts(account_id)
            );
            CREATE INDEX IF NOT EXISTS idx_characters_account ON characters(account_id);",
        )?;

        if prior_version > 0 && prior_version < SCHEMA_VERSION {
            // Wipe pre-v2 single-character state from the accounts table.
            // The characters table is already empty (just-created) so there's
            // nothing to migrate into it. Alpha — no preservation guarantee.
            self.conn.execute(
                "UPDATE accounts SET character_name = NULL, state_json = NULL",
                [],
            )?;
        }

        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
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

    pub fn account_username(&self, account_id: i64) -> Result<Option<String>, rusqlite::Error> {
        self.conn
            .query_row(
                "SELECT username FROM accounts WHERE account_id = ?1",
                params![account_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
    }

    /// List all characters owned by an account, newest first by `last_played_at`
    /// then `created_at`.
    pub fn list_characters(
        &self,
        account_id: i64,
    ) -> Result<Vec<CharacterSummary>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT character_id, character_name, class, state_json
             FROM characters
             WHERE account_id = ?1
             ORDER BY COALESCE(last_played_at, created_at) DESC, character_id ASC",
        )?;
        let rows = stmt.query_map(params![account_id], |row| {
            let character_id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let class_str: String = row.get(2)?;
            let state_json: Option<String> = row.get(3)?;
            Ok((character_id, name, class_str, state_json))
        })?;

        let mut summaries = Vec::new();
        for entry in rows {
            let (character_id, name, class_str, state_json) = entry?;
            let class = parse_class(&class_str).unwrap_or_default();
            let level = state_json
                .as_deref()
                .and_then(|json| serde_json::from_str::<PlayerStateDump>(json).ok())
                .map(|dump| dump.experience.level)
                .unwrap_or(1);
            summaries.push(CharacterSummary {
                character_id,
                name,
                class,
                level,
            });
        }
        Ok(summaries)
    }

    /// Create a new character row for the given account. Seeds an initial
    /// `state_json` so the first `SelectCharacter` has a saved snapshot to
    /// restore. Returns the new `character_id`.
    pub fn create_character(
        &mut self,
        account_id: i64,
        name: &str,
        class: Class,
        attributes: AttributeSet,
        appearance: crate::player::components::PlayerAppearance,
    ) -> Result<i64, AuthError> {
        let normalized = validate_character_name(name)?;
        validate_point_buy(&attributes).map_err(AuthError::PointBuyInvalid)?;

        let now = now_seconds();
        let class_str = class_to_str(class);

        let inserted = self.conn.execute(
            "INSERT INTO characters (account_id, character_name, class, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![account_id, normalized, class_str, now],
        );
        let character_id = match inserted {
            Ok(_) => self.conn.last_insert_rowid(),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(AuthError::CharacterNameTaken);
            }
            Err(err) => return Err(AuthError::Database(err)),
        };

        // Seed an initial state_json so the next SelectCharacter has a dump
        // to restore (with the chosen class + attributes + appearance).
        let dump = build_initial_dump(character_id, class, attributes, appearance);
        if let Ok(json) = serde_json::to_string(&dump) {
            self.conn.execute(
                "UPDATE characters SET state_json = ?1, updated_at = ?2 WHERE character_id = ?3",
                params![json, now, character_id],
            )?;
        }

        Ok(character_id)
    }

    pub fn delete_character(
        &mut self,
        account_id: i64,
        character_id: i64,
    ) -> Result<(), AuthError> {
        let affected = self.conn.execute(
            "DELETE FROM characters WHERE character_id = ?1 AND account_id = ?2",
            params![character_id, account_id],
        )?;
        if affected == 0 {
            return Err(AuthError::CharacterNotFound);
        }
        Ok(())
    }

    /// Returns true iff `character_id` exists and is owned by `account_id`.
    pub fn character_belongs_to_account(
        &self,
        account_id: i64,
        character_id: i64,
    ) -> Result<bool, rusqlite::Error> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM characters WHERE character_id = ?1 AND account_id = ?2",
            params![character_id, account_id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    /// Loads the persisted state for a character. Returns `None` if the
    /// character has no `state_json` set yet (shouldn't happen post-create
    /// since `create_character` seeds one, but kept tolerant).
    pub fn load_character(
        &self,
        character_id: i64,
    ) -> Result<Option<PlayerStateDump>, rusqlite::Error> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT state_json FROM characters WHERE character_id = ?1",
                params![character_id],
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
                bevy::log::warn!("failed to deserialize stored character {character_id}: {err}");
                Ok(None)
            }
        }
    }

    pub fn save_character(
        &self,
        character_id: i64,
        dump: &PlayerStateDump,
    ) -> Result<(), rusqlite::Error> {
        let json = serde_json::to_string(dump).map_err(|err| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(err)))
        })?;
        let now = now_seconds();
        self.conn.execute(
            "UPDATE characters
             SET state_json = ?1,
                 last_played_at = ?2,
                 updated_at = ?2
             WHERE character_id = ?3",
            params![json, now, character_id],
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

    /// Resolve a chat / UI display name for a *character*. Returns the
    /// character's name, or `format!("Player#{character_id}")` if the
    /// character has been deleted (graceful default for stale references).
    pub fn character_display_name(&self, character_id: i64) -> Result<String, rusqlite::Error> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT character_name FROM characters WHERE character_id = ?1",
                params![character_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(row.unwrap_or_else(|| format!("Player#{character_id}")))
    }
}

fn build_initial_dump(
    character_id: i64,
    class: Class,
    attributes: AttributeSet,
    appearance: crate::player::components::PlayerAppearance,
) -> PlayerStateDump {
    use crate::combat::components::{AttackProfile, CombatLeash};
    use crate::magic::effects::MagicEffects;
    use crate::player::components::{
        BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, PlayerId, VitalStats,
    };
    use crate::world::components::TilePosition;

    let base_stats = BaseStats {
        attributes,
        max_health: 0,
        max_mana: 0,
        storage_slots: 8,
    };
    let derived = DerivedStats::from_base_with_class(&base_stats, class, 1);
    let vital = VitalStats::full(derived.max_health as f32, derived.max_mana as f32);

    PlayerStateDump {
        player_id: PlayerId(character_id as u64),
        space_id: None,
        tile_position: TilePosition::ground(0, 0),
        inventory: Inventory::default(),
        chat_log: ChatLog::default(),
        base_stats,
        derived_stats: derived,
        vital_stats: vital,
        movement_cooldown: MovementCooldown::default(),
        attack_profile: AttackProfile::melee(),
        combat_leash: CombatLeash {
            max_distance_tiles: 6,
        },
        yarn_vars: Default::default(),
        facing: Default::default(),
        home_position: None,
        experience: Default::default(),
        class,
        magic_effects: MagicEffects::default(),
        stash: Default::default(),
        skill_sheet: Default::default(),
        appearance,
        discovered_tiles: Default::default(),
    }
}

fn class_to_str(class: Class) -> &'static str {
    match class {
        Class::Fighter => "Fighter",
        Class::Wizard => "Wizard",
        Class::Cleric => "Cleric",
        Class::Vagabond => "Vagabond",
    }
}

fn parse_class(s: &str) -> Option<Class> {
    match s {
        "Fighter" => Some(Class::Fighter),
        "Wizard" => Some(Class::Wizard),
        "Cleric" => Some(Class::Cleric),
        "Vagabond" => Some(Class::Vagabond),
        _ => None,
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

fn validate_character_name(name: &str) -> Result<String, AuthError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AuthError::CharacterNameInvalid("must not be empty"));
    }
    if trimmed.len() < MIN_CHARACTER_NAME_LEN {
        return Err(AuthError::CharacterNameInvalid(
            "must be at least 3 characters",
        ));
    }
    if trimmed.len() > MAX_CHARACTER_NAME_LEN {
        return Err(AuthError::CharacterNameInvalid(
            "must be at most 24 characters",
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ' ')
    {
        return Err(AuthError::CharacterNameInvalid(
            "may only contain letters, digits, spaces, underscore, and hyphen",
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

    fn balanced_attrs() -> AttributeSet {
        // 6 attributes at 10 + 12 budget — split as +2 each to STR/AGI/CON
        // and +2 each to WIL/CHA/FOC gives exactly 12 spent.
        AttributeSet::new(12, 12, 12, 12, 12, 12)
    }

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
        db.verify_login("alice", "hunter2!").unwrap();
    }

    #[test]
    fn local_account_is_reserved() {
        let mut db = AccountDb::open_in_memory().unwrap();
        assert!(db.account_exists(LOCAL_ACCOUNT_ID).unwrap());
        assert!(matches!(
            db.create_account("local", "whatever1"),
            Err(AuthError::UsernameInvalid(_))
        ));
        assert!(matches!(
            db.verify_login("local", "whatever"),
            Err(AuthError::UnknownUser)
        ));
    }

    #[test]
    fn creates_and_lists_characters() {
        let mut db = AccountDb::open_in_memory().unwrap();
        let account = db.create_account("carol", "hunter2!").unwrap();
        let attrs = balanced_attrs();
        let c1 = db
            .create_character(account, "Hero", Class::Fighter, attrs, Default::default())
            .unwrap();
        let c2 = db
            .create_character(account, "Mage", Class::Wizard, attrs, Default::default())
            .unwrap();
        assert!(c1 > 0 && c2 > 0 && c1 != c2);

        let list = db.list_characters(account).unwrap();
        assert_eq!(list.len(), 2);
        // Both names should appear regardless of ordering.
        let names: Vec<_> = list.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Hero"));
        assert!(names.contains(&"Mage"));
    }

    #[test]
    fn rejects_duplicate_character_name() {
        let mut db = AccountDb::open_in_memory().unwrap();
        let a = db.create_account("dave", "hunter2!").unwrap();
        let b = db.create_account("eve", "hunter2!").unwrap();
        let attrs = balanced_attrs();
        db.create_character(a, "Shared", Class::Fighter, attrs, Default::default())
            .unwrap();
        // Same account: rejected.
        assert!(matches!(
            db.create_character(a, "Shared", Class::Wizard, attrs, Default::default()),
            Err(AuthError::CharacterNameTaken)
        ));
        // Different account: also rejected (names are globally unique).
        assert!(matches!(
            db.create_character(b, "shared", Class::Wizard, attrs, Default::default()),
            Err(AuthError::CharacterNameTaken)
        ));
    }

    #[test]
    fn rejects_invalid_point_buy() {
        let mut db = AccountDb::open_in_memory().unwrap();
        let a = db.create_account("frank", "hunter2!").unwrap();
        // All 10s = 0 spent, budget is 12 — must fail.
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 10);
        assert!(matches!(
            db.create_character(a, "Cheater", Class::Fighter, attrs, Default::default()),
            Err(AuthError::PointBuyInvalid(_))
        ));
    }

    #[test]
    fn rejects_invalid_character_name() {
        let mut db = AccountDb::open_in_memory().unwrap();
        let a = db.create_account("greg", "hunter2!").unwrap();
        let attrs = balanced_attrs();
        assert!(matches!(
            db.create_character(a, "", Class::Fighter, attrs, Default::default()),
            Err(AuthError::CharacterNameInvalid(_))
        ));
        assert!(matches!(
            db.create_character(a, "ab", Class::Fighter, attrs, Default::default()),
            Err(AuthError::CharacterNameInvalid(_))
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
        let account = db.create_account("hank", "hunter2!").unwrap();
        let attrs = balanced_attrs();
        let cid = db
            .create_character(
                account,
                "Roundtrip",
                Class::Fighter,
                attrs,
                Default::default(),
            )
            .unwrap();
        // create_character seeds an initial dump, so load_character succeeds.
        let initial = db.load_character(cid).unwrap().unwrap();
        assert_eq!(initial.player_id, PlayerId(cid as u64));

        let dump = PlayerStateDump {
            player_id: PlayerId(cid as u64),
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
            yarn_vars: Default::default(),
            facing: Default::default(),
            home_position: None,
            experience: Default::default(),
            class: Class::Fighter,
            magic_effects: Default::default(),
            stash: Default::default(),
            skill_sheet: Default::default(),
            appearance: Default::default(),
            discovered_tiles: Default::default(),
        };
        db.save_character(cid, &dump).unwrap();

        let loaded = db.load_character(cid).unwrap().unwrap();
        assert_eq!(loaded.player_id, PlayerId(cid as u64));
        assert_eq!(loaded.tile_position, TilePosition::ground(5, 7));
    }

    #[test]
    fn delete_character_removes_row() {
        let mut db = AccountDb::open_in_memory().unwrap();
        let a = db.create_account("ivy", "hunter2!").unwrap();
        let attrs = balanced_attrs();
        let cid = db
            .create_character(a, "Doomed", Class::Wizard, attrs, Default::default())
            .unwrap();
        assert_eq!(db.list_characters(a).unwrap().len(), 1);
        db.delete_character(a, cid).unwrap();
        assert!(db.list_characters(a).unwrap().is_empty());
        assert!(matches!(
            db.delete_character(a, cid),
            Err(AuthError::CharacterNotFound)
        ));
    }

    #[test]
    fn point_buy_validation() {
        assert!(validate_point_buy(&AttributeSet::new(12, 12, 12, 12, 12, 12)).is_ok());
        // All 10s = 0 spent.
        assert!(validate_point_buy(&AttributeSet::new(10, 10, 10, 10, 10, 10)).is_err());
        // Below floor.
        assert!(validate_point_buy(&AttributeSet::new(7, 13, 13, 13, 13, 13)).is_err());
        // Above ceiling.
        assert!(validate_point_buy(&AttributeSet::new(19, 11, 10, 10, 10, 10)).is_err());
        // Refund-into-pool: drop one to 8 (refunds 2), lift another to 18
        // (costs 8), spread the remaining 6 among the rest = 12 total spent.
        assert!(validate_point_buy(&AttributeSet::new(8, 18, 12, 12, 12, 10)).is_ok());
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
