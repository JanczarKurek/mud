//! Map-fragment templates persisted to disk.
//!
//! A template is a `MapFragment` (objects + floors with relative coordinates)
//! serialized as YAML at `assets/templates/{name}.yaml`. Saved fragments
//! survive across editor sessions and act as reusable building blocks
//! ("hut", "treeline", "village square").
//!
//! Persistence layer is intentionally tiny: read/write a single YAML blob
//! per template, list directory entries lazily. The clipboard machinery
//! does the actual copy/paste; templates just plug a fragment into the
//! clipboard and enter paste mode.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::editor::resources::MapFragment;

const TEMPLATES_DIR: &str = "assets/templates";

/// Lazily-populated list of template names available on disk. Cleared
/// (`loaded = false`) on save / refresh so the next panel render reloads.
#[derive(Resource, Default)]
pub struct EditorTemplatesIndex {
    pub names: Vec<String>,
    pub loaded: bool,
}

fn templates_dir() -> &'static Path {
    Path::new(TEMPLATES_DIR)
}

fn template_path(name: &str) -> PathBuf {
    let mut p = templates_dir().to_path_buf();
    p.push(format!("{name}.yaml"));
    p
}

/// True if a name is composed only of safe filename chars. Mirrors the
/// validation in `process_modal_confirm` so corrupt paths can't reach disk.
pub fn is_valid_template_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

pub fn save_template(name: &str, fragment: &MapFragment) -> io::Result<()> {
    if !is_valid_template_name(name) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "invalid template name",
        ));
    }
    fs::create_dir_all(templates_dir())?;
    let yaml = serde_yaml::to_string(fragment).map_err(io::Error::other)?;
    fs::write(template_path(name), yaml)?;
    Ok(())
}

pub fn load_template(name: &str) -> io::Result<MapFragment> {
    if !is_valid_template_name(name) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "invalid template name",
        ));
    }
    let raw = fs::read_to_string(template_path(name))?;
    serde_yaml::from_str(&raw).map_err(io::Error::other)
}

pub fn list_templates() -> io::Result<Vec<String>> {
    let dir = templates_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            names.push(stem.to_owned());
        }
    }
    names.sort();
    Ok(names)
}
