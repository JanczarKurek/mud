//! [`MountablePanel`] impl for the Nearby NPCs panel.

use bevy::prelude::*;

use crate::ui::components::{
    NearbyNpcsPanelDockButton, NearbyNpcsPanelFloatingCloseButton, NearbyNpcsPanelFloatingRoot,
    NearbyNpcsPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState, NearbyNpcsPanelMode};
use crate::ui::setup::spawn_nearby_npcs_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct NearbyNpcsPanel;

impl MountablePanel for NearbyNpcsPanel {
    type Key = ();
    type Modes = NearbyNpcsPanelMode;
    type UndockButton = NearbyNpcsPanelUndockButton;
    type DockButton = NearbyNpcsPanelDockButton;
    type FloatingRoot = NearbyNpcsPanelFloatingRoot;
    type FloatingCloseButton = NearbyNpcsPanelFloatingCloseButton;

    fn movable_window_id(_: ()) -> MovableWindowId {
        MovableWindowId::NearbyNpcsPanel
    }
    fn floating_size(_: ()) -> Vec2 {
        Vec2::new(260.0, 220.0)
    }
    fn floating_position(_: ()) -> Vec2 {
        Vec2::new(460.0, 240.0)
    }
    fn panel_id_for(_: ()) -> usize {
        DockedPanelState::NEARBY_NPCS_PANEL_ID
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
            id: DockedPanelState::NEARBY_NPCS_PANEL_ID,
            kind: DockedPanelKind::NearbyNpcs,
            title: "Nearby NPCs".to_owned(),
            height: DockedPanelState::DEFAULT_TARGET_PANEL_HEIGHT,
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
        spawn_nearby_npcs_panel_body(parent, palette);
    }
}
