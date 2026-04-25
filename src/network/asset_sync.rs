use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::network::protocol::AssetEntry;

const SYNC_DIRS: &[&str] = &[
    "overworld_objects",
    "object_bases",
    "maps",
    "spells",
    "floors",
];

pub fn build_server_manifest() -> Vec<AssetEntry> {
    let mut entries = Vec::new();

    for subdir in SYNC_DIRS {
        let dir = PathBuf::from("assets").join(subdir);
        collect_entries(&dir, &dir, &mut entries);
    }

    entries
}

fn collect_entries(root: &Path, dir: &Path, entries: &mut Vec<AssetEntry>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_entries(root, &path, entries);
        } else if path.is_file() {
            let Ok(relative) = path.strip_prefix(PathBuf::from("assets")) else {
                continue;
            };
            let relative_str = relative.to_string_lossy().replace('\\', "/");
            let hash = hash_file(&path);
            entries.push(AssetEntry {
                path: relative_str.to_owned(),
                hash,
            });
        }
    }
}

pub fn hash_file(path: &Path) -> String {
    let Ok(data) = std::fs::read(path) else {
        return String::new();
    };
    hash_bytes(&data)
}

pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
