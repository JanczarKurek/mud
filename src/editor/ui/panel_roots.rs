//! Bundles the editor's panel-root queries into a single `SystemParam` so
//! click handlers can answer "is the cursor over any chrome?" with one
//! parameter instead of five. Without this, `handle_editor_left_click` blows
//! past Bevy's per-system parameter cap.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::editor::ui::lighting_panel::EditorLightingRoot;
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
    lighting: Query<
        'w,
        's,
        (&'static ComputedNode, &'static UiGlobalTransform),
        With<EditorLightingRoot>,
    >,
}

impl EditorPanelRoots<'_, '_> {
    pub fn cursor_over(&self, cursor: Vec2) -> bool {
        self.palette
            .iter()
            .chain(self.properties.iter())
            .chain(self.top_bar.iter())
            .chain(self.modal.iter())
            .chain(self.templates.iter())
            .chain(self.spawn_groups.iter())
            .chain(self.lighting.iter())
            .any(|(computed, transform)| computed.contains_point(*transform, cursor))
    }
}
