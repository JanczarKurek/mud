//! [`MountablePanel`] impl for the combat-target panel.

use bevy::prelude::*;

use crate::ui::components::{
    CurrentTargetPanelDockButton, CurrentTargetPanelFloatingCloseButton,
    CurrentTargetPanelFloatingRoot, CurrentTargetPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{CurrentTargetPanelMode, DockedPanelState};
use crate::ui::setup::spawn_current_target_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct CurrentTargetPanel;

impl MountablePanel for CurrentTargetPanel {
    type Key = ();
    type Modes = CurrentTargetPanelMode;
    type UndockButton = CurrentTargetPanelUndockButton;
    type DockButton = CurrentTargetPanelDockButton;
    type FloatingRoot = CurrentTargetPanelFloatingRoot;
    type FloatingCloseButton = CurrentTargetPanelFloatingCloseButton;

    fn movable_window_id(_: ()) -> MovableWindowId {
        MovableWindowId::CurrentTargetPanel
    }
    fn floating_size(_: ()) -> Vec2 {
        Vec2::new(260.0, 140.0)
    }
    fn floating_position(_: ()) -> Vec2 {
        Vec2::new(460.0, 240.0)
    }
    fn panel_id_for(_: ()) -> usize {
        DockedPanelState::CURRENT_TARGET_PANEL_ID
    }
    fn active_keys(panel_state: &DockedPanelState) -> Vec<()> {
        if panel_state.is_open(Self::panel_id_for(())) {
            vec![()]
        } else {
            vec![]
        }
    }

    fn spawn_body(
        parent: &mut ChildSpawnerCommands,
        _: (),
        _theme: &UiThemeAssets,
        palette: &Palette,
        _asset_server: &AssetServer,
    ) {
        spawn_current_target_panel_body(parent, palette);
    }
}
