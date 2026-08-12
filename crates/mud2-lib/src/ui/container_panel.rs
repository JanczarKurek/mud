//! [`MountablePanel`] impl for the pool of open inventory-grid panels
//! (world containers and inventory pouches share the same docked-pool
//! and the same 4×4 slot body). `Key = usize` is the sidebar slot
//! `panel_id`; the underlying `object_id` (containers) or
//! `backpack_slot` (pouches) is resolved on demand via
//! [`DockedPanelState`].

use bevy::prelude::*;

use crate::game::commands::{ContainerRef, GameCommand};
use crate::game::resources::ClientPendingCommands;
use crate::ui::components::{
    ContainerFloatingCloseButton, ContainerFloatingRoot, ContainerPanelDockButton,
    ContainerPanelUndockButton, ContainerTakeAllButton,
};
use crate::ui::mountable_panel::MountablePanel;
use crate::ui::movable_window::MovableWindowId;
use crate::ui::resources::{ContainerPanelModes, DockedPanelKind, DockedPanelState};
use crate::ui::setup::{spawn_container_panel_body, spawn_container_take_all_button};
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct ContainerPanel;

impl MountablePanel for ContainerPanel {
    type Key = usize;
    type Modes = ContainerPanelModes;
    type UndockButton = ContainerPanelUndockButton;
    type DockButton = ContainerPanelDockButton;
    type FloatingRoot = ContainerFloatingRoot;
    type FloatingCloseButton = ContainerFloatingCloseButton;

    fn movable_window_id(panel_id: usize) -> MovableWindowId {
        MovableWindowId::ContainerPanel { panel_id }
    }
    fn floating_size(_: usize) -> Vec2 {
        Vec2::new(360.0, 380.0)
    }
    /// Cascade so undocking several at once doesn't pile them up.
    fn floating_position(panel_id: usize) -> Vec2 {
        let index = panel_id.saturating_sub(DockedPanelState::FIRST_CONTAINER_PANEL_ID) as f32;
        Vec2::new(420.0, 160.0) + Vec2::splat(index * 24.0)
    }
    fn panel_id_for(panel_id: usize) -> usize {
        panel_id
    }

    fn active_keys(panel_state: &DockedPanelState) -> Vec<usize> {
        panel_state
            .panels
            .iter()
            .filter(|p| {
                matches!(
                    p.kind,
                    DockedPanelKind::Container { .. } | DockedPanelKind::PouchInBackpack { .. }
                )
            })
            .map(|p| p.id)
            .collect()
    }

    /// Containers also need the server to tear down the open container;
    /// pouches just need the docked row removed.
    fn handle_floating_close(
        panel_id: usize,
        panel_state: &mut DockedPanelState,
        pending: &mut ClientPendingCommands,
    ) {
        if let Some(object_id) = panel_state.container_object_id_for_panel(panel_id) {
            pending.push(GameCommand::CloseContainer { object_id });
        }
        panel_state.close_panel(panel_id);
    }

    fn spawn_title_extras(
        bar: &mut ChildSpawnerCommands,
        panel_id: usize,
        theme: &UiThemeAssets,
        palette: &Palette,
    ) {
        spawn_container_take_all_button(bar, theme, palette, panel_id);
    }

    fn spawn_body(
        parent: &mut ChildSpawnerCommands,
        panel_id: usize,
        theme: &UiThemeAssets,
        palette: &Palette,
        _asset_server: &AssetServer,
    ) {
        spawn_container_panel_body(parent, theme, palette, panel_id);
    }
}

/// "Take all" in a container/pouch panel body: ask the server to empty the
/// container into the backpack. Both the docked and the floating instance of a
/// panel carry the button, hence the `panel_id` dedupe.
pub fn handle_container_take_all_click(
    button_query: Query<(&Interaction, &ContainerTakeAllButton), Changed<Interaction>>,
    panel_state: Res<DockedPanelState>,
    mut pending: ResMut<ClientPendingCommands>,
) {
    let mut handled: Vec<usize> = Vec::new();
    for (interaction, button) in button_query.iter() {
        if !matches!(interaction, Interaction::Pressed) || handled.contains(&button.panel_id) {
            continue;
        }
        handled.push(button.panel_id);
        let container =
            if let Some(object_id) = panel_state.container_object_id_for_panel(button.panel_id) {
                ContainerRef::World { object_id }
            } else if let Some(backpack_slot) =
                panel_state.pouch_backpack_slot_for_panel(button.panel_id)
            {
                ContainerRef::PouchInBackpack { backpack_slot }
            } else {
                continue;
            };
        pending.push(GameCommand::TakeAllFromContainer { container });
    }
}
