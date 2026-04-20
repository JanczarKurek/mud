use bevy::prelude::*;

use crate::game::commands::ItemReference;
use crate::ui::components::ItemSlotKind;
use crate::world::components::TilePosition;

#[derive(Clone, Copy)]
pub enum ContextMenuTarget {
    World(u64),
    Slot(ItemSlotKind, u64),
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
}

impl ContextMenuState {
    pub fn show(
        &mut self,
        position: Vec2,
        target: ContextMenuTarget,
        can_open: bool,
        can_use: bool,
        can_use_on: bool,
        can_attack: bool,
        can_take_partial: bool,
    ) {
        self.position = position;
        self.target = Some(target);
        self.can_open = can_open;
        self.can_use = can_use;
        self.can_use_on = can_use_on;
        self.can_attack = can_attack;
        self.can_take_partial = can_take_partial;
    }

    pub fn hide(&mut self) {
        self.target = None;
        self.can_open = false;
        self.can_use = false;
        self.can_use_on = false;
        self.can_attack = false;
        self.can_take_partial = false;
    }

    pub fn is_visible(&self) -> bool {
        self.target.is_some()
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
    Container { object_id: u64 },
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
            Some(DockedPanelKind::Minimap)
            | Some(DockedPanelKind::Status)
            | Some(DockedPanelKind::Equipment)
            | Some(DockedPanelKind::Backpack)
            | Some(DockedPanelKind::CurrentTarget)
            | None => None,
        }
    }

    pub fn is_open(&self, panel_id: usize) -> bool {
        self.panel(panel_id).is_some()
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
            DockedPanelKind::Container { .. } => Some(panel.id),
            DockedPanelKind::Minimap
            | DockedPanelKind::Status
            | DockedPanelKind::Equipment
            | DockedPanelKind::Backpack
            | DockedPanelKind::CurrentTarget => None,
        })
    }

    fn close_oldest_container_if_needed(&mut self) {
        let open_container_count = self
            .panels
            .iter()
            .filter(|panel| matches!(panel.kind, DockedPanelKind::Container { .. }))
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
            panels: vec![
                DockedPanel {
                    id: Self::MINIMAP_PANEL_ID,
                    kind: DockedPanelKind::Minimap,
                    title: "Minimap".to_owned(),
                    height: Self::DEFAULT_MINIMAP_PANEL_HEIGHT,
                    closable: false,
                    resizable: true,
                    movable: true,
                },
                DockedPanel {
                    id: Self::STATUS_PANEL_ID,
                    kind: DockedPanelKind::Status,
                    title: "Status".to_owned(),
                    height: Self::DEFAULT_STATUS_PANEL_HEIGHT,
                    closable: false,
                    resizable: true,
                    movable: true,
                },
                DockedPanel {
                    id: Self::EQUIPMENT_PANEL_ID,
                    kind: DockedPanelKind::Equipment,
                    title: "Equipment".to_owned(),
                    height: Self::DEFAULT_EQUIPMENT_PANEL_HEIGHT,
                    closable: false,
                    resizable: true,
                    movable: true,
                },
                DockedPanel {
                    id: Self::BACKPACK_PANEL_ID,
                    kind: DockedPanelKind::Backpack,
                    title: "Backpack".to_owned(),
                    height: Self::DEFAULT_BACKPACK_PANEL_HEIGHT,
                    closable: false,
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
}

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
