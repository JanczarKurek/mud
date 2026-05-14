//! [`MountablePanel`] impl for the Status (HP / MP / XP / effects /
//! carry-weight) panel. All lifecycle plumbing lives in
//! [`crate::ui::mountable_panel`]; this module supplies the per-panel
//! constants and points at the body builder.

use bevy::prelude::*;

use crate::ui::components::{
    StatusPanelDockButton, StatusPanelFloatingRoot, StatusPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanelKind, DockedPanelState, StatusPanelMode};
use crate::ui::setup::spawn_status_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

/// Zero-sized marker. Used as the type parameter for the generic
/// [`crate::ui::mountable_panel`] systems registered on the Status
/// panel.
pub struct StatusPanel;

impl MountablePanel for StatusPanel {
    type Mode = StatusPanelMode;
    type UndockButton = StatusPanelUndockButton;
    type DockButton = StatusPanelDockButton;
    type FloatingRoot = StatusPanelFloatingRoot;

    const PANEL_ID: usize = DockedPanelState::STATUS_PANEL_ID;
    const MOVABLE_WINDOW_ID: MovableWindowId = MovableWindowId::StatusPanel;
    const TITLE: &'static str = "Status";
    const FLOATING_SIZE: Vec2 = Vec2::new(260.0, 180.0);
    const FLOATING_POSITION: Vec2 = Vec2::new(360.0, 120.0);
    const PANEL_KIND: DockedPanelKind = DockedPanelKind::Status;
    const PANEL_HEIGHT: f32 = DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT;

    fn spawn_body(parent: &mut ChildSpawnerCommands, _theme: &UiThemeAssets, palette: &Palette) {
        spawn_status_panel_body(parent, palette);
    }
}
