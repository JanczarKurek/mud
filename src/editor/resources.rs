use std::collections::{HashMap, VecDeque};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Maximum number of object/floor type ids kept in the recent-used strip.
pub const RECENT_TYPES_CAP: usize = 12;

/// Maximum size of the editor undo stack. Older ops are dropped from the
/// bottom when this is exceeded so unbounded edit sessions don't grow
/// memory without limit.
pub const UNDO_STACK_CAP: usize = 256;

use crate::world::components::{SpaceId, TilePosition};
use crate::world::floor_definitions::FloorTypeId;
use crate::world::map_layout::{
    AmbientKeyframe, MapBehavior, PortalDefinition, SpaceLightingDef, SpawnGroupDef, TileRectangle,
    VendorStashDef,
};

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
    /// Pick-a-rectangle mode. Drags a marquee like Select, but on release
    /// writes the result to `EditorPickRectResult` and the previous tool is
    /// restored. Used by the spawn-group modal and the per-NPC behavior editor
    /// to capture rectangles by dragging on the map.
    PickRect {
        target: PickRectTarget,
    },
    /// Building drawing mode. Drags a marquee like Select, and on release
    /// stamps walls around the perimeter + floor inside the rectangle as a
    /// single composite undo, driven by the currently-selected building
    /// preset (see [`BuildingToolState`] and `crate::editor::building`).
    /// Subsequent clicks on perimeter walls (with `place_door_armed`) swap
    /// the wall for the preset's door.
    BuildingDraw,
}

/// Where the result of a `PickRect` drag should land.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum PickRectTarget {
    /// Spawn-group area bounds (modal).
    SpawnArea,
    /// Spawn-group behavior bounds (modal).
    SpawnBehavior,
    /// Per-instance NPC behavior bounds (properties panel).
    InstanceBehavior,
    /// New spawn group being created from the Mobs panel. The picked rect
    /// becomes both the area bounds and the roam-and-chase behavior bounds;
    /// the template id is stashed on
    /// `EditorSpawnGroupBuffer.pending_new_spawn_group_template`.
    NewSpawnGroup,
}

/// Result of a `PickRect` drag, consumed by whichever UI requested it.
#[derive(Resource, Default)]
pub struct EditorPickRectResult {
    pub pending: Option<PickedRect>,
}

#[derive(Clone, Copy, Debug)]
pub struct PickedRect {
    pub target: PickRectTarget,
    pub rect: TileRectangle,
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
    /// User-toggled visibility of the Spawn Groups side panel.
    pub spawn_groups_panel_visible: bool,
    /// User-toggled visibility of the Mobs side panel.
    pub mobs_panel_visible: bool,
    /// User-toggled visibility of the Lighting side panel.
    pub lighting_panel_visible: bool,
    /// User-toggled visibility of the Vendor Stashes side panel.
    pub vendor_stashes_panel_visible: bool,
    /// Tool to restore when a `PickRect` mode finishes (or is cancelled).
    pub tool_before_pick: Option<EditorTool>,
    /// Building-tool selections (preset, floor override, door-arm). Only
    /// consulted while `current_tool == BuildingDraw`.
    pub building: BuildingToolState,
    /// Most-recently-clicked object/floor type ids, newest first. Bounded by
    /// `RECENT_TYPES_CAP`. Rendered as a quick-access strip at the top of
    /// the palette.
    pub recent_object_types: VecDeque<String>,
    pub recent_floor_types: VecDeque<String>,
    /// Side length of the object/floor brush in tiles (1 = single tile).
    pub brush_radius: u32,
    /// Active editing floor for multi-floor maps. Ground floor is `0`; upper
    /// floors are positive. PgUp/PgDn cycle this through the floors used by
    /// the map.
    pub current_editing_floor: i32,
    /// Object-fill mode: when true, holding Shift while LMB-dragging the
    /// Brush tool fills a rectangle on release. Toggled by `G` for the
    /// flood-fill alternative.
    pub fill_mode: FillMode,
}

/// Drag-fill mode for the object/floor brush. `Single` is the legacy
/// per-tile paint behavior; `Rect` / `Flood` are opt-in via hotkey.
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FillMode {
    #[default]
    Single,
    /// Shift+drag with Brush or FloorBrush rectangles the region.
    Rect,
    /// Click with Brush or FloorBrush flood-fills the contiguous region.
    Flood,
}

/// Per-session state for the building-draw tool: which preset is active,
/// whether the user has overridden its default floor, and whether the next
/// perimeter click should swap a wall for the preset's door.
#[derive(Default, Clone, Debug)]
pub struct BuildingToolState {
    pub selected_preset_id: Option<String>,
    /// `None` = use the preset's `default_floor`. `Some(id)` overrides it.
    pub floor_override: Option<FloorTypeId>,
    /// When true, the next left-click on a perimeter wall converts it into
    /// the active preset's door. Auto-disarms after one successful swap so
    /// each toggle = one door.
    pub place_door_armed: bool,
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

impl EditorState {
    /// Push `type_id` to the front of `recent_object_types`, deduping and
    /// capping at `RECENT_TYPES_CAP`. Newest entries appear first.
    pub fn touch_recent_object(&mut self, type_id: &str) {
        push_recent(&mut self.recent_object_types, type_id);
    }

    /// Mirror of `touch_recent_object` for floor type ids.
    pub fn touch_recent_floor(&mut self, floor_id: &str) {
        push_recent(&mut self.recent_floor_types, floor_id);
    }

    /// Effective brush radius for tile painting (always `>= 1`).
    pub fn effective_brush_radius(&self) -> u32 {
        self.brush_radius.max(1)
    }

    /// Convert `current_editing_floor` (a floor index, where 0 = ground,
    /// 1 = second story, …) into the raw half-block z that
    /// `TilePosition.z` lives in (`floor_index * 2`). Use this whenever
    /// the editor places, queries, or compares **objects** on the active
    /// floor. `FloorMaps` keys + `GameCommand::EditorSetFloorTile` already
    /// use floor-index units, so they keep using `current_editing_floor`
    /// directly.
    pub fn active_object_raw_z(&self) -> i32 {
        self.current_editing_floor * 2
    }

    /// True iff `tile_z` (raw half-block z, e.g. from `TilePosition.z`)
    /// belongs to the active editing floor. Two half-blocks per floor —
    /// raw z 0 and 1 both live on floor 0, raw z 2 and 3 live on floor 1,
    /// and so on. Equivalent to `floor_index(tile_z) == current_editing_floor`.
    pub fn tile_on_active_floor(&self, tile_z: i32) -> bool {
        crate::world::components::floor_index(tile_z) == self.current_editing_floor
    }
}

fn push_recent(queue: &mut VecDeque<String>, id: &str) {
    if let Some(existing) = queue.iter().position(|s| s == id) {
        queue.remove(existing);
    }
    queue.push_front(id.to_owned());
    while queue.len() > RECENT_TYPES_CAP {
        queue.pop_back();
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
    /// Procedurally generate a dungeon (rooms + corridors) into a new space.
    GenerateDungeon,
    PortalCreate,
    /// Save current selection as a named template.
    SaveAsTemplate,
    /// Create or edit a spawn group. `editing_index = None` is create mode;
    /// `Some(i)` edits the spawn group at that index in
    /// `EditorSpawnGroupBuffer`.
    SpawnGroupEdit {
        editing_index: Option<usize>,
    },
    /// Create or edit a single day/night curve keyframe. `editing_index = None`
    /// is create mode; `Some(i)` edits the keyframe at that index in
    /// `EditorLightingBuffer.config.outdoor_curve`.
    LightingKeyframeEdit {
        editing_index: Option<usize>,
    },
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
    GenerateDungeon {
        authored_id: String,
        width: i32,
        height: i32,
        wall_type: String,
        chamber_floor: String,
        corridor_floor: String,
        target_rooms: u32,
        room_padding: i32,
        corridor_wander: f32,
        branch_factor: f32,
        seed: Option<u64>,
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

/// One row in a `ModalPickerField`. `id = None` is a sentinel "no selection"
/// option (e.g. "(No fill)") which the confirm path forwards as an empty
/// string. `swatch` is the colour shown next to the label.
#[derive(Clone, Debug)]
pub struct ModalPickerOption {
    pub id: Option<String>,
    pub label: String,
    pub swatch: Color,
}

/// A click-picker control embedded in a modal (alongside text fields).
/// Rendered as a labeled scrollable list of swatch+label rows.
#[derive(Clone, Debug)]
pub struct ModalPickerField {
    pub label: String,
    pub options: Vec<ModalPickerOption>,
    pub selected: usize,
}

#[derive(Resource, Default)]
pub struct ModalState {
    pub active: Option<ModalKind>,
    pub text_fields: Vec<ModalTextField>,
    pub focused_field: usize,
    /// Click-pickers rendered after the text fields. Each picker's selection
    /// is stored as an index into its `options` vec.
    pub picker_fields: Vec<ModalPickerField>,
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
    /// Working draft for the SpawnGroupEdit modal. Populated when the modal
    /// opens (cloning the existing group when editing) and consumed on
    /// confirm. Field-level UI binds directly to this struct.
    pub spawn_group_draft: Option<SpawnGroupDraft>,
    /// Out-of-band confirm channel for spawn-group saves so the heavy
    /// `apply_modal_confirmed` system doesn't have to add another mutable
    /// resource and exceed Bevy's per-system parameter cap. Populated by
    /// `process_modal_confirm` for the SpawnGroupEdit kind; consumed by a
    /// dedicated `apply_spawn_group_confirmed` system.
    pub confirmed_spawn_group: Option<ConfirmedSpawnGroup>,
    /// Working draft for the LightingKeyframeEdit modal.
    pub lighting_keyframe_draft: Option<LightingKeyframeDraft>,
    /// Out-of-band confirm channel for lighting keyframes (same rationale as
    /// `confirmed_spawn_group`).
    pub confirmed_lighting_keyframe: Option<ConfirmedLightingKeyframe>,
}

#[derive(Clone, Debug)]
pub struct ConfirmedSpawnGroup {
    pub editing_index: Option<usize>,
    pub group: SpawnGroupDef,
}

#[derive(Clone, Debug)]
pub struct ConfirmedLightingKeyframe {
    pub editing_index: Option<usize>,
    pub keyframe: AmbientKeyframe,
}

/// Mutable working state for the lighting-keyframe modal. Numeric fields kept
/// as strings so partial input round-trips through the UI without losing the
/// user's edit position (same pattern as `SpawnGroupDraft`).
#[derive(Clone, Debug)]
pub struct LightingKeyframeDraft {
    pub editing_index: Option<usize>,
    pub time: String,
    pub r: String,
    pub g: String,
    pub b: String,
    pub alpha: String,
    pub focused_field: LightingKeyframeField,
    /// Cached hue (∈ [0, 1)) for the color-picker widgets. R/G/B remain the
    /// source of truth, but pure-gray RGB has no defined hue, so the picker
    /// needs a remembered value to keep the hue marker stable while the user
    /// drags through gray.
    pub last_hue: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightingKeyframeField {
    Time,
    R,
    G,
    B,
    Alpha,
}

impl Default for LightingKeyframeDraft {
    fn default() -> Self {
        Self {
            editing_index: None,
            time: "0.5".into(),
            r: "255".into(),
            g: "255".into(),
            b: "255".into(),
            alpha: "0.0".into(),
            focused_field: LightingKeyframeField::Time,
            last_hue: 0.0,
        }
    }
}

impl LightingKeyframeDraft {
    pub fn from_existing(index: usize, kf: &AmbientKeyframe) -> Self {
        let hue = crate::editor::ui::color_picker::rgb_to_hsv(kf.color)[0];
        Self {
            editing_index: Some(index),
            time: format!("{:.3}", kf.time),
            r: kf.color[0].to_string(),
            g: kf.color[1].to_string(),
            b: kf.color[2].to_string(),
            alpha: format!("{:.3}", kf.alpha),
            focused_field: LightingKeyframeField::Time,
            last_hue: hue,
        }
    }

    pub fn field_mut(&mut self, field: LightingKeyframeField) -> &mut String {
        match field {
            LightingKeyframeField::Time => &mut self.time,
            LightingKeyframeField::R => &mut self.r,
            LightingKeyframeField::G => &mut self.g,
            LightingKeyframeField::B => &mut self.b,
            LightingKeyframeField::Alpha => &mut self.alpha,
        }
    }
}

/// Mutable working state for the spawn-group modal. Numeric fields are kept
/// as strings so partial input (e.g. an empty `max_count` field) round-trips
/// through the UI without losing the user's edit position.
#[derive(Clone, Debug)]
pub struct SpawnGroupDraft {
    /// Index of the group being edited in `EditorSpawnGroupBuffer.groups`.
    /// `None` = create mode. Stashed on the draft (rather than the
    /// `ModalKind`) so the pick-rect flow can close-and-reopen the modal
    /// without losing this state.
    pub editing_index: Option<usize>,
    pub id: String,
    pub template: String,
    pub max_count: String,
    pub respawn_mean_seconds: String,
    pub area_kind: SpawnAreaKind,
    pub area_min_x: String,
    pub area_min_y: String,
    pub area_max_x: String,
    pub area_max_y: String,
    pub area_tiles: Vec<TilePosition>,
    pub behavior_kind: BehaviorKind,
    pub bhv_min_x: String,
    pub bhv_min_y: String,
    pub bhv_max_x: String,
    pub bhv_max_y: String,
    /// Which numeric/text field has keyboard focus.
    pub focused_field: SpawnGroupField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnAreaKind {
    Bounds,
    Tiles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BehaviorKind {
    Roam,
    RoamAndChase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnGroupField {
    Id,
    Template,
    MaxCount,
    RespawnMean,
    AreaMinX,
    AreaMinY,
    AreaMaxX,
    AreaMaxY,
    BhvMinX,
    BhvMinY,
    BhvMaxX,
    BhvMaxY,
}

impl Default for SpawnGroupDraft {
    fn default() -> Self {
        Self {
            editing_index: None,
            id: String::new(),
            template: String::new(),
            max_count: "3".into(),
            respawn_mean_seconds: "30.0".into(),
            area_kind: SpawnAreaKind::Bounds,
            area_min_x: String::new(),
            area_min_y: String::new(),
            area_max_x: String::new(),
            area_max_y: String::new(),
            area_tiles: Vec::new(),
            behavior_kind: BehaviorKind::Roam,
            bhv_min_x: String::new(),
            bhv_min_y: String::new(),
            bhv_max_x: String::new(),
            bhv_max_y: String::new(),
            focused_field: SpawnGroupField::Id,
        }
    }
}

impl SpawnGroupDraft {
    /// Initialise a draft from an existing spawn group (edit mode).
    pub fn from_existing(index: usize, group: &SpawnGroupDef) -> Self {
        let area_kind = if group.area.tiles.is_some() {
            SpawnAreaKind::Tiles
        } else {
            SpawnAreaKind::Bounds
        };
        let bounds = group.area.bounds;
        let (a_min_x, a_min_y, a_max_x, a_max_y) = bounds
            .map(|r| {
                (
                    r.min_x.to_string(),
                    r.min_y.to_string(),
                    r.max_x.to_string(),
                    r.max_y.to_string(),
                )
            })
            .unwrap_or_default();
        let area_tiles = group
            .area
            .tiles
            .as_ref()
            .map(|ts| ts.iter().map(|t| TilePosition::ground(t.x, t.y)).collect())
            .unwrap_or_default();
        let (behavior_kind, bhv_rect) = match &group.behavior {
            MapBehavior::Roam { bounds } => (BehaviorKind::Roam, *bounds),
            MapBehavior::RoamAndChase { bounds } => (BehaviorKind::RoamAndChase, *bounds),
        };
        Self {
            editing_index: Some(index),
            id: group.id.clone(),
            template: group.template.clone(),
            max_count: group.max_count.to_string(),
            respawn_mean_seconds: group.respawn_mean_seconds.to_string(),
            area_kind,
            area_min_x: a_min_x,
            area_min_y: a_min_y,
            area_max_x: a_max_x,
            area_max_y: a_max_y,
            area_tiles,
            behavior_kind,
            bhv_min_x: bhv_rect.min_x.to_string(),
            bhv_min_y: bhv_rect.min_y.to_string(),
            bhv_max_x: bhv_rect.max_x.to_string(),
            bhv_max_y: bhv_rect.max_y.to_string(),
            focused_field: SpawnGroupField::Id,
        }
    }

    /// Look up the mutable string for a given focused field.
    pub fn field_mut(&mut self, field: SpawnGroupField) -> Option<&mut String> {
        Some(match field {
            SpawnGroupField::Id => &mut self.id,
            SpawnGroupField::Template => &mut self.template,
            SpawnGroupField::MaxCount => &mut self.max_count,
            SpawnGroupField::RespawnMean => &mut self.respawn_mean_seconds,
            SpawnGroupField::AreaMinX => &mut self.area_min_x,
            SpawnGroupField::AreaMinY => &mut self.area_min_y,
            SpawnGroupField::AreaMaxX => &mut self.area_max_x,
            SpawnGroupField::AreaMaxY => &mut self.area_max_y,
            SpawnGroupField::BhvMinX => &mut self.bhv_min_x,
            SpawnGroupField::BhvMinY => &mut self.bhv_min_y,
            SpawnGroupField::BhvMaxX => &mut self.bhv_max_x,
            SpawnGroupField::BhvMaxY => &mut self.bhv_max_y,
        })
    }

    pub fn is_field_numeric(field: SpawnGroupField) -> bool {
        !matches!(field, SpawnGroupField::Id | SpawnGroupField::Template)
    }
}

// ── Clipboard fragment ────────────────────────────────────────────────────────

/// A region of the map captured by copy/cut or loaded from a template.
/// Coordinates are *relative to the selection origin* (top-left of the bbox).
/// Per-object `MapBehavior` (NPC roam/chase config) is preserved on each
/// `FragmentObject` and reattached on paste. Multi-tile sprites are captured
/// by their tile origin only.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<MapBehavior>,
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

/// Holds the spawn-group definitions for the currently-edited space (mutable,
/// persisted to YAML on save). Populated when a map opens; drained back to
/// the serializer's `SpaceOutput.spawn_groups`.
#[derive(Resource, Default)]
pub struct EditorSpawnGroupBuffer {
    pub groups: Vec<SpawnGroupDef>,
    /// Index of the row currently selected in the spawn-groups panel; drives
    /// the area-overlay highlight on the map.
    pub selected: Option<usize>,
    /// Template id stashed by the Mobs panel's "+ Group" button while the
    /// user is dragging a rectangle on the map. Consumed when the
    /// `PickRectTarget::NewSpawnGroup` result lands.
    pub pending_new_spawn_group_template: Option<String>,
}

/// Holds the vendor-stash definitions for the currently-edited space (mutable,
/// persisted to YAML on save). Populated when a map opens; drained back to
/// the serializer's `SpaceOutput.vendor_stashes`.
///
/// Inline editing state lives alongside the stash list: `editing_ware` and
/// `edit_text` mirror the pattern in `EditorPropertyEditBuffer` so a single
/// click on a ware field puts that field into edit mode with no modal.
#[derive(Resource, Default)]
pub struct EditorVendorStashBuffer {
    pub stashes: Vec<VendorStashDef>,
    /// Index of the stash whose wares are expanded for editing in the panel.
    /// `None` collapses every stash to its summary row.
    pub selected: Option<usize>,
    /// Active inline-edit cursor: which stash and which field is being typed
    /// into. `None` means no field has focus.
    pub editing: Option<VendorStashEditingField>,
    /// Buffered text for the currently-edited field; committed back to the
    /// stash when focus moves or Enter is pressed.
    pub edit_text: String,
    /// "Pick from palette" arm. While `Some`, the next click on an
    /// `EditorPaletteItem` is captured by `handle_vendor_stash_palette_pick`
    /// (instead of arming the brush) and the picked `type_id` is written into
    /// `stashes[stash_index].wares[ware_index].type_id`.
    pub pending_ware_pick: Option<VendorWarePickTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VendorWarePickTarget {
    pub stash_index: usize,
    pub ware_index: usize,
}

/// Identifies one editable text field inside the Vendor Stashes panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VendorStashEditingField {
    /// The stash's identifier (the value referenced from a shopkeeper's
    /// `vendor_stash` property).
    StashId { stash_index: usize },
    /// A ware's `type_id` column.
    WareTypeId {
        stash_index: usize,
        ware_index: usize,
    },
    /// A ware's `price_copper` column (numeric).
    WarePrice {
        stash_index: usize,
        ware_index: usize,
    },
    /// A ware's `stock` column. Accepts `infinite` or a non-negative integer.
    WareStock {
        stash_index: usize,
        ware_index: usize,
    },
}

/// Bundle the per-map-edit buffers into a single `SystemParam` so callers
/// like `apply_modal_confirmed` can take both with one slot — Bevy caps
/// system parameter count at 16, and threading this many resources through
/// the modal flow easily blows past it.
#[derive(SystemParam)]
pub struct EditorMapBuffers<'w> {
    pub portals: ResMut<'w, EditorPortalBuffer>,
    pub spawn_groups: ResMut<'w, EditorSpawnGroupBuffer>,
    pub lighting: ResMut<'w, EditorLightingBuffer>,
    pub vendor_stashes: ResMut<'w, EditorVendorStashBuffer>,
}

/// Bundle the inputs to `reset_space_contents_from_def` into one
/// `SystemParam` slot. Same arity-limit motivation as `EditorMapBuffers` —
/// `apply_modal_confirmed` already takes ~15 params before this.
#[derive(SystemParam)]
pub struct EditorSpaceResetDeps<'w, 's> {
    pub spawn_group_registry: ResMut<'w, crate::npc::spawn_groups::SpawnGroupRegistry>,
    pub residents: Query<
        'w,
        's,
        (
            bevy::prelude::Entity,
            &'static crate::world::components::SpaceResident,
        ),
        bevy::prelude::Without<crate::player::components::Player>,
    >,
    pub portal_markers:
        Query<'w, 's, bevy::prelude::Entity, bevy::prelude::With<EditorPortalMarker>>,
}

/// Editor-side mutable view state — context, UI state, camera, and the
/// shared `WorldConfig` snapshot. Bundled so `apply_modal_confirmed` stays
/// under Bevy's 16-param system arity cap.
#[derive(SystemParam)]
pub struct EditorViewState<'w> {
    pub context: ResMut<'w, EditorContext>,
    pub state: ResMut<'w, EditorState>,
    pub camera: ResMut<'w, EditorCamera>,
    pub world_config: ResMut<'w, crate::world::WorldConfig>,
}

/// Holds the lighting configuration for the currently-edited space. Mutable
/// while the editor session is open; persisted into YAML on save and mirrored
/// into the live `ClientGameState.current_space.lighting` so the darkness
/// overlay reflects edits in real time.
#[derive(Resource, Default)]
pub struct EditorLightingBuffer {
    pub config: SpaceLightingDef,
    /// Index of the keyframe currently selected in the panel (drives row
    /// highlight + future "Preview" affordance).
    pub selected_keyframe: Option<usize>,
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
    Despawn {
        object_id: u64,
    },
    /// Spawn a new object at the given position with given properties.
    /// `behavior` carries any per-instance `MapBehavior` so undo-of-delete
    /// (and cut/paste round-trips) preserve NPC roam/chase config.
    Spawn {
        type_id: String,
        space_id: SpaceId,
        tile: TilePosition,
        properties: HashMap<String, String>,
        behavior: Option<MapBehavior>,
    },
    /// Remove portal at the given index from EditorPortalBuffer.
    RemovePortal {
        index: usize,
    },
    /// Add a portal to EditorPortalBuffer.
    AddPortal {
        portal: PortalDefinition,
    },
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
    Composite {
        ops: Vec<UndoOp>,
    },
    /// Spawn-group buffer ops. The `before` snapshot in `EditSpawnGroup` is
    /// the *previous* contents of the slot — applying the op swaps in the
    /// snapshot and emits the *current* contents as the inverse.
    AddSpawnGroup {
        index: usize,
        group: SpawnGroupDef,
    },
    RemoveSpawnGroup {
        index: usize,
    },
    EditSpawnGroup {
        index: usize,
        before: SpawnGroupDef,
    },
    /// Per-instance NPC behavior change. `before` is the prior behavior (None
    /// = no behavior was attached). Applying it swaps the registry entry and
    /// returns the inverse.
    SetBehavior {
        object_id: u64,
        before: Option<MapBehavior>,
    },
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
        // Drop the oldest ops once we exceed the cap so an unbounded edit
        // session can't grow memory without limit.
        if self.undo_ops.len() > UNDO_STACK_CAP {
            let drop_count = self.undo_ops.len() - UNDO_STACK_CAP;
            self.undo_ops.drain(0..drop_count);
        }
    }

    pub fn clear(&mut self) {
        self.undo_ops.clear();
        self.redo_ops.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confirms the two unit systems: editing floor 0 (ground) → raw z 0,
    /// floor 1 (second story) → raw z 2 — i.e. a `floor_plank` placed on
    /// floor 1 actually lands at the z the engine's occlusion + visibility
    /// scan expects, instead of at z=1 (a half-block-elevated decal on
    /// floor 0).
    #[test]
    fn active_object_raw_z_doubles_floor_index() {
        let mut state = EditorState::default();
        assert_eq!(state.active_object_raw_z(), 0);
        state.current_editing_floor = 1;
        assert_eq!(state.active_object_raw_z(), 2);
        state.current_editing_floor = 3;
        assert_eq!(state.active_object_raw_z(), 6);
    }

    /// Both half-blocks of a floor count as "on that floor" — a chest
    /// stacked on the floor's base z (raw z 3 on floor 1) still picks up
    /// when the editor queries who's on floor 1.
    #[test]
    fn tile_on_active_floor_groups_half_blocks() {
        let mut state = EditorState::default();
        state.current_editing_floor = 1;
        assert!(state.tile_on_active_floor(2)); // floor 1 base
        assert!(state.tile_on_active_floor(3)); // floor 1 + half-block
        assert!(!state.tile_on_active_floor(0)); // floor 0
        assert!(!state.tile_on_active_floor(4)); // floor 2
    }
}
