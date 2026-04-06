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
