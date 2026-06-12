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
    ///
    /// Per-module content bundles (`<root>/modules/<name>/<subdir>`) are appended
    /// last. See [`AssetResolver::scan_dirs_with_prefix`] for the id-namespacing.
    pub fn scan_dirs(&self, subdir: &str) -> Vec<PathBuf> {
        self.scan_dirs_with_prefix(subdir)
            .into_iter()
            .map(|(_, dir)| dir)
            .collect()
    }

    /// Like [`scan_dirs`](Self::scan_dirs), but pairs each directory with the
    /// **id prefix** its assets register under. Core dirs (bundled + XDG) use an
    /// empty prefix; a per-module dir uses `"<module>/"`, so a module asset named
    /// `foo` loads under the qualified id `<module>/foo` and never collides with
    /// core content or another module. `build-module` resolves authored
    /// references to these qualified ids at compile time, so the engine only ever
    /// sees absolute strings.
    pub fn scan_dirs_with_prefix(&self, subdir: &str) -> Vec<(String, PathBuf)> {
        let mut out = vec![(String::new(), PathBuf::from("assets").join(subdir))];
        if let Some(ref xdg) = self.xdg_root {
            let xdg_dir = xdg.join(subdir);
            if xdg_dir.is_dir() {
                out.push((String::new(), xdg_dir));
            }
        }
        for (module, dir) in module_dirs_with_names(subdir) {
            out.push((format!("{module}/"), dir));
        }
        out
    }
}

/// Root holding per-module content bundles. A module mirrors the top-level asset
/// layout inside its own folder — `assets/modules/<name>/{overworld_objects,
/// spells,recipes,dialogs,quests}/…` — so a whole content pack lives in one
/// place and can be dropped in or removed wholesale without touching the global
/// asset dirs.
const MODULES_DIRNAME: &str = "modules";

/// `(module_name, <root>/modules/<module_name>/<subdir>)` for every module that
/// ships a `<subdir>`, across the bundled (`assets/`) and XDG roots, sorted by
/// module name.
///
/// Sorted so load order is deterministic across processes — the same property
/// `discover_yaml_assets` relies on (TcpClient and HeadlessServer must agree on
/// authored ids, or every wire-level `object_id` → `type_id` lookup breaks).
/// Only directories that actually exist are returned, so callers tolerate
/// modules that ship only some content kinds.
pub fn module_dirs_with_names(subdir: &str) -> Vec<(String, PathBuf)> {
    let mut roots = vec![PathBuf::from("assets")];
    if let Some(xdg) = xdg_asset_root() {
        roots.push(xdg.to_path_buf());
    }
    roots
        .iter()
        .flat_map(|root| module_subdirs_in(root, subdir))
        .collect()
}

fn module_subdirs_in(root: &Path, subdir: &str) -> Vec<(String, PathBuf)> {
    let modules_root = root.join(MODULES_DIRNAME);
    let mut names: Vec<String> = match fs::read_dir(&modules_root) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect(),
        Err(_) => return Vec::new(),
    };
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let dir = modules_root.join(&name).join(subdir);
            (name, dir)
        })
        .filter(|(_, dir)| dir.is_dir())
        .collect()
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

    for (id_prefix, scan_dir) in resolver.scan_dirs_with_prefix(subdir) {
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

            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_else(|| panic!("{kind} file has invalid name: {}", path.display()));
            // Module assets load under `<module>/<stem>`; core under bare `<stem>`.
            let id = format!("{id_prefix}{stem}");

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
