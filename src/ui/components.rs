use bevy::prelude::*;

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
pub struct DockedPanelTitle {
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
pub struct CurrentTargetPanelContent;

#[derive(Component)]
pub struct ContainerPanelContent;

#[derive(Component)]
pub struct CurrentCombatTargetLabel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemSlotKind {
    Backpack(usize),
    OpenContainer { panel_id: usize, slot_index: usize },
    Equipment(EquipmentSlot),
}
