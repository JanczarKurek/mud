use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::world::components::{SpaceId, TilePosition};
use crate::world::floor_definitions::FloorTypeId;
use crate::world::map_layout::PortalDefinition;

/// Metadata about the space being edited.
#[derive(Resource)]
pub struct EditorContext {
    pub space_id: SpaceId,
    pub authored_id: String,
    pub map_width: i32,
    pub map_height: i32,
    pub fill_floor_type: String,
}

/// Which broad tool is active in the editor.
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum EditorTool {
    #[default]
    Brush,
    Portal,
    FloorBrush,
    /// Marquee-rectangle selection. Drag-LMB selects; selection persists
    /// across tool switches and is consumed by clipboard copy/cut.
    Select,
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
    /// Floor-type id painted by the FloorBrush tool. `None` = clear the tile.
    pub selected_floor_type: Option<String>,
    /// Active marquee selection (Select tool result, persisted across tool
    /// switches). Cleared by Esc or by starting a new drag.
    pub selection: Option<EditorSelection>,
    /// Paste-mode state. While `active`, the cursor ghost previews the
    /// clipboard fragment and left-click stamps it; right-click / Esc cancels.
    pub paste_state: PasteState,
    /// User-toggled visibility of the Templates side panel.
    pub templates_panel_visible: bool,
}

/// Inclusive rectangular region of the editor map. `min`/`max` are the
/// component-wise lower / upper bounds (z is ignored for the rectangle but
/// objects of any z within the bbox are still captured at copy time).
#[derive(Clone, Copy, Debug)]
pub struct EditorSelection {
    pub space_id: SpaceId,
    pub min: TilePosition,
    pub max: TilePosition,
}

impl EditorSelection {
    pub fn width(&self) -> i32 {
        (self.max.x - self.min.x).max(0) + 1
    }
    pub fn height(&self) -> i32 {
        (self.max.y - self.min.y).max(0) + 1
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.min.x && x <= self.max.x && y >= self.min.y && y <= self.max.y
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PasteState {
    pub active: bool,
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
    /// Save current selection as a named template.
    SaveAsTemplate,
}

/// Filled by the confirm handler; consumed by `apply_modal_confirmed`.
#[derive(Clone, Debug)]
pub enum ModalConfirmed {
    FileOpen {
        authored_id: String,
    },
    SaveAs {
        authored_id: String,
    },
    NewMap {
        authored_id: String,
        width: i32,
        height: i32,
        fill_type: String,
    },
    PortalCreate {
        source_tile: TilePosition,
        id: String,
        dest_space_id: String,
        dest_tile_x: i32,
        dest_tile_y: i32,
    },
    SaveAsTemplate {
        name: String,
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
    /// Pre-built fragment stashed when opening `SaveAsTemplate` so the confirm
    /// path doesn't need to re-query the world. Cleared by `apply_modal_confirmed`.
    pub pending_template_fragment: Option<MapFragment>,
}

// ── Clipboard fragment ────────────────────────────────────────────────────────

/// A region of the map captured by copy/cut or loaded from a template.
/// Coordinates are *relative to the selection origin* (top-left of the bbox).
/// Authored `MapBehavior` is intentionally dropped — behaviors are tied to
/// authored object IDs that don't survive runtime allocation. Multi-tile
/// sprites are captured by their tile origin only.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MapFragment {
    pub width: i32,
    pub height: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<FragmentObject>,
    /// Includes `None` floor entries so paste can faithfully clear floors
    /// under the stamp. When the fragment was copied with the
    /// "objects-only" modifier, `floors` is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floors: Vec<FragmentFloor>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FragmentObject {
    pub dx: i32,
    pub dy: i32,
    #[serde(default)]
    pub z: i32,
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FragmentFloor {
    pub dx: i32,
    pub dy: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_id: Option<FloorTypeId>,
}

#[derive(Resource, Default)]
pub struct EditorClipboard {
    pub fragment: Option<MapFragment>,
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

/// Marker for ephemeral cursor-ghost sprites. Despawned and respawned each
/// frame by `update_editor_cursor_ghost` so tool/selection changes never leak
/// stale visuals.
#[derive(Component)]
pub struct EditorCursorMarker;

/// Marker for ephemeral paste-ghost sprites (the translucent template /
/// clipboard preview). Owned exclusively by `render_paste_ghost` — the brush
/// cursor cleanup must not touch these, otherwise a parallel-execution race
/// can despawn the just-spawned ghost.
#[derive(Component)]
pub struct EditorPasteGhostMarker;

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
    /// Set a single floor cell to `value`, returning the previous value as the
    /// inverse op. Used by clipboard cut/paste so floor edits round-trip
    /// cleanly through undo/redo.
    SetFloor {
        space_id: SpaceId,
        z: i32,
        x: i32,
        y: i32,
        value: Option<FloorTypeId>,
    },
    /// A bundle executed atomically. Used by clipboard cut and paste so the
    /// whole stamp undoes/redoes in one Ctrl+Z.
    Composite { ops: Vec<UndoOp> },
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
