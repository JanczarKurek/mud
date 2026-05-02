//! Per-role on-disk layout for Mud 2.0.
//!
//! Each runtime role has its own subtree so offline (embedded) play, a
//! headless server, and an online TcpClient connecting to some server never
//! clobber each other's state.
//!
//! ```text
//! ~/.local/share/mud2/        (dirs::data_dir — DATA, preserve)
//!     embedded/
//!         accounts.db
//!         saves/world-state.json
//!     server/
//!         accounts.db
//!         saves/world-state.json
//!
//! ~/.cache/mud2/              (dirs::cache_dir — CACHE, safe to nuke)
//!     client/
//!         assets/             (TcpClient asset-sync overlay)
//! ```

use std::path::PathBuf;

use crate::app::plugin::AppRuntime;

const DATA_ROOT_DIR: &str = "mud2";
const CACHE_ROOT_DIR: &str = "mud2";
const EMBEDDED_SUBDIR: &str = "embedded";
const SERVER_SUBDIR: &str = "server";
const CLIENT_SUBDIR: &str = "client";
const ACCOUNTS_DB_FILE: &str = "accounts.db";
const WORLD_SNAPSHOT_REL: &str = "saves/world-state.json";
const CLIENT_ASSETS_SUBDIR: &str = "assets";
const ADMIN_SOCKET_FILE: &str = "admin.sock";

/// Resolved data paths for a server-side role (embedded or headless server).
#[derive(Clone, Debug)]
pub struct RolePaths {
    pub accounts_db: PathBuf,
    pub world_snapshot: PathBuf,
}

/// Resolved cache paths for the online client.
#[derive(Clone, Debug)]
pub struct ClientPaths {
    pub asset_cache_dir: PathBuf,
}

fn data_root() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join(DATA_ROOT_DIR))
        .unwrap_or_else(|| PathBuf::from(DATA_ROOT_DIR))
}

fn cache_root() -> PathBuf {
    dirs::cache_dir()
        .map(|d| d.join(CACHE_ROOT_DIR))
        .unwrap_or_else(|| PathBuf::from(CACHE_ROOT_DIR))
}

fn role_paths_at(role_dir: PathBuf) -> RolePaths {
    RolePaths {
        accounts_db: role_dir.join(ACCOUNTS_DB_FILE),
        world_snapshot: role_dir.join(WORLD_SNAPSHOT_REL),
    }
}

/// Paths used by the single-binary embedded (offline) runtime.
pub fn embedded_paths() -> RolePaths {
    role_paths_at(data_root().join(EMBEDDED_SUBDIR))
}

/// Paths used by the standalone headless server.
pub fn server_paths() -> RolePaths {
    role_paths_at(data_root().join(SERVER_SUBDIR))
}

/// Paths used by the online TcpClient (asset-sync overlay cache).
pub fn client_paths() -> ClientPaths {
    ClientPaths {
        asset_cache_dir: cache_root().join(CLIENT_SUBDIR).join(CLIENT_ASSETS_SUBDIR),
    }
}

/// Role-specific data paths, if the role has any. `TcpClient` returns `None`
/// because it does not own authoritative data — its state lives on the server
/// and its only on-disk footprint is the asset cache.
pub fn role_data_paths(runtime: AppRuntime) -> Option<RolePaths> {
    match runtime {
        AppRuntime::EmbeddedClient => Some(embedded_paths()),
        AppRuntime::HeadlessServer => Some(server_paths()),
        AppRuntime::TcpClient => None,
    }
}

/// Root directory of the data tree (parent of embedded/ and server/).
pub fn data_root_dir() -> PathBuf {
    data_root()
}

/// Default location of the admin REPL UNIX socket for `role`. Returns
/// `None` for `TcpClient` because that role doesn't host an admin REPL.
pub fn default_admin_socket_path(runtime: AppRuntime) -> Option<PathBuf> {
    let role_dir = match runtime {
        AppRuntime::EmbeddedClient => data_root().join(EMBEDDED_SUBDIR),
        AppRuntime::HeadlessServer => data_root().join(SERVER_SUBDIR),
        AppRuntime::TcpClient => return None,
    };
    Some(role_dir.join(ADMIN_SOCKET_FILE))
}

/// Root directory of the cache tree (parent of client/).
pub fn cache_root_dir() -> PathBuf {
    cache_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_and_server_paths_are_disjoint() {
        let e = embedded_paths();
        let s = server_paths();
        assert_ne!(e.accounts_db, s.accounts_db);
        assert_ne!(e.world_snapshot, s.world_snapshot);
        assert!(e.accounts_db.to_string_lossy().contains(EMBEDDED_SUBDIR));
        assert!(s.accounts_db.to_string_lossy().contains(SERVER_SUBDIR));
    }

    #[test]
    fn client_cache_lives_under_cache_root() {
        let c = client_paths();
        assert!(c.asset_cache_dir.starts_with(cache_root_dir()));
        assert!(c.asset_cache_dir.ends_with(CLIENT_ASSETS_SUBDIR));
    }

    #[test]
    fn role_data_paths_omits_tcp_client() {
        assert!(role_data_paths(AppRuntime::EmbeddedClient).is_some());
        assert!(role_data_paths(AppRuntime::HeadlessServer).is_some());
        assert!(role_data_paths(AppRuntime::TcpClient).is_none());
    }
}
