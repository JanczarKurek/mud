//! [`MountablePanel`] impl for the Backpack inventory panel.

use bevy::prelude::*;

use crate::ui::components::{
    BackpackPanelDockButton, BackpackPanelFloatingRoot, BackpackPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{BackpackPanelMode, DockedPanelKind, DockedPanelState};
use crate::ui::setup::spawn_backpack_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct BackpackPanel;

impl MountablePanel for BackpackPanel {
    type Mode = BackpackPanelMode;
    type UndockButton = BackpackPanelUndockButton;
    type DockButton = BackpackPanelDockButton;
    type FloatingRoot = BackpackPanelFloatingRoot;

    const PANEL_ID: usize = DockedPanelState::BACKPACK_PANEL_ID;
    const MOVABLE_WINDOW_ID: MovableWindowId = MovableWindowId::BackpackPanel;
    const TITLE: &'static str = "Backpack";
    const FLOATING_SIZE: Vec2 = Vec2::new(280.0, 320.0);
    const FLOATING_POSITION: Vec2 = Vec2::new(420.0, 200.0);
    const PANEL_KIND: DockedPanelKind = DockedPanelKind::Backpack;
    const PANEL_HEIGHT: f32 = DockedPanelState::DEFAULT_BACKPACK_PANEL_HEIGHT;

    fn spawn_body(parent: &mut ChildSpawnerCommands, theme: &UiThemeAssets, palette: &Palette) {
        spawn_backpack_panel_body(parent, theme, palette);
    }
}
