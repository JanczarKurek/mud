pub mod components;
pub mod resources;
pub mod setup;
pub mod systems;

use bevy::prelude::*;

use crate::ui::resources::{DragState, InventoryState, OpenContainerState};
use crate::ui::setup::spawn_hud;
use crate::ui::systems::{
    handle_collectible_dragging, manage_open_containers, sync_active_container_slots,
    sync_drag_preview, sync_vital_bars,
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InventoryState::default())
            .insert_resource(OpenContainerState::default())
            .insert_resource(DragState::default())
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    manage_open_containers,
                    sync_vital_bars,
                    sync_active_container_slots,
                    handle_collectible_dragging,
                    sync_drag_preview,
                ),
            );
    }
}
