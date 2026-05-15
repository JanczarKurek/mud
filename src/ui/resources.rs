use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::commands::ItemReference;
use crate::ui::components::ItemSlotKind;
use crate::ui::mountable_panel::{ModeStore, PanelMountMode};
use crate::world::components::TilePosition;

#[derive(Clone, Copy)]
pub enum ContextMenuTarget {
    World(u64),
    Slot(ItemSlotKind),
}

#[derive(Resource, Default)]
pub struct ContextMenuState {
    pub target: Option<ContextMenuTarget>,
    pub position: Vec2,
    pub can_open: bool,
    pub can_use: bool,
    pub can_use_on: bool,
    pub can_attack: bool,
    pub can_take_partial: bool,
    pub can_talk: bool,
    /// True when the right-clicked target is a tradeable peer (another player
    /// adjacent to the local player; later: a shopkeeper NPC).
    pub can_trade: bool,
    /// `(verb, label)` for the *single* interaction (door open/close,
    /// torch light/extinguish, lever pull) currently applicable to the
    /// hovered object. `None` means no interact button is shown.
    pub interaction: Option<(String, String)>,
}

impl ContextMenuState {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        position: Vec2,
        target: ContextMenuTarget,
        can_open: bool,
        can_use: bool,
        can_use_on: bool,
        can_attack: bool,
        can_take_partial: bool,
        can_talk: bool,
        can_trade: bool,
        interaction: Option<(String, String)>,
    ) {
        self.position = position;
        self.target = Some(target);
        self.can_open = can_open;
        self.can_use = can_use;
        self.can_use_on = can_use_on;
        self.can_attack = can_attack;
        self.can_take_partial = can_take_partial;
        self.can_talk = can_talk;
        self.can_trade = can_trade;
        self.interaction = interaction;
    }

    pub fn hide(&mut self) {
        self.target = None;
        self.can_open = false;
        self.can_use = false;
        self.can_use_on = false;
        self.can_attack = false;
        self.can_take_partial = false;
        self.can_talk = false;
        self.can_trade = false;
        self.interaction = None;
    }

    pub fn is_visible(&self) -> bool {
        self.target.is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogEntryKind {
    Npc,
    Player,
}

#[derive(Clone, Debug)]
pub struct DialogEntry {
    pub speaker: Option<String>,
    pub text: String,
    pub kind: DialogEntryKind,
}

/// Client-side UI state mirroring the currently open dialog panel. Updated by
/// `apply_game_ui_events` in response to server-emitted
/// `DialogLine`/`DialogOptions`/`DialogClose` events.
#[derive(Resource, Default)]
pub struct ActiveDialogState {
    pub session_id: Option<u64>,
    pub speaker: Option<String>,
    pub text: String,
    pub options: Vec<String>,
    /// If `true`, show a "Continue" button (line presented, no options).
    pub awaiting_continue: bool,
    /// Bumped each time the current line / options change — used by
    /// `sync_dialog_panel_options` to rebuild option buttons without
    /// re-diffing vectors.
    pub revision: u64,
    /// Append-only conversation log for the current session. Cleared
    /// whenever the session id changes or `close()` is called.
    pub transcript: Vec<DialogEntry>,
    /// Bumped each time `transcript` changes, so the renderer can detect
    /// growth without comparing the vector.
    pub transcript_revision: u64,
    /// Last position/size of the dialog window — cached so the lifecycle
    /// system can re-open it where the user left it.
    pub last_position: Option<Vec2>,
    pub last_size: Option<Vec2>,
}

impl ActiveDialogState {
    pub fn is_active(&self) -> bool {
        self.session_id.is_some()
    }

    pub fn show_line(&mut self, session_id: u64, speaker: Option<String>, text: String) {
        if self.session_id != Some(session_id) {
            self.transcript.clear();
        }
        self.session_id = Some(session_id);
        self.speaker = speaker.clone();
        self.text = text.clone();
        self.options.clear();
        self.awaiting_continue = true;
        self.revision = self.revision.wrapping_add(1);
        self.transcript.push(DialogEntry {
            speaker,
            text,
            kind: DialogEntryKind::Npc,
        });
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
    }

    pub fn show_options(&mut self, session_id: u64, options: Vec<String>) {
        if self.session_id != Some(session_id) {
            self.transcript.clear();
            self.transcript_revision = self.transcript_revision.wrapping_add(1);
        }
        self.session_id = Some(session_id);
        self.options = options;
        self.awaiting_continue = false;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn close(&mut self) {
        self.session_id = None;
        self.speaker = None;
        self.text.clear();
        self.options.clear();
        self.awaiting_continue = false;
        self.revision = self.revision.wrapping_add(1);
        if !self.transcript.is_empty() {
            self.transcript.clear();
            self.transcript_revision = self.transcript_revision.wrapping_add(1);
        }
    }

    /// Append the player's chosen option to the transcript. Called by the
    /// click handler so the choice appears in the log immediately, without
    /// waiting for a server round-trip.
    pub fn push_player_choice(&mut self, text: String) {
        self.transcript.push(DialogEntry {
            speaker: None,
            text,
            kind: DialogEntryKind::Player,
        });
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
    }
}

#[derive(Resource, Default)]
pub struct TakePartialState {
    pub source: Option<ItemReference>,
    pub max_amount: u32,
    pub selected_amount: u32,
}

pub enum DragSource {
    World,
    UiSlot(ItemSlotKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DockedPanelKind {
    Minimap,
    Status,
    Equipment,
    Backpack,
    CurrentTarget,
    Container {
        object_id: u64,
    },
    /// A pouch sitting in the local player's backpack at `backpack_slot`.
    /// Slot contents come from
    /// `client_state.inventory.backpack_slots[backpack_slot].contained_slots`.
    /// Closes automatically when the underlying slot empties or stops being
    /// a container.
    PouchInBackpack {
        backpack_slot: usize,
    },
}

#[derive(Clone, Debug)]
pub struct DockedPanel {
    pub id: usize,
    pub kind: DockedPanelKind,
    pub title: String,
    pub height: f32,
    pub closable: bool,
    pub resizable: bool,
    pub movable: bool,
}

#[derive(Resource)]
pub struct DockedPanelState {
    pub panels: Vec<DockedPanel>,
    /// `panel_id`s currently rendered as a floating window rather than
    /// in the sidebar. Maintained by `sync_panel_floating_lifecycle`
    /// across all `MountablePanel` impls. The layout system consults
    /// this for both visibility (hide the docked entity) and
    /// y-stacking (skip floating rows in the offset sum).
    pub floating: std::collections::HashSet<usize>,
}

/// Single-instance mount-state newtype around [`PanelMountMode`]. One
/// per singleton panel — Bevy resources are looked up by concrete type,
/// so each singleton needs its own distinct resource. The [`ModeStore`]
/// impl is the same for all of them; the macro below stamps it out.
macro_rules! singleton_mode_resource {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Resource, Clone, Copy, Debug, Default)]
            pub struct $name(pub PanelMountMode);

            impl ModeStore for $name {
                type Key = ();
                fn mode(&self, _: ()) -> PanelMountMode { self.0 }
                fn set_mode(&mut self, _: (), mode: PanelMountMode) { self.0 = mode; }
                fn clear(&mut self, _: ()) { self.0 = PanelMountMode::default(); }
                fn known_keys(&self) -> Vec<()> {
                    // Only report a known key while the entry is
                    // non-default — that way the lifecycle GC step
                    // doesn't have to special-case the singleton
                    // resting state.
                    match self.0 {
                        PanelMountMode::Mounted => vec![],
                        PanelMountMode::Floating { .. } => vec![()],
                    }
                }
            }
        )*
    };
}

singleton_mode_resource!(
    StatusPanelMode,
    EquipmentPanelMode,
    BackpackPanelMode,
    CurrentTargetPanelMode,
    MinimapPanelMode,
);

/// Per-instance mount state for the container/pouch panel pool. Keyed
/// by the fixed sidebar `panel_id` (`FIRST_CONTAINER_PANEL_ID..`),
/// shared across `Container` and `PouchInBackpack` kinds since they
/// reuse the same docked-panel pool and the same body builder.
///
/// Missing entries default to `Mounted` on read. Entries are cleared
/// when the underlying panel disappears from `DockedPanelState`
/// (server closed the container, player walked away, pouch emptied).
#[derive(Resource, Default)]
pub struct ContainerPanelModes {
    pub modes: HashMap<usize, PanelMountMode>,
}

impl ContainerPanelModes {
    pub fn is_floating(&self, panel_id: usize) -> bool {
        matches!(self.mode(panel_id), PanelMountMode::Floating { .. })
    }
}

impl ModeStore for ContainerPanelModes {
    type Key = usize;
    fn mode(&self, panel_id: usize) -> PanelMountMode {
        self.modes.get(&panel_id).copied().unwrap_or_default()
    }
    fn set_mode(&mut self, panel_id: usize, mode: PanelMountMode) {
        self.modes.insert(panel_id, mode);
    }
    fn clear(&mut self, panel_id: usize) {
        self.modes.remove(&panel_id);
    }
    fn known_keys(&self) -> Vec<usize> {
        self.modes.keys().copied().collect()
    }
}

impl DockedPanelState {
    pub const STATUS_PANEL_ID: usize = 0;
    pub const EQUIPMENT_PANEL_ID: usize = 1;
    pub const BACKPACK_PANEL_ID: usize = 2;
    pub const CURRENT_TARGET_PANEL_ID: usize = 3;
    pub const FIRST_CONTAINER_PANEL_ID: usize = 4;
    pub const MINIMAP_PANEL_ID: usize = 10;
    pub const MAX_OPEN_CONTAINERS: usize = 4;
    pub const DEFAULT_STATUS_PANEL_HEIGHT: f32 = 96.0;
    pub const DEFAULT_EQUIPMENT_PANEL_HEIGHT: f32 = 248.0;
    pub const DEFAULT_BACKPACK_PANEL_HEIGHT: f32 = 184.0;
    pub const DEFAULT_TARGET_PANEL_HEIGHT: f32 = 88.0;
    pub const DEFAULT_CONTAINER_PANEL_HEIGHT: f32 = 182.0;
    pub const DEFAULT_MINIMAP_PANEL_HEIGHT: f32 = 220.0;
    pub const MIN_PANEL_HEIGHT: f32 = 84.0;
    pub const MAX_PANEL_HEIGHT: f32 = 480.0;

    pub fn open_current_target(&mut self) {
        let panel = DockedPanel {
            id: Self::CURRENT_TARGET_PANEL_ID,
            kind: DockedPanelKind::CurrentTarget,
            title: "Current Target".to_owned(),
            height: Self::DEFAULT_TARGET_PANEL_HEIGHT,
            closable: true,
            resizable: true,
            movable: true,
        };
        self.upsert_panel(panel);
    }

    pub fn close_current_target(&mut self) {
        self.close_panel(Self::CURRENT_TARGET_PANEL_ID);
    }

    pub fn open(&mut self, object_id: u64) {
        let panel = DockedPanel {
            id: self.next_container_panel_id(),
            kind: DockedPanelKind::Container { object_id },
            title: "Container".to_owned(),
            height: Self::DEFAULT_CONTAINER_PANEL_HEIGHT,
            closable: true,
            resizable: true,
            movable: true,
        };

        if let Some(existing_index) = self
            .panels
            .iter()
            .position(|panel| panel.kind == DockedPanelKind::Container { object_id })
        {
            let existing_panel = self.panels.remove(existing_index);
            self.panels.push(existing_panel);
            return;
        }

        self.close_oldest_container_if_needed();
        self.upsert_panel(panel);
    }

    /// Open (or refocus) a panel viewing the contents of a pouch sitting in
    /// the local inventory at `backpack_slot`. Reuses the same physical panel
    /// pool as world-container panels (`MAX_OPEN_CONTAINERS = 4`).
    pub fn open_pouch(&mut self, backpack_slot: usize) {
        let kind = DockedPanelKind::PouchInBackpack { backpack_slot };
        if let Some(existing_index) = self.panels.iter().position(|panel| panel.kind == kind) {
            let existing_panel = self.panels.remove(existing_index);
            self.panels.push(existing_panel);
            return;
        }
        self.close_oldest_container_if_needed();
        let panel = DockedPanel {
            id: self.next_container_panel_id(),
            kind,
            title: "Pouch".to_owned(),
            height: Self::DEFAULT_CONTAINER_PANEL_HEIGHT,
            closable: true,
            resizable: true,
            movable: true,
        };
        self.upsert_panel(panel);
    }

    pub fn close_panel(&mut self, panel_id: usize) {
        if let Some(index) = self.panels.iter().position(|panel| panel.id == panel_id) {
            self.panels.remove(index);
        }
    }

    pub fn panel(&self, panel_id: usize) -> Option<&DockedPanel> {
        self.panels.iter().find(|panel| panel.id == panel_id)
    }

    pub fn panel_mut(&mut self, panel_id: usize) -> Option<&mut DockedPanel> {
        self.panels.iter_mut().find(|panel| panel.id == panel_id)
    }

    pub fn container_object_id_for_panel(&self, panel_id: usize) -> Option<u64> {
        match self.panel(panel_id).map(|panel| panel.kind) {
            Some(DockedPanelKind::Container { object_id }) => Some(object_id),
            _ => None,
        }
    }

    /// If `panel_id` resolves to a `PouchInBackpack` panel, return the
    /// underlying inventory slot index. Pairs with
    /// `container_object_id_for_panel` so callers can branch on whether the
    /// slot grid points at a world container or an inventory pouch.
    pub fn pouch_backpack_slot_for_panel(&self, panel_id: usize) -> Option<usize> {
        match self.panel(panel_id).map(|panel| panel.kind) {
            Some(DockedPanelKind::PouchInBackpack { backpack_slot }) => Some(backpack_slot),
            _ => None,
        }
    }

    pub fn is_open(&self, panel_id: usize) -> bool {
        self.panel(panel_id).is_some()
    }

    pub fn is_floating(&self, panel_id: usize) -> bool {
        self.floating.contains(&panel_id)
    }

    pub fn set_floating(&mut self, panel_id: usize, floating: bool) {
        if floating {
            self.floating.insert(panel_id);
        } else {
            self.floating.remove(&panel_id);
        }
    }

    pub fn move_panel_to_index(&mut self, panel_id: usize, target_index: usize) {
        let Some(current_index) = self.panels.iter().position(|panel| panel.id == panel_id) else {
            return;
        };

        let panel = self.panels.remove(current_index);
        let bounded_index = target_index.min(self.panels.len());
        self.panels.insert(bounded_index, panel);
    }

    fn upsert_panel(&mut self, panel: DockedPanel) {
        if let Some(existing) = self
            .panels
            .iter_mut()
            .find(|existing| existing.id == panel.id)
        {
            *existing = panel;
            return;
        }
        self.panels.push(panel);
    }

    fn next_container_panel_id(&self) -> usize {
        for panel_id in
            (0..Self::MAX_OPEN_CONTAINERS).map(|index| Self::FIRST_CONTAINER_PANEL_ID + index)
        {
            if !self.is_open(panel_id) {
                return panel_id;
            }
        }

        self.oldest_container_panel_id()
            .unwrap_or(Self::FIRST_CONTAINER_PANEL_ID)
    }

    fn oldest_container_panel_id(&self) -> Option<usize> {
        self.panels.iter().find_map(|panel| match panel.kind {
            DockedPanelKind::Container { .. } | DockedPanelKind::PouchInBackpack { .. } => {
                Some(panel.id)
            }
            _ => None,
        })
    }

    fn close_oldest_container_if_needed(&mut self) {
        let open_container_count = self
            .panels
            .iter()
            .filter(|panel| {
                matches!(
                    panel.kind,
                    DockedPanelKind::Container { .. } | DockedPanelKind::PouchInBackpack { .. }
                )
            })
            .count();

        if open_container_count >= Self::MAX_OPEN_CONTAINERS {
            if let Some(panel_id) = self.oldest_container_panel_id() {
                self.close_panel(panel_id);
            }
        }
    }
}

impl Default for DockedPanelState {
    fn default() -> Self {
        Self {
            floating: std::collections::HashSet::new(),
            panels: vec![
                DockedPanel {
                    id: Self::MINIMAP_PANEL_ID,
                    kind: DockedPanelKind::Minimap,
                    title: "Minimap".to_owned(),
                    height: Self::DEFAULT_MINIMAP_PANEL_HEIGHT,
                    closable: true,
                    resizable: true,
                    movable: true,
                },
                DockedPanel {
                    id: Self::STATUS_PANEL_ID,
                    kind: DockedPanelKind::Status,
                    title: "Status".to_owned(),
                    height: Self::DEFAULT_STATUS_PANEL_HEIGHT,
                    closable: true,
                    resizable: true,
                    movable: true,
                },
                DockedPanel {
                    id: Self::EQUIPMENT_PANEL_ID,
                    kind: DockedPanelKind::Equipment,
                    title: "Equipment".to_owned(),
                    height: Self::DEFAULT_EQUIPMENT_PANEL_HEIGHT,
                    closable: true,
                    resizable: true,
                    movable: true,
                },
                DockedPanel {
                    id: Self::BACKPACK_PANEL_ID,
                    kind: DockedPanelKind::Backpack,
                    title: "Backpack".to_owned(),
                    height: Self::DEFAULT_BACKPACK_PANEL_HEIGHT,
                    closable: true,
                    resizable: true,
                    movable: true,
                },
            ],
        }
    }
}

#[derive(Resource, Default)]
pub struct DockedPanelResizeState {
    pub panel_id: Option<usize>,
    pub start_cursor_y: f32,
    pub start_height: f32,
}

#[derive(Resource, Default)]
pub struct DockedPanelDragState {
    pub panel_id: Option<usize>,
    /// Cursor position at mouse-down, used as the anchor for the drag
    /// threshold — reordering doesn't fire until the cursor has moved
    /// at least [`DOCKED_PANEL_DRAG_THRESHOLD_PX`] from this point.
    /// `None` while idle.
    pub press_origin: Option<Vec2>,
    /// `true` once the cursor has moved past the threshold this drag.
    /// Latches on so a release after movement doesn't re-test.
    pub passed_threshold: bool,
}

/// Pixels the cursor must travel from the press point before clicks
/// on a docked-panel title bar start reordering. Plain clicks below
/// this stay no-ops so docked windows don't snap around on accidental
/// or focus-only clicks.
pub const DOCKED_PANEL_DRAG_THRESHOLD_PX: f32 = 6.0;

#[derive(Resource, Default)]
pub struct DragState {
    pub source: Option<DragSource>,
    pub object_id: Option<u64>,
    pub world_origin: Option<TilePosition>,
}

#[derive(Resource, Default)]
pub struct UseOnState {
    pub source: Option<ContextMenuTarget>,
}

#[derive(Resource, Default)]
pub struct SpellTargetingState {
    pub source: Option<ContextMenuTarget>,
    pub spell_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorMode {
    #[default]
    Default,
    UseOn,
    SpellTarget,
    AttackTarget,
}

impl CursorMode {}

#[derive(Resource, Default)]
pub struct CursorState {
    pub mode: CursorMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MinimapZoom {
    Close,
    Medium,
    Far,
}

impl Default for MinimapZoom {
    fn default() -> Self {
        Self::Medium
    }
}

impl MinimapZoom {
    pub const fn tile_span(self) -> i32 {
        match self {
            Self::Close => 15,
            Self::Medium => 33,
            Self::Far => 65,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Close => "Close",
            Self::Medium => "Medium",
            Self::Far => "Far",
        }
    }

    pub fn zoom_in(self) -> Self {
        match self {
            Self::Far => Self::Medium,
            Self::Medium => Self::Close,
            Self::Close => Self::Close,
        }
    }

    pub fn zoom_out(self) -> Self {
        match self {
            Self::Close => Self::Medium,
            Self::Medium => Self::Far,
            Self::Far => Self::Far,
        }
    }
}

#[derive(Resource)]
pub struct HudMinimapSettings {
    pub zoom: MinimapZoom,
}

impl Default for HudMinimapSettings {
    fn default() -> Self {
        Self {
            zoom: MinimapZoom::Medium,
        }
    }
}

#[derive(Resource)]
pub struct FullMapWindowState {
    pub open: bool,
    pub zoom: MinimapZoom,
}

impl Default for FullMapWindowState {
    fn default() -> Self {
        Self {
            open: false,
            zoom: MinimapZoom::Far,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuBarId {
    File,
    View,
    Window,
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    ToggleFullMap,
    ToggleStatus,
    ToggleBackpack,
    ToggleEquipment,
    ToggleMinimap,
    Logout,
    Quit,
}

#[derive(Resource, Default)]
pub struct OpenMenuState {
    pub open_id: Option<MenuBarId>,
}

#[derive(Resource, Default)]
pub struct PendingMenuActions {
    pub actions: Vec<MenuAction>,
}

/// Whether the Character sheet modal is currently open. Toggled by the HUD
/// player-sprite button; consumed by `manage_character_sheet_overlay` to
/// spawn/despawn the modal.
#[derive(Resource, Default)]
pub struct CharacterSheetState {
    pub open: bool,
}

/// Floating-popup state for the trade window. The window itself is a
/// `MovableWindow` — it's spawned dynamically when `session_id` becomes
/// `Some` and despawned when it returns to `None`. Position/size live on
/// the entity's `Node`; we cache the last values across sessions so re-opens
/// remember where the user left the window.
#[derive(Resource, Default)]
pub struct TradePopupState {
    pub session_id: Option<u64>,
    /// Last-seen window position (top-left, px). Populated when the window is
    /// despawned. `None` ⇒ open at center.
    pub last_position: Option<Vec2>,
    /// Last-seen window size (px). Populated when the window is despawned.
    /// `None` ⇒ use `DEFAULT_SIZE`.
    pub last_size: Option<Vec2>,
}

impl TradePopupState {
    pub const DEFAULT_SIZE: Vec2 = Vec2::new(720.0, 480.0);
    pub const MIN_SIZE: Vec2 = Vec2::new(480.0, 320.0);

    pub fn open(&mut self, session_id: u64) {
        self.session_id = Some(session_id);
    }

    pub fn close(&mut self) {
        self.session_id = None;
    }
}
