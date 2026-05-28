//! Client-side gameplay toggles. Mirrors `display.rs` — a single
//! `Resource` with a `dirty` flag, an enum of options, and a `cycle` that
//! advances each option to its next value. Consumers read the resource
//! directly; there is no central `apply_*` system.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Persistent gameplay toggles. Currently a single switch — the Nearby NPCs
/// panel's auto-open behavior — but shaped to grow.
#[derive(Resource, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct GameplaySettings {
    /// When `true`, the Nearby NPCs panel opens automatically whenever any
    /// NPC is in interest range and closes when none are. When `false`
    /// (default), the panel only opens when the user manually shows it via
    /// the docked panel chrome.
    pub auto_open_nearby_npcs_panel: bool,
    /// Mirrors `DisplaySettings::dirty` / `Keybindings::dirty` — set on any
    /// change, drained by `persist_settings`.
    #[serde(skip)]
    pub dirty: bool,
}

impl Default for GameplaySettings {
    fn default() -> Self {
        Self {
            auto_open_nearby_npcs_panel: false,
            dirty: false,
        }
    }
}

/// One configurable gameplay knob — the unit the UI builds a row per and the
/// click handler advances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameplayOption {
    AutoOpenNearbyNpcsPanel,
}

impl GameplayOption {
    pub const ALL: [GameplayOption; 1] = [Self::AutoOpenNearbyNpcsPanel];

    pub fn label(self) -> &'static str {
        match self {
            Self::AutoOpenNearbyNpcsPanel => "Auto-open Nearby NPCs panel",
        }
    }

    pub fn value_label(self, s: &GameplaySettings) -> String {
        match self {
            Self::AutoOpenNearbyNpcsPanel => {
                if s.auto_open_nearby_npcs_panel {
                    "On"
                } else {
                    "Off"
                }
            }
        }
        .to_owned()
    }

    pub fn cycle(self, s: &mut GameplaySettings) {
        match self {
            Self::AutoOpenNearbyNpcsPanel => {
                s.auto_open_nearby_npcs_panel = !s.auto_open_nearby_npcs_panel;
            }
        }
        s.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_open_toggles() {
        let mut s = GameplaySettings::default();
        assert!(!s.auto_open_nearby_npcs_panel);
        GameplayOption::AutoOpenNearbyNpcsPanel.cycle(&mut s);
        assert!(s.auto_open_nearby_npcs_panel);
        assert!(s.dirty);
        GameplayOption::AutoOpenNearbyNpcsPanel.cycle(&mut s);
        assert!(!s.auto_open_nearby_npcs_panel);
    }
}
