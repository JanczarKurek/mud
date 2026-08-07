//! [`MountablePanel`] impl for the Backpack inventory panel.

use bevy::prelude::*;

use crate::ui::components::{
    BackpackPanelDockButton, BackpackPanelFloatingCloseButton, BackpackPanelFloatingRoot,
    BackpackPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{BackpackPanelMode, DockedPanel, DockedPanelKind, DockedPanelState};
use crate::ui::setup::spawn_backpack_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct BackpackPanel;

impl MountablePanel for BackpackPanel {
    type Key = ();
    type Modes = BackpackPanelMode;
    type UndockButton = BackpackPanelUndockButton;
    type DockButton = BackpackPanelDockButton;
    type FloatingRoot = BackpackPanelFloatingRoot;
    type FloatingCloseButton = BackpackPanelFloatingCloseButton;

    fn movable_window_id(_: ()) -> MovableWindowId {
        MovableWindowId::BackpackPanel
    }
    fn floating_size(_: ()) -> Vec2 {
        Vec2::new(280.0, 320.0)
    }
    fn floating_position(_: ()) -> Vec2 {
        Vec2::new(420.0, 200.0)
    }
    fn panel_id_for(_: ()) -> usize {
        DockedPanelState::BACKPACK_PANEL_ID
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
            id: DockedPanelState::BACKPACK_PANEL_ID,
            kind: DockedPanelKind::Backpack,
            title: "Backpack".to_owned(),
            height: DockedPanelState::DEFAULT_BACKPACK_PANEL_HEIGHT,
            closable: true,
            resizable: true,
            movable: true,
        })
    }

    fn spawn_body(
        parent: &mut ChildSpawnerCommands,
        _: (),
        theme: &UiThemeAssets,
        palette: &Palette,
        _asset_server: &AssetServer,
    ) {
        spawn_backpack_panel_body(parent, theme, palette);
    }
}
