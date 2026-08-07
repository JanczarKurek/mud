//! [`MountablePanel`] impl for the Status (HP / MP / XP / effects /
//! carry-weight) panel. Singleton: `Key = ()`. All lifecycle plumbing
//! lives in [`crate::ui::mountable_panel`].

use bevy::prelude::*;

use crate::ui::components::{
    StatusPanelDockButton, StatusPanelFloatingCloseButton, StatusPanelFloatingRoot,
    StatusPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState, StatusPanelMode};
use crate::ui::setup::spawn_status_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct StatusPanel;

impl MountablePanel for StatusPanel {
    type Key = ();
    type Modes = StatusPanelMode;
    type UndockButton = StatusPanelUndockButton;
    type DockButton = StatusPanelDockButton;
    type FloatingRoot = StatusPanelFloatingRoot;
    type FloatingCloseButton = StatusPanelFloatingCloseButton;

    fn movable_window_id(_: ()) -> MovableWindowId {
        MovableWindowId::StatusPanel
    }
    fn floating_size(_: ()) -> Vec2 {
        Vec2::new(260.0, 180.0)
    }
    fn floating_position(_: ()) -> Vec2 {
        Vec2::new(360.0, 120.0)
    }
    fn panel_id_for(_: ()) -> usize {
        DockedPanelState::STATUS_PANEL_ID
    }
    fn active_keys(panel_state: &DockedPanelState) -> Vec<()> {
        if panel_state.is_open(Self::panel_id_for(())) {
            vec![()]
        } else {
            vec![]
        }
    }

    fn docked_definition(_: ()) -> Option<DockedPanel> {
        Some(DockedPanel {
            id: DockedPanelState::STATUS_PANEL_ID,
            kind: DockedPanelKind::Status,
            title: "Status".to_owned(),
            height: DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
            closable: true,
            resizable: true,
            movable: true,
        })
    }

    fn spawn_body(
        parent: &mut ChildSpawnerCommands,
        _: (),
        _theme: &UiThemeAssets,
        palette: &Palette,
        _asset_server: &AssetServer,
    ) {
        spawn_status_panel_body(parent, palette);
    }
}
