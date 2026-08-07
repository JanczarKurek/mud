//! Filesystem watcher for the asset viewer. Watches `assets/overworld_objects/`
//! and `assets/spells/` for changes and feeds them into a debouncer so the
//! reload pipeline only sees one event per editor save.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};

use crate::asset_viewer::reload::ReloadKind;

const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(150);

const OBJECTS_ROOT: &str = "assets/overworld_objects";
const SPELLS_ROOT: &str = "assets/spells";

/// Owns the live filesystem watcher and the receiving end of the debounced
/// event channel. Wrapped in `Option` because watcher construction can fail
/// (sandboxed environments, missing directories) and the viewer must still
/// work for save-triggered reloads.
#[derive(Resource)]
pub struct AssetWatcher {
    #[allow(dead_code)] // kept alive for the lifetime of the resource
    debouncer: Debouncer<RecommendedWatcher, FileIdMap>,
    // std::sync::mpsc::Receiver is `Send` but not `Sync`; Bevy `Resource`
    // requires both. The watcher is only drained from the main thread, so a
    // Mutex around the receiver costs nothing in practice.
    events: Mutex<Receiver<DebounceEventResult>>,
}

impl AssetWatcher {
    /// Non-blocking drain of pending debounced batches.
    pub fn try_iter(&self) -> Vec<DebounceEventResult> {
        let Ok(rx) = self.events.lock() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(event) => out.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

pub fn setup_asset_watcher(mut commands: Commands) {
    match build_watcher() {
        Ok(watcher) => {
            commands.insert_resource(watcher);
            info!(
                "Asset watcher active on {} and {}",
                OBJECTS_ROOT, SPELLS_ROOT
            );
        }
        Err(e) => {
            warn!(
                "Asset watcher could not start ({}). Save-triggered reload still works.",
                e
            );
        }
    }
}

fn build_watcher() -> Result<AssetWatcher, String> {
    let (tx, rx) = channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(DEBOUNCE_TIMEOUT, None, tx)
        .map_err(|e| format!("create debouncer: {}", e))?;

    for root in [OBJECTS_ROOT, SPELLS_ROOT] {
        let path = Path::new(root);
        if !path.exists() {
            warn!("Asset watcher: directory {} does not exist; skipping", root);
            continue;
        }
        debouncer
            .watcher()
            .watch(path, RecursiveMode::Recursive)
            .map_err(|e| format!("watch {}: {}", root, e))?;
    }

    Ok(AssetWatcher {
        debouncer,
        events: Mutex::new(rx),
    })
}

/// Classify a filesystem path into the asset kind whose loader should rerun.
/// Returns `None` for paths we don't care about (stray PNGs, editor swap
/// files, files outside the asset roots, etc.).
pub fn classify_path(path: &Path) -> Option<ReloadKind> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    let objects_idx = components.iter().position(|c| *c == "overworld_objects");
    let spells_idx = components.iter().position(|c| *c == "spells");

    if let Some(i) = objects_idx {
        // expect overworld_objects/<dir>/metadata.yaml
        if components.get(i + 2) == Some(&"metadata.yaml") {
            return Some(ReloadKind::Objects);
        }
    }

    if let Some(i) = spells_idx {
        // expect spells/<file>.yaml (flat, top-level only)
        if let Some(last) = components.get(i + 1) {
            if last.ends_with(".yaml") && components.get(i + 2).is_none() {
                return Some(ReloadKind::Spells);
            }
        }
    }

    None
}

/// Extract every relevant path from a debounced event batch.
pub fn batch_paths(batch: &[notify_debouncer_full::DebouncedEvent]) -> Vec<PathBuf> {
    batch
        .iter()
        .flat_map(|ev| ev.event.paths.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_object_metadata() {
        let p = Path::new("assets/overworld_objects/oak_tree/metadata.yaml");
        assert!(matches!(classify_path(p), Some(ReloadKind::Objects)));
    }

    #[test]
    fn classifies_spell_yaml() {
        let p = Path::new("assets/spells/spark_bolt.yaml");
        assert!(matches!(classify_path(p), Some(ReloadKind::Spells)));
    }

    #[test]
    fn ignores_sprite_png() {
        let p = Path::new("assets/overworld_objects/oak_tree/sprite.png");
        assert!(classify_path(p).is_none());
    }

    #[test]
    fn ignores_nested_spell_dir() {
        let p = Path::new("assets/spells/subdir/foo.yaml");
        assert!(classify_path(p).is_none());
    }
}
