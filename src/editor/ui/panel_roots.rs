//! Bundles the editor's panel-root queries into a single `SystemParam` so
//! click handlers can answer "is the cursor over any chrome?" with one
//! parameter instead of five. Without this, `handle_editor_left_click` blows
//! past Bevy's per-system parameter cap.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::editor::ui::lighting_panel::EditorLightingRoot;
use crate::editor::ui::mobs_panel::EditorMobsRoot;
use crate::editor::ui::modal::ModalOverlayRoot;
use crate::editor::ui::palette::EditorPaletteRoot;
use crate::editor::ui::properties::EditorPropertiesRoot;
use crate::editor::ui::spawn_groups_panel::EditorSpawnGroupsRoot;
use crate::editor::ui::templates_panel::EditorTemplatesRoot;
use crate::editor::ui::EditorTopBarRoot;

#[derive(SystemParam)]
pub struct EditorPanelRoots<'w, 's> {
    palette:
        Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<EditorPaletteRoot>>,
    properties: Query<
        'w,
        's,
        (&'static ComputedNode, &'static UiGlobalTransform),
        With<EditorPropertiesRoot>,
    >,
    top_bar:
        Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<EditorTopBarRoot>>,
    modal:
        Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<ModalOverlayRoot>>,
    templates: Query<
        'w,
        's,
        (&'static ComputedNode, &'static UiGlobalTransform),
        With<EditorTemplatesRoot>,
    >,
    spawn_groups: Query<
        'w,
        's,
        (&'static ComputedNode, &'static UiGlobalTransform),
        With<EditorSpawnGroupsRoot>,
    >,
    mobs: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<EditorMobsRoot>>,
    lighting: Query<
        'w,
        's,
        (&'static ComputedNode, &'static UiGlobalTransform),
        With<EditorLightingRoot>,
    >,
}

impl EditorPanelRoots<'_, '_> {
    /// Test whether `cursor` (logical pixels, as returned by
    /// `Window::cursor_position`) lies over any editor chrome panel.
    /// `ComputedNode::size` and `UiGlobalTransform` are in **physical** pixels,
    /// so the logical cursor must be scaled before `contains_point` is called —
    /// on HiDPI displays (e.g. macOS retina, scale_factor 2.0) the mismatch
    /// would otherwise silently miss every panel and clicks would fall through
    /// to the map.
    pub fn cursor_over(&self, cursor: Vec2, scale_factor: f32) -> bool {
        let physical = cursor * scale_factor;
        self.palette
            .iter()
            .chain(self.properties.iter())
            .chain(self.top_bar.iter())
            .chain(self.modal.iter())
            .chain(self.templates.iter())
            .chain(self.spawn_groups.iter())
            .chain(self.mobs.iter())
            .chain(self.lighting.iter())
            .any(|(computed, transform)| computed.contains_point(*transform, physical))
    }
}
