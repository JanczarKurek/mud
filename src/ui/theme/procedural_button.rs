//! Procedurally-generated 9-slice frames for Primary / Secondary buttons.
//!
//! Three small RGBA images are built once at startup and registered as
//! `Assets<Image>`. The shared tint system in `widgets.rs` swaps which one a
//! button uses based on `Interaction` state, giving a warm gold beveled look
//! with a true pushed-in pressed state — all without external art files.
//!
//! Image layout (12×12, 4px 9-slice border):
//! - Outer corner pixels are alpha-cut for a subtle rounding.
//! - 1px highlight strip on top + left edges (idle).
//! - 1px shadow strip on bottom + right edges (idle).
//! - Inner pixels are a flat fill (uniform under 9-slice stretching).
//! - Pressed inverts the bevel direction and darkens the fill.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SIZE: u32 = 12;

#[derive(Clone)]
pub(crate) struct ButtonFrameHandles {
    pub idle: Handle<Image>,
    pub hover: Handle<Image>,
    pub pressed: Handle<Image>,
}

#[derive(Clone, Copy)]
struct FrameRecipe {
    top_left_bevel: [u8; 4],
    bottom_right_bevel: [u8; 4],
    fill: [u8; 4],
}

pub(crate) fn build_button_frames(images: &mut Assets<Image>) -> ButtonFrameHandles {
    // Warm gold base.
    let highlight = rgba(0.98, 0.85, 0.50);
    let shadow = rgba(0.38, 0.22, 0.06);
    let fill = rgba(0.80, 0.56, 0.22);

    // Brightened ~12% for hover.
    let highlight_hi = rgba(1.00, 0.95, 0.62);
    let shadow_hi = rgba(0.50, 0.32, 0.12);
    let fill_hi = rgba(0.93, 0.70, 0.32);

    // Pressed: bevel inverts and the fill darkens so the surface reads as
    // sunken into the panel.
    let highlight_lo = rgba(0.85, 0.62, 0.28);
    let shadow_lo = rgba(0.28, 0.16, 0.04);
    let fill_lo = rgba(0.65, 0.42, 0.14);

    let idle = FrameRecipe {
        top_left_bevel: highlight,
        bottom_right_bevel: shadow,
        fill,
    };
    let hover = FrameRecipe {
        top_left_bevel: highlight_hi,
        bottom_right_bevel: shadow_hi,
        fill: fill_hi,
    };
    let pressed = FrameRecipe {
        top_left_bevel: shadow_lo,
        bottom_right_bevel: highlight_lo,
        fill: fill_lo,
    };

    ButtonFrameHandles {
        idle: images.add(build_image(idle)),
        hover: images.add(build_image(hover)),
        pressed: images.add(build_image(pressed)),
    }
}

fn build_image(recipe: FrameRecipe) -> Image {
    let w = SIZE as usize;
    let h = SIZE as usize;
    let mut data = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let pixel = if is_rounded_corner(x, y, w, h) {
                [0, 0, 0, 0]
            } else if y == 0 || x == 0 {
                recipe.top_left_bevel
            } else if y == h - 1 || x == w - 1 {
                recipe.bottom_right_bevel
            } else {
                recipe.fill
            };
            data[i..i + 4].copy_from_slice(&pixel);
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // Crisp pixel art — no blurring when the 9-slice center stretches.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
    image
}

fn is_rounded_corner(x: usize, y: usize, w: usize, h: usize) -> bool {
    (x == 0 && y == 0)
        || (x == w - 1 && y == 0)
        || (x == 0 && y == h - 1)
        || (x == w - 1 && y == h - 1)
}

fn rgba(r: f32, g: f32, b: f32) -> [u8; 4] {
    [
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
        255,
    ]
}
