//! Per-character client-side UI state persistence.
//!
//! Mirrors `crate::ui::quickbar`'s load/save pattern. The on-disk schema is
//! intentionally a single forward-compatible JSON file under
//! `crate::app::paths::ui_state_path` so future presentation prefs (window
//! positions, minimap zoom, …) can slot in as new optional fields without
//! a new file or version bump.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::plugin::AppRuntime;
use crate::game::resources::ClientGameState;
use crate::player::components::PlayerId;
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState};

/// On-disk format. Every field is `#[serde(default)]` so files written by
/// older or newer builds round-trip cleanly.
#[derive(Default, Serialize, Deserialize)]
struct UiStateFile {
    #[serde(default)]
    docked_panels: Option<DockedPanelsState>,
}

#[derive(Default, Serialize, Deserialize)]
struct DockedPanelsState {
    /// Singleton panel entries in sidebar order. Only the 5 singleton
    /// `DockedPanelKind`s are persisted; `Container` and `PouchInBackpack`
    /// reference live runtime ids that don't survive logout.
    #[serde(default)]
    panels: Vec<PersistedPanel>,
    /// Subset of `panels` currently popped out as floating windows.
    #[serde(default)]
    floating: Vec<PersistedPanelKind>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct PersistedPanel {
    kind: PersistedPanelKind,
    /// User-adjusted docked height in pixels. Clamped to
    /// `[DockedPanelState::MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT]` on load
    /// so a corrupt file can't make a panel unreasonably tiny or huge.
    height: f32,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
enum PersistedPanelKind {
    Minimap,
    Status,
    Equipment,
    Backpack,
    NearbyNpcs,
}

impl PersistedPanelKind {
    fn from_kind(kind: DockedPanelKind) -> Option<Self> {
        match kind {
            DockedPanelKind::Minimap => Some(Self::Minimap),
            DockedPanelKind::Status => Some(Self::Status),
            DockedPanelKind::Equipment => Some(Self::Equipment),
            DockedPanelKind::Backpack => Some(Self::Backpack),
            DockedPanelKind::NearbyNpcs => Some(Self::NearbyNpcs),
            DockedPanelKind::Container { .. } | DockedPanelKind::PouchInBackpack { .. } => None,
        }
    }

    fn panel_id(self) -> usize {
        match self {
            Self::Status => DockedPanelState::STATUS_PANEL_ID,
            Self::Equipment => DockedPanelState::EQUIPMENT_PANEL_ID,
            Self::Backpack => DockedPanelState::BACKPACK_PANEL_ID,
            Self::NearbyNpcs => DockedPanelState::NEARBY_NPCS_PANEL_ID,
            Self::Minimap => DockedPanelState::MINIMAP_PANEL_ID,
        }
    }

    /// Build the full `DockedPanel` for this kind from defaults. Used only
    /// when the persisted file names a kind that isn't currently in
    /// `DockedPanelState::panels` (e.g. `NearbyNpcs`, which isn't in the
    /// default layout — it's opened on demand via the menu bar).
    fn default_panel(self) -> DockedPanel {
        match self {
            Self::Minimap => DockedPanel {
                id: DockedPanelState::MINIMAP_PANEL_ID,
                kind: DockedPanelKind::Minimap,
                title: "Minimap".to_owned(),
                height: DockedPanelState::DEFAULT_MINIMAP_PANEL_HEIGHT,
                closable: true,
                resizable: true,
                movable: true,
            },
            Self::Status => DockedPanel {
                id: DockedPanelState::STATUS_PANEL_ID,
                kind: DockedPanelKind::Status,
                title: "Status".to_owned(),
                height: DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
                closable: true,
                resizable: true,
                movable: true,
            },
            Self::Equipment => DockedPanel {
                id: DockedPanelState::EQUIPMENT_PANEL_ID,
                kind: DockedPanelKind::Equipment,
                title: "Equipment".to_owned(),
                height: DockedPanelState::DEFAULT_EQUIPMENT_PANEL_HEIGHT,
                closable: true,
                resizable: true,
                movable: true,
            },
            Self::Backpack => DockedPanel {
                id: DockedPanelState::BACKPACK_PANEL_ID,
                kind: DockedPanelKind::Backpack,
                title: "Backpack".to_owned(),
                height: DockedPanelState::DEFAULT_BACKPACK_PANEL_HEIGHT,
                closable: true,
                resizable: true,
                movable: true,
            },
            Self::NearbyNpcs => DockedPanel {
                id: DockedPanelState::NEARBY_NPCS_PANEL_ID,
                kind: DockedPanelKind::NearbyNpcs,
                title: "Nearby NPCs".to_owned(),
                height: DockedPanelState::DEFAULT_TARGET_PANEL_HEIGHT,
                closable: true,
                resizable: true,
                movable: true,
            },
        }
    }
}

/// Tracks the `PlayerId` we last loaded for, so the loader fires exactly
/// once per character login. `teardown_hud` resets this on logout.
#[derive(Resource, Default)]
pub struct UiStateLoadedFor {
    pub player_id: Option<PlayerId>,
}

/// Load and apply persisted UI state on the first frame after a new
/// `local_player_id` is observed. Uses `bypass_change_detection` when
/// mutating `DockedPanelState` so the immediately-following
/// `persist_ui_state` doesn't see the load as a user-driven change and
/// rewrite the file we just read.
pub fn load_ui_state_on_login(
    runtime: Res<AppRuntime>,
    client_state: Res<ClientGameState>,
    mut docked: ResMut<DockedPanelState>,
    mut loaded_for: ResMut<UiStateLoadedFor>,
) {
    let Some(player_id) = client_state.local_player_id else {
        if loaded_for.player_id.is_some() {
            loaded_for.player_id = None;
        }
        return;
    };

    if loaded_for.player_id == Some(player_id) {
        return;
    }

    let Some(path) = crate::app::paths::ui_state_path(*runtime, player_id.0) else {
        loaded_for.player_id = Some(player_id);
        return;
    };

    let file = read_ui_state_file(&path);
    if let Some(persisted) = file.docked_panels {
        apply_docked_panels(docked.bypass_change_detection(), persisted);
    }
    loaded_for.player_id = Some(player_id);
}

fn apply_docked_panels(docked: &mut DockedPanelState, persisted: DockedPanelsState) {
    // Split current panels into (singletons by kind, non-singleton tail).
    let mut existing_singletons: Vec<DockedPanel> = Vec::new();
    let mut others: Vec<DockedPanel> = Vec::new();
    for panel in docked.panels.drain(..) {
        if PersistedPanelKind::from_kind(panel.kind).is_some() {
            existing_singletons.push(panel);
        } else {
            others.push(panel);
        }
    }

    // Rebuild singletons in persisted order, preferring the in-memory
    // `DockedPanel` (preserves title/flags) and falling back to defaults
    // when the kind wasn't in the current layout. Persisted height is
    // applied (clamped) on top of either base.
    let mut new_panels: Vec<DockedPanel> = Vec::with_capacity(persisted.panels.len());
    let mut seen: HashSet<PersistedPanelKind> = HashSet::new();
    for entry in persisted.panels {
        if !seen.insert(entry.kind) {
            continue;
        }
        let mut panel = existing_singletons
            .iter()
            .position(|p| PersistedPanelKind::from_kind(p.kind) == Some(entry.kind))
            .map(|idx| existing_singletons.remove(idx))
            .unwrap_or_else(|| entry.kind.default_panel());
        panel.height = entry.height.clamp(
            DockedPanelState::MIN_PANEL_HEIGHT,
            DockedPanelState::MAX_PANEL_HEIGHT,
        );
        new_panels.push(panel);
    }

    // Non-singleton panels (Container, PouchInBackpack) shouldn't exist
    // immediately after login, but if they do, keep them at the tail so
    // we never silently drop in-flight UI state.
    new_panels.extend(others);
    docked.panels = new_panels;

    // Rebuild floating set from persisted kinds, intersected with what
    // actually ended up in the panels list.
    let visible: HashSet<usize> = docked.panels.iter().map(|p| p.id).collect();
    docked.floating = persisted
        .floating
        .into_iter()
        .map(|k| k.panel_id())
        .filter(|id| visible.contains(id))
        .collect();
}

/// Write current UI state to disk when `DockedPanelState` has changed.
/// Skips when there's no logged-in player or when the runtime has no
/// client-side path (headless server).
pub fn persist_ui_state(
    runtime: Res<AppRuntime>,
    client_state: Res<ClientGameState>,
    docked: Res<DockedPanelState>,
    loaded_for: Res<UiStateLoadedFor>,
) {
    if !docked.is_changed() {
        return;
    }
    let Some(player_id) = client_state.local_player_id else {
        return;
    };
    // Only persist after the load step has run for this character — protects
    // against writing the default layout on top of a real persisted one
    // before the loader fires.
    if loaded_for.player_id != Some(player_id) {
        return;
    }
    let Some(path) = crate::app::paths::ui_state_path(*runtime, player_id.0) else {
        return;
    };

    let panels: Vec<PersistedPanel> = docked
        .panels
        .iter()
        .filter_map(|p| {
            PersistedPanelKind::from_kind(p.kind).map(|kind| PersistedPanel {
                kind,
                height: p.height,
            })
        })
        .collect();
    let visible: HashSet<usize> = docked.panels.iter().map(|p| p.id).collect();
    let floating: Vec<PersistedPanelKind> = panels
        .iter()
        .map(|p| p.kind)
        .filter(|kind| {
            let id = kind.panel_id();
            visible.contains(&id) && docked.floating.contains(&id)
        })
        .collect();

    let file = UiStateFile {
        docked_panels: Some(DockedPanelsState { panels, floating }),
    };

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&file) {
        Ok(json) => {
            if let Err(err) = fs::write(&path, json) {
                warn!("failed to write ui state file {}: {err}", path.display());
            }
        }
        Err(err) => warn!("failed to serialize ui state: {err}"),
    }
}

fn read_ui_state_file(path: &PathBuf) -> UiStateFile {
    let Ok(raw) = fs::read_to_string(path) else {
        return UiStateFile::default();
    };
    serde_json::from_str::<UiStateFile>(&raw).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> DockedPanelState {
        DockedPanelState::default()
    }

    fn entry(kind: PersistedPanelKind, height: f32) -> PersistedPanel {
        PersistedPanel { kind, height }
    }

    #[test]
    fn apply_reorders_and_drops_singletons() {
        let mut state = default_state();
        // Default order: Minimap, Status, Equipment, Backpack.
        let persisted = DockedPanelsState {
            panels: vec![
                entry(
                    PersistedPanelKind::Backpack,
                    DockedPanelState::DEFAULT_BACKPACK_PANEL_HEIGHT,
                ),
                entry(
                    PersistedPanelKind::Status,
                    DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
                ),
            ],
            floating: vec![],
        };
        apply_docked_panels(&mut state, persisted);
        let kinds: Vec<_> = state.panels.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![DockedPanelKind::Backpack, DockedPanelKind::Status]
        );
        assert!(state.floating.is_empty());
    }

    #[test]
    fn apply_synthesizes_missing_default_panel() {
        let mut state = default_state();
        // NearbyNpcs isn't in the default layout — loader must build it.
        let persisted = DockedPanelsState {
            panels: vec![
                entry(
                    PersistedPanelKind::NearbyNpcs,
                    DockedPanelState::DEFAULT_TARGET_PANEL_HEIGHT,
                ),
                entry(
                    PersistedPanelKind::Minimap,
                    DockedPanelState::DEFAULT_MINIMAP_PANEL_HEIGHT,
                ),
            ],
            floating: vec![PersistedPanelKind::NearbyNpcs],
        };
        apply_docked_panels(&mut state, persisted);
        let kinds: Vec<_> = state.panels.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![DockedPanelKind::NearbyNpcs, DockedPanelKind::Minimap]
        );
        assert!(state
            .floating
            .contains(&DockedPanelState::NEARBY_NPCS_PANEL_ID));
        assert_eq!(state.floating.len(), 1);
    }

    #[test]
    fn apply_restores_persisted_heights_clamped() {
        let mut state = default_state();
        let persisted = DockedPanelsState {
            panels: vec![
                entry(PersistedPanelKind::Status, 170.0),
                // Below MIN_PANEL_HEIGHT — must clamp up.
                entry(PersistedPanelKind::Backpack, 1.0),
                // Above MAX_PANEL_HEIGHT — must clamp down.
                entry(PersistedPanelKind::Equipment, 9999.0),
            ],
            floating: vec![],
        };
        apply_docked_panels(&mut state, persisted);
        let by_kind = |k: DockedPanelKind| state.panels.iter().find(|p| p.kind == k).unwrap();
        assert_eq!(by_kind(DockedPanelKind::Status).height, 170.0);
        assert_eq!(
            by_kind(DockedPanelKind::Backpack).height,
            DockedPanelState::MIN_PANEL_HEIGHT
        );
        assert_eq!(
            by_kind(DockedPanelKind::Equipment).height,
            DockedPanelState::MAX_PANEL_HEIGHT
        );
    }

    #[test]
    fn apply_preserves_non_singleton_tail() {
        let mut state = default_state();
        state.panels.push(DockedPanel {
            id: DockedPanelState::FIRST_CONTAINER_PANEL_ID,
            kind: DockedPanelKind::Container { object_id: 42 },
            title: "Container".to_owned(),
            height: DockedPanelState::DEFAULT_CONTAINER_PANEL_HEIGHT,
            closable: true,
            resizable: true,
            movable: true,
        });
        let persisted = DockedPanelsState {
            panels: vec![entry(
                PersistedPanelKind::Status,
                DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
            )],
            floating: vec![],
        };
        apply_docked_panels(&mut state, persisted);
        let kinds: Vec<_> = state.panels.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![
                DockedPanelKind::Status,
                DockedPanelKind::Container { object_id: 42 },
            ]
        );
    }

    #[test]
    fn apply_drops_duplicate_persisted_entries() {
        let mut state = default_state();
        let persisted = DockedPanelsState {
            panels: vec![
                entry(
                    PersistedPanelKind::Status,
                    DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
                ),
                entry(
                    PersistedPanelKind::Status,
                    DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
                ),
                entry(
                    PersistedPanelKind::Backpack,
                    DockedPanelState::DEFAULT_BACKPACK_PANEL_HEIGHT,
                ),
            ],
            floating: vec![],
        };
        apply_docked_panels(&mut state, persisted);
        let kinds: Vec<_> = state.panels.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![DockedPanelKind::Status, DockedPanelKind::Backpack]
        );
    }
}
