//! [`MountablePanel`] impl for the Minimap panel.
//!
//! The body builder creates a fresh `Image` asset for each instance.
//! `update_minimap_images` iterates over every `MinimapView` entity so
//! the docked + floating instances render independently; zoom is
//! shared via `HudMinimapSettings`.

use bevy::prelude::*;

use crate::ui::components::{
    MinimapPanelDockButton, MinimapPanelFloatingCloseButton, MinimapPanelFloatingRoot,
    MinimapPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState, MinimapPanelMode};
use crate::ui::setup::spawn_minimap_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct MinimapPanel;

impl MountablePanel for MinimapPanel {
    type Key = ();
    type Modes = MinimapPanelMode;
    type UndockButton = MinimapPanelUndockButton;
    type DockButton = MinimapPanelDockButton;
    type FloatingRoot = MinimapPanelFloatingRoot;
    type FloatingCloseButton = MinimapPanelFloatingCloseButton;

    fn movable_window_id(_: ()) -> MovableWindowId {
        MovableWindowId::MinimapPanel
    }
    fn floating_size(_: ()) -> Vec2 {
        Vec2::new(320.0, 360.0)
    }
    fn floating_position(_: ()) -> Vec2 {
        Vec2::new(500.0, 80.0)
    }
    fn panel_id_for(_: ()) -> usize {
        DockedPanelState::MINIMAP_PANEL_ID
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
            id: DockedPanelState::MINIMAP_PANEL_ID,
            kind: DockedPanelKind::Minimap,
            title: "Minimap".to_owned(),
            height: DockedPanelState::DEFAULT_MINIMAP_PANEL_HEIGHT,
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
        asset_server: &AssetServer,
    ) {
        spawn_minimap_panel_body(parent, theme, palette, asset_server);
    }
}
