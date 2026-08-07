//! [`MountablePanel`] impl for the Equipment paperdoll panel.

use bevy::prelude::*;

use crate::ui::components::{
    EquipmentPanelDockButton, EquipmentPanelFloatingCloseButton, EquipmentPanelFloatingRoot,
    EquipmentPanelUndockButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState, EquipmentPanelMode};
use crate::ui::setup::spawn_equipment_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct EquipmentPanel;

impl MountablePanel for EquipmentPanel {
    type Key = ();
    type Modes = EquipmentPanelMode;
    type UndockButton = EquipmentPanelUndockButton;
    type DockButton = EquipmentPanelDockButton;
    type FloatingRoot = EquipmentPanelFloatingRoot;
    type FloatingCloseButton = EquipmentPanelFloatingCloseButton;

    fn movable_window_id(_: ()) -> MovableWindowId {
        MovableWindowId::EquipmentPanel
    }
    fn floating_size(_: ()) -> Vec2 {
        Vec2::new(300.0, 260.0)
    }
    fn floating_position(_: ()) -> Vec2 {
        Vec2::new(380.0, 160.0)
    }
    fn panel_id_for(_: ()) -> usize {
        DockedPanelState::EQUIPMENT_PANEL_ID
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
            id: DockedPanelState::EQUIPMENT_PANEL_ID,
            kind: DockedPanelKind::Equipment,
            title: "Equipment".to_owned(),
            height: DockedPanelState::DEFAULT_EQUIPMENT_PANEL_HEIGHT,
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
        spawn_equipment_panel_body(parent, theme, palette);
    }
}
