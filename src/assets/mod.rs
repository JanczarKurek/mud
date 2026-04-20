use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;

static XDG_OVERRIDES_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable XDG override scanning globally. Should be called once at
/// app startup before any plugin build reads assets. Only the TcpClient runtime
/// sets this to `true` — the synced assets cached at `~/.local/share/mud2/assets/`
/// are meaningful only when connected to a remote server. Standalone modes
/// (EmbeddedClient, HeadlessServer) load bundled assets exclusively so stale
/// cache files don't shadow repo changes during testing.
pub fn set_xdg_overrides_enabled(enabled: bool) {
    XDG_OVERRIDES_ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn xdg_overrides_enabled() -> bool {
    XDG_OVERRIDES_ENABLED.load(Ordering::SeqCst)
}

/// Resolves game asset paths, checking the XDG data directory before bundled assets.
///
/// XDG path: `~/.local/share/mud2/assets/`
/// Bundled path: `assets/` (working directory)
#[derive(Resource, Clone)]
pub struct AssetResolver {
    xdg_root: Option<PathBuf>,
}

impl AssetResolver {
    pub fn new() -> Self {
        let xdg_root = dirs::data_dir().map(|d| d.join("mud2").join("assets"));
        Self { xdg_root }
    }

    /// Returns the XDG assets directory, if available.
    pub fn xdg_assets_dir(&self) -> Option<&Path> {
        self.xdg_root.as_deref()
    }

    /// Returns directories to scan for a given asset subdirectory.
    ///
    /// Bundled directory comes first; XDG directory second so its entries take precedence
    /// when callers use a HashMap (later inserts overwrite earlier ones). XDG is only
    /// included when overrides are globally enabled (TcpClient mode).
    pub fn scan_dirs(&self, subdir: &str) -> Vec<PathBuf> {
        let mut dirs = vec![PathBuf::from("assets").join(subdir)];
        if xdg_overrides_enabled() {
            if let Some(ref xdg) = self.xdg_root {
                let xdg_dir = xdg.join(subdir);
                if xdg_dir.is_dir() {
                    dirs.push(xdg_dir);
                }
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
/// Later entries shadow earlier ones when callers insert into a HashMap, giving
/// XDG overrides precedence. `kind` is used only in panic messages.
pub fn discover_yaml_assets(subdir: &str, kind: &str) -> Vec<DiscoveredYamlAsset> {
    let resolver = AssetResolver::new();
    let mut out = Vec::new();

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

            out.push(DiscoveredYamlAsset { id, path, contents });
        }
    }

    out
}
