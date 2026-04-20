use bevy::prelude::*;
use bevy::sprite::{BorderRect, SliceScaleMode, TextureSlicer};
use bevy::ui::widget::NodeImageMode;

/// Handles and 9-slice config for every themed surface. Placeholder art lives
/// in `assets/ui/theme/*.png` (plain white PNGs); real hand-painted frames
/// drop in later by overwriting those files.
#[derive(Resource, Clone)]
pub struct UiThemeAssets {
    pub panel_frame: Handle<Image>,
    pub panel_frame_slicer: TextureSlicer,
    pub title_bar: Handle<Image>,
    pub title_bar_slicer: TextureSlicer,
    pub button_frame: Handle<Image>,
    pub button_frame_slicer: TextureSlicer,
    pub slot_frame: Handle<Image>,
    pub slot_frame_slicer: TextureSlicer,
    pub divider: Handle<Image>,
}

impl UiThemeAssets {
    pub fn load(asset_server: &AssetServer) -> Self {
        Self {
            panel_frame: asset_server.load("ui/theme/panel_frame.png"),
            panel_frame_slicer: slicer(8.0),
            title_bar: asset_server.load("ui/theme/title_bar.png"),
            title_bar_slicer: slicer(4.0),
            button_frame: asset_server.load("ui/theme/button_frame.png"),
            button_frame_slicer: slicer(4.0),
            slot_frame: asset_server.load("ui/theme/slot_frame.png"),
            slot_frame_slicer: slicer(2.0),
            divider: asset_server.load("ui/theme/divider.png"),
        }
    }

    pub fn panel_image_mode(&self) -> NodeImageMode {
        NodeImageMode::Sliced(self.panel_frame_slicer.clone())
    }

    pub fn title_bar_image_mode(&self) -> NodeImageMode {
        NodeImageMode::Sliced(self.title_bar_slicer.clone())
    }

    pub fn button_image_mode(&self) -> NodeImageMode {
        NodeImageMode::Sliced(self.button_frame_slicer.clone())
    }

    pub fn slot_image_mode(&self) -> NodeImageMode {
        NodeImageMode::Sliced(self.slot_frame_slicer.clone())
    }
}

fn slicer(border: f32) -> TextureSlicer {
    TextureSlicer {
        border: BorderRect::all(border),
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 1.0,
    }
}
