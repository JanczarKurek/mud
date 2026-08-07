//! [`MountablePanel`] impl for the Minimap panel.
//!
//! When docked, the body renders the small HUD minimap (`HudSmall` mode,
//! 220×220) keyed off `HudMinimapSettings.zoom`. When floating, the body
//! renders the larger pop-out minimap (`FullscreenLarge` mode, 520×520)
//! keyed off `FloatingMinimapZoom`. The two views have independent zoom
//! state; pressing `M` toggles the panel between the two modes.

use bevy::prelude::*;

use crate::ui::components::{
    MinimapPanelDockButton, MinimapPanelFloatingCloseButton, MinimapPanelFloatingRoot,
    MinimapPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState, MinimapPanelMode};
use crate::ui::setup::{spawn_minimap_panel_body_for_mode, BodyMode};
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
        // Big enough to host the 520×520 minimap image plus title bar
        // and zoom row; the user can resize from the corner.
        Vec2::new(560.0, 620.0)
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
        // `spawn_body` is only invoked by the floating-window lifecycle.
        // `update_minimap_images` will re-snap the image to the saved
        // `FloatingMinimapZoom` on the next frame.
        spawn_minimap_panel_body_for_mode(
            parent,
            theme,
            palette,
            asset_server,
            BodyMode::Floating,
            crate::ui::resources::MinimapZoom::Far,
        );
    }
}
