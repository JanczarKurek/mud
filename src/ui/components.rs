use bevy::prelude::*;

use crate::world::object_definitions::EquipmentSlot;

#[derive(Component)]
pub struct HealthFill;

#[derive(Component)]
pub struct ManaFill;

#[derive(Component)]
pub struct ItemSlotButton {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct ItemSlotImage {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct OpenContainerTitle;

#[derive(Component)]
pub struct CloseContainerButton;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemSlotKind {
    ActiveContainer(usize),
    Equipment(EquipmentSlot),
}
