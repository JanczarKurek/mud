use bevy::prelude::*;

use crate::ui::resources::{MenuAction, MenuBarId, MinimapZoom};
use crate::world::object_definitions::EquipmentSlot;

#[derive(Component)]
pub struct HealthFill;

#[derive(Component)]
pub struct ManaFill;

#[derive(Component)]
pub struct HealthLabel;

#[derive(Component)]
pub struct ManaLabel;

#[derive(Component)]
pub struct ItemSlotButton {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct ItemSlotImage {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct EquipmentSlotButton;

#[derive(Component)]
pub struct ContainerSlotButton;

#[derive(Component)]
pub struct EquipmentSlotImage;

#[derive(Component)]
pub struct ContainerSlotImage;

#[derive(Component)]
pub struct DockedPanelRoot {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelCanvas;

#[derive(Component)]
pub struct DockedPanelTitle {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelDragHandle {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelCloseButton {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelBody {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelResizeHandle {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DragPreviewRoot;

#[derive(Component)]
pub struct DragPreviewLabel;

#[derive(Component)]
pub struct PythonConsolePanel;

#[derive(Component)]
pub struct PythonConsoleOutput;

#[derive(Component)]
pub struct PythonConsoleInput;

#[derive(Component)]
pub struct PythonConsoleOutputViewport;

#[derive(Component)]
pub struct PythonConsoleScrollbarThumb;

#[derive(Component)]
pub struct ChatLogText;

#[derive(Component)]
pub struct ContextMenuRoot;

#[derive(Component)]
pub struct ContextMenuInspectButton;

#[derive(Component)]
pub struct ContextMenuOpenButton;

#[derive(Component)]
pub struct ContextMenuUseButton;

#[derive(Component)]
pub struct ContextMenuUseOnButton;

#[derive(Component)]
pub struct ContextMenuAttackButton;

#[derive(Component)]
pub struct ContextMenuTakePartialButton;

#[derive(Component)]
pub struct ContextMenuTalkButton;

/// Single dynamic-label button for stateful-object interactions ("Open" /
/// "Close" / "Light" / "Extinguish" / "Pull"). The label is rewritten each
/// time the menu opens against the verb chosen for the currently hovered
/// object's state.
#[derive(Component)]
pub struct ContextMenuInteractButton;

#[derive(Component)]
pub struct DialogPanelRoot;

#[derive(Component)]
pub struct DialogPanelSpeakerLabel;

#[derive(Component)]
pub struct DialogPanelBodyText;

#[derive(Component)]
pub struct DialogPanelOptionsContainer;

#[derive(Component)]
pub struct DialogPanelContinueButton;

#[derive(Component)]
pub struct DialogPanelCloseButton;

#[derive(Component)]
pub struct DialogPanelOptionButton {
    pub option_idx: usize,
}

#[derive(Component)]
pub struct CurrentTargetPanelContent;

#[derive(Component)]
pub struct ContainerPanelContent;

#[derive(Component)]
pub struct StatusPanelContent;

#[derive(Component)]
pub struct EquipmentPanelContent;

#[derive(Component)]
pub struct BackpackPanelContent;

#[derive(Component)]
pub struct CurrentCombatTargetLabel;

#[derive(Component)]
pub struct RightSidebarRoot;

#[derive(Component)]
pub struct BackpackSlotRow {
    pub row_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemSlotKind {
    Backpack(usize),
    OpenContainer { panel_id: usize, slot_index: usize },
    Equipment(EquipmentSlot),
}

#[derive(Component)]
pub struct ItemSlotQuantityLabel {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct TakePartialPopupRoot;

#[derive(Component)]
pub struct TakePartialDecButton;

#[derive(Component)]
pub struct TakePartialIncButton;

#[derive(Component)]
pub struct TakePartialConfirmButton;

#[derive(Component)]
pub struct TakePartialCancelButton;

#[derive(Component)]
pub struct TakePartialAmountLabel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MinimapMode {
    HudSmall,
    FullscreenLarge,
}

#[derive(Component)]
pub struct MinimapView {
    pub mode: MinimapMode,
}

/// Holds the `Image` asset handle backing the tile window for a `MinimapView`.
/// Swapped out when zoom changes (tile span dictates image size); otherwise
/// the bytes inside are rewritten in place each frame.
#[derive(Component)]
pub struct MinimapCanvas {
    pub image_handle: Handle<Image>,
    pub last_zoom: Option<MinimapZoom>,
}

#[derive(Component)]
pub struct MinimapOverlayDot;

#[derive(Component)]
pub struct HudMinimapZoomLabel;

#[derive(Component)]
pub struct HudMinimapZoomInButton;

#[derive(Component)]
pub struct HudMinimapZoomOutButton;

#[derive(Component)]
pub struct FullMapWindowRoot;

#[derive(Component)]
pub struct FullMapZoomLabel;

#[derive(Component)]
pub struct FullMapZoomInButton;

#[derive(Component)]
pub struct FullMapZoomOutButton;

#[derive(Component)]
pub struct FullMapCloseButton;

#[derive(Component)]
pub struct FullMapBodyRoot;

#[derive(Component)]
pub struct MenuBarRoot;

#[derive(Component)]
pub struct MenuBarItemButton {
    pub menu: MenuBarId,
}

#[derive(Component)]
pub struct MenuDropdownRoot {
    pub menu: MenuBarId,
}

#[derive(Component)]
pub struct MenuDropdownEntryButton {
    pub action: MenuAction,
}
