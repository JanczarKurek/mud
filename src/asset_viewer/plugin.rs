use bevy::prelude::*;

use crate::asset_viewer::resources::{InspectorBuffer, PreviewState, ViewerState};
use crate::asset_viewer::systems::{
    apply_clip_change, attach_preview_animation, handle_clip_button_clicks, handle_filter_click,
    handle_inspector_row_click, handle_keyboard, handle_palette_clicks, handle_save_button,
    handle_tab_clicks, handle_viewer_zoom, setup_viewer_camera, sync_clip_button_highlight,
    sync_clip_buttons, sync_filter_text, sync_inspector_panel, sync_palette, sync_save_button,
    sync_tab_buttons, sync_top_bar_title, update_preview,
};
use crate::asset_viewer::ui::spawn_viewer_hud;
use crate::magic::resources::SpellDefinitions;
use crate::world::animation::advance_animation_timers;
use crate::world::object_definitions::OverworldObjectDefinitions;

pub struct AssetViewerPlugin;

impl Plugin for AssetViewerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(OverworldObjectDefinitions::load_from_disk())
            .insert_resource(SpellDefinitions::load_from_disk())
            .init_resource::<ViewerState>()
            .init_resource::<PreviewState>()
            .init_resource::<InspectorBuffer>()
            .add_systems(
                Startup,
                (
                    setup_viewer_camera,
                    spawn_viewer_hud.after(setup_viewer_camera),
                ),
            )
            .add_systems(
                Update,
                (
                    // Preview
                    update_preview,
                    attach_preview_animation.after(update_preview),
                    advance_animation_timers,
                    apply_clip_change,
                    handle_viewer_zoom,
                    // Palette
                    handle_palette_clicks,
                    handle_filter_click,
                    handle_tab_clicks,
                    sync_palette,
                    sync_filter_text,
                    sync_tab_buttons,
                    // Inspector
                    sync_inspector_panel,
                    handle_inspector_row_click,
                    handle_save_button,
                    sync_save_button,
                    // Clip buttons
                    sync_clip_buttons,
                    handle_clip_button_clicks,
                    sync_clip_button_highlight,
                    // Keyboard (filter + inspector editing)
                    handle_keyboard,
                    // Top bar
                    sync_top_bar_title,
                ),
            );
    }
}
