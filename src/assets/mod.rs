use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::prelude::*;

/// Process-wide XDG asset overlay directory, or `None` if disabled.
///
/// Set once at app startup by `GameAppPlugin::build` via `init_xdg_asset_root`.
/// `Some(path)` means the TcpClient runtime will consult the overlay at that
/// path; `None` means standalone modes (EmbeddedClient, HeadlessServer) load
/// bundled assets exclusively, so stale cache files never shadow repo changes
/// during testing.
static XDG_ASSET_ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Install the process-wide XDG overlay root. Must be called once, before any
/// `AssetResolver::new()` call. Subsequent calls are ignored.
pub fn init_xdg_asset_root(root: Option<PathBuf>) {
    let _ = XDG_ASSET_ROOT.set(root);
}

fn xdg_asset_root() -> Option<&'static Path> {
    XDG_ASSET_ROOT.get().and_then(|opt| opt.as_deref())
}

/// Resolves game asset paths, checking the XDG data directory before bundled assets.
///
/// The overlay path is set at startup by `init_xdg_asset_root`. Bundled path is
/// `assets/` (working directory).
#[derive(Resource, Clone)]
pub struct AssetResolver {
    xdg_root: Option<PathBuf>,
}

impl AssetResolver {
    pub fn new() -> Self {
        Self {
            xdg_root: xdg_asset_root().map(PathBuf::from),
        }
    }

    /// Returns the XDG assets directory, if configured.
    pub fn xdg_assets_dir(&self) -> Option<&Path> {
        self.xdg_root.as_deref()
    }

    /// Returns directories to scan for a given asset subdirectory.
    ///
    /// Bundled directory comes first; XDG directory second so its entries take precedence
    /// when callers use a HashMap (later inserts overwrite earlier ones). XDG is only
    /// included when a root is configured (TcpClient mode).
    pub fn scan_dirs(&self, subdir: &str) -> Vec<PathBuf> {
        let mut dirs = vec![PathBuf::from("assets").join(subdir)];
        if let Some(ref xdg) = self.xdg_root {
            let xdg_dir = xdg.join(subdir);
            if xdg_dir.is_dir() {
                dirs.push(xdg_dir);
            }
        }
        dirs
    }
}

impl Default for AssetResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// A YAML asset discovered on disk. `id` is the file stem.
pub struct DiscoveredYamlAsset {
    pub id: String,
    pub path: PathBuf,
    pub contents: String,
}

/// Scans `subdir` across all asset roots (bundled then XDG overrides) for flat
/// `.yaml` files and returns them as discovered assets keyed by file stem.
///
/// Deduplicated by `id`: each file stem appears at most once in the output. When
/// the same id is present in both bundled and XDG locations, the XDG entry wins
/// (last-write-overrides semantics, matching the documented override intent).
///
/// Sorted alphabetically by `id` so callers that allocate ids based on iteration
/// order (notably `SpaceDefinitions::load_from_disk`) produce identical results
/// across processes and across runs — `fs::read_dir` itself is not order-stable,
/// and TcpClient + HeadlessServer used to disagree on authored object ids,
/// which corrupted every wire-level `object_id` → `type_id` lookup on the client.
///
/// `kind` is used only in panic messages.
pub fn discover_yaml_assets(subdir: &str, kind: &str) -> Vec<DiscoveredYamlAsset> {
    let resolver = AssetResolver::new();
    let mut by_id: std::collections::BTreeMap<String, DiscoveredYamlAsset> =
        std::collections::BTreeMap::new();

    for scan_dir in resolver.scan_dirs(subdir) {
        info!("loading {kind} from {}", scan_dir.display());
        let Ok(entries) = fs::read_dir(&scan_dir) else {
            continue;
        };

        for entry in entries {
            let entry = entry.unwrap_or_else(|error| {
                panic!(
                    "Failed to read {kind} directory entry in {}: {error}",
                    scan_dir.display()
                )
            });
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("yaml") {
                continue;
            }

            let id = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_else(|| panic!("{kind} file has invalid name: {}", path.display()))
                .to_owned();

            let contents = fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!("Failed to read {kind} {}: {error}", path.display())
            });

            // XDG entries come after bundled in `scan_dirs`, so this insert
            // (last-write-wins) gives XDG overrides precedence.
            by_id.insert(id.clone(), DiscoveredYamlAsset { id, path, contents });
        }
    }

    by_id.into_values().collect()
}
