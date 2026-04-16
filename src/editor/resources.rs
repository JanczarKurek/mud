use std::collections::HashMap;

use bevy::prelude::*;

use crate::world::components::{SpaceId, TilePosition};
use crate::world::map_layout::PortalDefinition;

/// Metadata about the space being edited.
#[derive(Resource)]
pub struct EditorContext {
    pub space_id: SpaceId,
    pub authored_id: String,
    pub map_width: i32,
    pub map_height: i32,
    pub fill_object_type: String,
}

/// Which broad tool is active in the editor.
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum EditorTool {
    #[default]
    Brush,
    Portal,
}

/// Editor interaction state.
#[derive(Resource, Default)]
pub struct EditorState {
    pub selected_type_id: Option<String>,
    pub selected_object_id: Option<u64>,
    pub dirty: bool,
    pub current_tool: EditorTool,
    /// When true, `attach_editor_visuals` should run (e.g. after switching maps).
    pub needs_visual_reattach: bool,
    /// Filter text for the object palette.
    pub palette_filter: String,
    pub palette_filter_focused: bool,
    /// Set by undo/redo toolbar buttons; consumed by handle_undo_redo.
    pub undo_requested: bool,
    pub redo_requested: bool,
}

/// Virtual camera for free panning in the editor.
#[derive(Resource)]
pub struct EditorCamera {
    pub center: Vec2,
    pub pan_speed_tiles_per_sec: f32,
    pub zoom_level: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            pan_speed_tiles_per_sec: 8.0,
            zoom_level: 1.0,
        }
    }
}

/// Buffered property editing for the selected object.
#[derive(Resource, Default)]
pub struct EditorPropertyEditBuffer {
    pub object_id: Option<u64>,
    pub entries: Vec<(String, String)>,
    pub editing_index: Option<usize>,
    pub editing_field: EditingField,
    pub edit_text: String,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum EditingField {
    #[default]
    Value,
    Key,
}

// ── Modal dialog ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ModalTextField {
    pub label: String,
    pub value: String,
    pub placeholder: String,
    pub numeric_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalKind {
    FileOpen,
    SaveAs,
    NewMap,
    PortalCreate,
}

/// Filled by the confirm handler; consumed by `apply_modal_confirmed`.
#[derive(Clone, Debug)]
pub enum ModalConfirmed {
    FileOpen { authored_id: String },
    SaveAs { authored_id: String },
    NewMap { authored_id: String, width: i32, height: i32, fill_type: String },
    PortalCreate {
        source_tile: TilePosition,
        id: String,
        dest_space_id: String,
        dest_tile_x: i32,
        dest_tile_y: i32,
    },
}

#[derive(Resource, Default)]
pub struct ModalState {
    pub active: Option<ModalKind>,
    pub text_fields: Vec<ModalTextField>,
    pub focused_field: usize,
    /// Items shown in a scrollable list (used by FileOpen).
    pub list_items: Vec<String>,
    pub selected_list_item: Option<usize>,
    pub error_message: Option<String>,
    /// Stored source tile for PortalCreate.
    pub portal_source_tile: Option<TilePosition>,
    /// Set by handle_modal_confirm; read and cleared by apply_modal_confirmed.
    pub confirmed: Option<ModalConfirmed>,
    /// Set by keyboard Enter or confirm button; consumed by apply_modal_confirmed.
    pub confirm_triggered: bool,
}

// ── Portal buffer ─────────────────────────────────────────────────────────────

/// Holds the portals for the currently-edited space (mutable, persisted to YAML on save).
#[derive(Resource, Default)]
pub struct EditorPortalBuffer {
    pub portals: Vec<PortalDefinition>,
}

/// Marker component for portal overlay sprites.
#[derive(Component)]
pub struct EditorPortalMarker {
    pub portal_index: usize,
}

// ── Undo / Redo ───────────────────────────────────────────────────────────────

/// A single reversible editor operation stored in the undo/redo stacks.
#[derive(Clone, Debug)]
pub enum UndoOp {
    /// Despawn the entity with this object_id.
    Despawn { object_id: u64 },
    /// Spawn a new object at the given position with given properties.
    Spawn {
        type_id: String,
        space_id: SpaceId,
        tile: TilePosition,
        properties: HashMap<String, String>,
    },
    /// Remove portal at the given index from EditorPortalBuffer.
    RemovePortal { index: usize },
    /// Add a portal to EditorPortalBuffer.
    AddPortal { portal: PortalDefinition },
}

#[derive(Resource, Default)]
pub struct UndoStack {
    pub undo_ops: Vec<UndoOp>,
    pub redo_ops: Vec<UndoOp>,
}

impl UndoStack {
    pub fn push_undo(&mut self, op: UndoOp) {
        self.undo_ops.push(op);
        self.redo_ops.clear();
    }

    pub fn clear(&mut self) {
        self.undo_ops.clear();
        self.redo_ops.clear();
    }
}
