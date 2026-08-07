//! Lazy on-disk index of `.yarn` dialog files used by the editor's
//! per-instance dialog dropdown. Refreshed by setting `loaded = false`
//! (e.g. via the panel's refresh button or `OnEnter(MapEditor)`).

use std::fs;

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct EditorDialogIndex {
    pub names: Vec<String>,
    pub loaded: bool,
}

impl EditorDialogIndex {
    /// Re-scan `assets/dialogs/` for `.yarn` files. File stems are exposed
    /// — this matches what `bevy_yarnspinner` uses as node-source ids and is
    /// also what the editor writes into `properties["dialog_id"]`.
    pub fn refresh(&mut self) {
        let mut names = Vec::new();
        let dir = "assets/dialogs";
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|x| x.to_str()) == Some("yarn") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_owned());
                    }
                }
            }
        }
        names.sort();
        self.names = names;
        self.loaded = true;
    }
}
