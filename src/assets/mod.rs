use std::path::{Path, PathBuf};

use bevy::prelude::*;

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
    /// when callers use a HashMap (later inserts overwrite earlier ones).
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
