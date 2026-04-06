use bevy::prelude::*;

#[derive(Component)]
pub struct HealthFill;

#[derive(Component)]
pub struct ManaFill;

#[derive(Component)]
pub struct ContainerSlot {
    pub index: usize,
}

#[derive(Component)]
pub struct ContainerSlotImage {
    pub index: usize,
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
