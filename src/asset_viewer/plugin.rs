use bevy::prelude::*;

use crate::asset_viewer::reload::{
    drain_file_watcher_events, handle_reload_requests, refresh_inspector_on_reload,
    AssetReloadCompleted, AssetReloadRequest,
};
use crate::asset_viewer::resources::{
    InspectorBuffer, PreviewState, SelfWriteSuppressor, ViewerState,
};
use crate::asset_viewer::systems::{
    apply_clip_change, attach_preview_animation, handle_clip_button_clicks, handle_conflict_keep,
    handle_conflict_reload, handle_filter_click, handle_inspector_row_click, handle_keyboard,
    handle_palette_clicks, handle_reload_button, handle_save_button, handle_tab_clicks,
    handle_viewer_zoom, setup_viewer_camera, sync_clip_button_highlight, sync_clip_buttons,
    sync_filter_text, sync_inspector_panel, sync_palette, sync_save_button, sync_tab_buttons,
    sync_top_bar_title, update_preview,
};
use crate::asset_viewer::ui::spawn_viewer_hud;
use crate::asset_viewer::watcher::setup_asset_watcher;
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
            .init_resource::<SelfWriteSuppressor>()
            .add_message::<AssetReloadRequest>()
            .add_message::<AssetReloadCompleted>()
            .add_systems(
                Startup,
                (
                    setup_viewer_camera,
                    spawn_viewer_hud.after(setup_viewer_camera),
                    setup_asset_watcher,
                ),
            )
            .add_systems(
                Update,
                (
                    // Reload pipeline — ordered so save → drain → handle →
                    // refresh runs before the regular preview/inspector sync.
                    handle_save_button,
                    handle_reload_button.after(handle_save_button),
                    drain_file_watcher_events.after(handle_reload_button),
                    handle_reload_requests.after(drain_file_watcher_events),
                    refresh_inspector_on_reload.after(handle_reload_requests),
                    update_preview.after(refresh_inspector_on_reload),
                    attach_preview_animation.after(update_preview),
                    advance_animation_timers,
                    apply_clip_change,
                    handle_viewer_zoom,
                ),
            )
            .add_systems(
                Update,
                (
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
                    sync_save_button,
                    handle_conflict_reload,
                    handle_conflict_keep,
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
