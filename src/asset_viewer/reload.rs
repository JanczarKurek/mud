//! Reload pipeline: messages, watcher draining, and resource rebuild.

use std::collections::HashSet;

use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::prelude::*;

use crate::asset_viewer::resources::{AssetKind, InspectorBuffer, SelfWriteSuppressor, ViewerState};
use crate::asset_viewer::watcher::{batch_paths, classify_path, AssetWatcher};
use crate::magic::resources::SpellDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReloadKind {
    Objects,
    Spells,
    All,
}

#[derive(Message)]
pub struct AssetReloadRequest {
    pub kind: ReloadKind,
}

#[derive(Message, Default)]
pub struct AssetReloadCompleted {
    pub changed_ids: Vec<(AssetKind, String)>,
}

/// Drains the file-watcher channel, suppresses self-writes, and emits
/// `AssetReloadRequest` for genuine external changes.
pub fn drain_file_watcher_events(
    watcher: Option<Res<AssetWatcher>>,
    mut suppressor: ResMut<SelfWriteSuppressor>,
    mut writer: MessageWriter<AssetReloadRequest>,
) {
    suppressor.purge_expired();

    let Some(watcher) = watcher else {
        return;
    };

    let mut emitted: HashSet<ReloadKind> = HashSet::new();

    for result in watcher.try_iter() {
        let batch = match result {
            Ok(batch) => batch,
            Err(errors) => {
                for e in errors {
                    warn!("Asset watcher error: {}", e);
                }
                continue;
            }
        };

        for path in batch_paths(&batch) {
            if suppressor.consume(&path) {
                continue;
            }
            let Some(kind) = classify_path(&path) else {
                continue;
            };
            if emitted.insert(kind) {
                writer.write(AssetReloadRequest { kind });
            }
        }
    }
}

/// Re-runs the on-disk loaders in response to reload requests, diffs against
/// the prior state, and emits `AssetReloadCompleted` with the changed IDs.
pub fn handle_reload_requests(
    mut reader: MessageReader<AssetReloadRequest>,
    mut object_defs: ResMut<OverworldObjectDefinitions>,
    mut spell_defs: ResMut<SpellDefinitions>,
    mut viewer_state: ResMut<ViewerState>,
    mut completed: MessageWriter<AssetReloadCompleted>,
) {
    let mut want_objects = false;
    let mut want_spells = false;

    for req in reader.read() {
        match req.kind {
            ReloadKind::Objects => want_objects = true,
            ReloadKind::Spells => want_spells = true,
            ReloadKind::All => {
                want_objects = true;
                want_spells = true;
            }
        }
    }

    if !want_objects && !want_spells {
        return;
    }

    let mut changed: Vec<(AssetKind, String)> = Vec::new();

    if want_objects {
        let prev: HashSet<String> = object_defs.ids().map(str::to_owned).collect();
        let next = OverworldObjectDefinitions::load_from_disk();
        let next_ids: HashSet<String> = next.ids().map(str::to_owned).collect();
        for id in prev.union(&next_ids) {
            changed.push((AssetKind::Object, id.clone()));
        }
        *object_defs = next;
        info!("Reloaded {} overworld objects", next_ids.len());
    }

    if want_spells {
        let prev: HashSet<String> = spell_defs.ids().map(str::to_owned).collect();
        let next = SpellDefinitions::load_from_disk();
        let next_ids: HashSet<String> = next.ids().map(str::to_owned).collect();
        for id in prev.union(&next_ids) {
            changed.push((AssetKind::Spell, id.clone()));
        }
        *spell_defs = next;
        info!("Reloaded {} spells", next_ids.len());
    }

    viewer_state.reload_counter = viewer_state.reload_counter.wrapping_add(1);

    completed.write(AssetReloadCompleted {
        changed_ids: changed,
    });
}

/// Reload the inspector buffer if the currently-selected asset just changed
/// on disk. If the buffer is dirty, raise `conflict` instead of clobbering.
pub fn refresh_inspector_on_reload(
    mut reader: MessageReader<AssetReloadCompleted>,
    mut inspector_buffer: ResMut<InspectorBuffer>,
) {
    for completed in reader.read() {
        let Some(current_id) = inspector_buffer.asset_id.clone() else {
            continue;
        };
        let current_kind = inspector_buffer.kind;
        let matches = completed
            .changed_ids
            .iter()
            .any(|(k, id)| *k == current_kind && *id == current_id);
        if !matches {
            continue;
        }
        if inspector_buffer.dirty {
            inspector_buffer.conflict = true;
        } else {
            match current_kind {
                AssetKind::Object => inspector_buffer.load_object(&current_id),
                AssetKind::Spell => inspector_buffer.load_spell(&current_id),
            }
        }
    }
}
