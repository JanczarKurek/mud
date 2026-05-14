//! [`MountablePanel`] impl for the Equipment paperdoll panel.

use bevy::prelude::*;

use crate::ui::components::{
    EquipmentPanelDockButton, EquipmentPanelFloatingRoot, EquipmentPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanelKind, DockedPanelState, EquipmentPanelMode};
use crate::ui::setup::spawn_equipment_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct EquipmentPanel;

impl MountablePanel for EquipmentPanel {
    type Mode = EquipmentPanelMode;
    type UndockButton = EquipmentPanelUndockButton;
    type DockButton = EquipmentPanelDockButton;
    type FloatingRoot = EquipmentPanelFloatingRoot;

    const PANEL_ID: usize = DockedPanelState::EQUIPMENT_PANEL_ID;
    const MOVABLE_WINDOW_ID: MovableWindowId = MovableWindowId::EquipmentPanel;
    const TITLE: &'static str = "Equipment";
    const FLOATING_SIZE: Vec2 = Vec2::new(300.0, 260.0);
    const FLOATING_POSITION: Vec2 = Vec2::new(380.0, 160.0);
    const PANEL_KIND: DockedPanelKind = DockedPanelKind::Equipment;
    const PANEL_HEIGHT: f32 = DockedPanelState::DEFAULT_EQUIPMENT_PANEL_HEIGHT;

    fn spawn_body(parent: &mut ChildSpawnerCommands, theme: &UiThemeAssets, palette: &Palette) {
        spawn_equipment_panel_body(parent, theme, palette);
    }
}
