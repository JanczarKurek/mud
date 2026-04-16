use bevy::prelude::*;

use crate::world::components::SpaceId;

/// Metadata about the space being edited.
#[derive(Resource)]
pub struct EditorContext {
    pub space_id: SpaceId,
    pub authored_id: String,
    pub map_width: i32,
    pub map_height: i32,
    pub fill_object_type: String,
}

/// Editor interaction state.
#[derive(Resource, Default)]
pub struct EditorState {
    /// Object type selected in the palette (brush mode).
    pub selected_type_id: Option<String>,
    /// Object currently selected for property viewing/editing.
    pub selected_object_id: Option<u64>,
    /// Unsaved changes exist.
    pub dirty: bool,
}

/// Virtual camera for free panning in the editor.
#[derive(Resource)]
pub struct EditorCamera {
    /// Center tile (float for sub-tile smoothness).
    pub center: Vec2,
    pub pan_speed_tiles_per_sec: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            pan_speed_tiles_per_sec: 8.0,
        }
    }
}

/// Buffered property editing for the selected object.
#[derive(Resource, Default)]
pub struct EditorPropertyEditBuffer {
    /// The object whose properties are shown.
    pub object_id: Option<u64>,
    /// Snapshot of properties being edited (key, value).
    pub entries: Vec<(String, String)>,
    /// Index of the entry currently being edited (None = not editing).
    pub editing_index: Option<usize>,
    /// Which part of the entry is being edited.
    pub editing_field: EditingField,
    /// Current text in the edit buffer.
    pub edit_text: String,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum EditingField {
    #[default]
    Value,
    Key,
}
