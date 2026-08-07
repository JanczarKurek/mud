//! HSV color-picker helpers for the lighting-keyframe modal.
//!
//! Provides:
//! - sRGB ↔ HSV math (`rgb_to_hsv`, `hsv_to_rgb`).
//! - Pre-baked `Image` textures used as the background of the hue strip and
//!   saturation-value pad widgets. Built lazily on first request and cached on
//!   the `EditorColorPickerAssets` resource so the modal can rebuild without
//!   re-uploading textures every frame.
//!
//! Coordinate convention: hue ∈ `[0, 1)`, saturation/value ∈ `[0, 1]`.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

pub const HUE_STRIP_WIDTH: u32 = 256;
pub const HUE_STRIP_HEIGHT: u32 = 16;
pub const SV_PAD_SIZE: u32 = 128;

/// Cached gradient textures for the keyframe color picker. The hue strip is
/// built once; the SV pad is rebuilt whenever the active hue changes.
#[derive(Resource, Default)]
pub struct EditorColorPickerAssets {
    pub hue_strip: Option<Handle<Image>>,
    pub sv_pad: Option<Handle<Image>>,
    /// Hue currently baked into `sv_pad`. `f32::NAN` until first build.
    pub sv_pad_hue: f32,
}

/// Convert sRGB byte triple to HSV floats. Hue in [0, 1); S, V in [0, 1].
pub fn rgb_to_hsv(rgb: [u8; 3]) -> [f32; 3] {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;
    let s = if max <= 0.0 { 0.0 } else { delta / max };
    let h = if delta <= f32::EPSILON {
        0.0
    } else if (max - r).abs() < f32::EPSILON {
        ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } / 6.0;

    [h.rem_euclid(1.0), s.clamp(0.0, 1.0), v.clamp(0.0, 1.0)]
}

/// Convert HSV (h ∈ [0,1), s/v ∈ [0,1]) to sRGB byte triple.
pub fn hsv_to_rgb(hsv: [f32; 3]) -> [u8; 3] {
    let h = hsv[0].rem_euclid(1.0) * 6.0;
    let s = hsv[1].clamp(0.0, 1.0);
    let v = hsv[2].clamp(0.0, 1.0);

    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    ]
}

/// Build the horizontal hue strip texture (S = V = 1).
fn build_hue_strip_image() -> Image {
    let w = HUE_STRIP_WIDTH as usize;
    let h = HUE_STRIP_HEIGHT as usize;
    let mut data = vec![0u8; w * h * 4];
    for x in 0..w {
        let hue = x as f32 / w as f32;
        let rgb = hsv_to_rgb([hue, 1.0, 1.0]);
        for y in 0..h {
            let i = (y * w + x) * 4;
            data[i] = rgb[0];
            data[i + 1] = rgb[1];
            data[i + 2] = rgb[2];
            data[i + 3] = 255;
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: HUE_STRIP_WIDTH,
            height: HUE_STRIP_HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// Build the saturation-value pad texture for a given hue. x = saturation
/// (0 → 1, left → right), y = value (1 → 0, top → bottom).
fn build_sv_pad_image(hue: f32) -> Image {
    let size = SV_PAD_SIZE as usize;
    let mut data = vec![0u8; size * size * 4];
    for y in 0..size {
        let v = 1.0 - y as f32 / (size - 1) as f32;
        for x in 0..size {
            let s = x as f32 / (size - 1) as f32;
            let rgb = hsv_to_rgb([hue, s, v]);
            let i = (y * size + x) * 4;
            data[i] = rgb[0];
            data[i + 1] = rgb[1];
            data[i + 2] = rgb[2];
            data[i + 3] = 255;
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SV_PAD_SIZE,
            height: SV_PAD_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// Ensure the hue strip handle exists; returns the cached one otherwise.
pub fn ensure_hue_strip(
    assets: &mut EditorColorPickerAssets,
    images: &mut Assets<Image>,
) -> Handle<Image> {
    if let Some(handle) = &assets.hue_strip {
        return handle.clone();
    }
    let handle = images.add(build_hue_strip_image());
    assets.hue_strip = Some(handle.clone());
    handle
}

/// Ensure the SV pad handle reflects the requested hue. Rebuilds the
/// underlying image when the cached hue differs by more than a quantization
/// step (so subpixel scrubbing doesn't churn texture uploads).
pub fn ensure_sv_pad(
    hue: f32,
    assets: &mut EditorColorPickerAssets,
    images: &mut Assets<Image>,
) -> Handle<Image> {
    let needs_rebuild = match &assets.sv_pad {
        None => true,
        Some(_) => (assets.sv_pad_hue - hue).abs() > 1.0 / 256.0,
    };
    if needs_rebuild {
        let image = build_sv_pad_image(hue);
        let handle = if let Some(existing) = &assets.sv_pad {
            let _ = images.insert(existing, image);
            existing.clone()
        } else {
            let h = images.add(image);
            assets.sv_pad = Some(h.clone());
            h
        };
        assets.sv_pad_hue = hue;
        return handle;
    }
    assets.sv_pad.clone().expect("sv_pad checked above")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: u8, b: u8) -> bool {
        a.abs_diff(b) <= 1
    }

    #[test]
    fn pure_colors_roundtrip() {
        for rgb in [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 0],
            [255, 0, 255],
            [0, 255, 255],
        ] {
            let hsv = rgb_to_hsv(rgb);
            let back = hsv_to_rgb(hsv);
            assert!(
                close(rgb[0], back[0]) && close(rgb[1], back[1]) && close(rgb[2], back[2]),
                "roundtrip lost data: {rgb:?} -> {hsv:?} -> {back:?}",
            );
        }
    }

    #[test]
    fn grayscale_has_zero_saturation() {
        for v in [0u8, 64, 128, 200, 255] {
            let hsv = rgb_to_hsv([v, v, v]);
            assert!(hsv[1] < 0.001, "gray must have S~=0, got {hsv:?}");
        }
    }

    #[test]
    fn black_clamps_value() {
        let hsv = rgb_to_hsv([0, 0, 0]);
        assert!(hsv[2] < 0.001);
        let back = hsv_to_rgb(hsv);
        assert_eq!(back, [0, 0, 0]);
    }

    #[test]
    fn off_axis_roundtrip() {
        for rgb in [[200, 80, 30], [12, 200, 90], [40, 70, 200], [255, 220, 100]] {
            let hsv = rgb_to_hsv(rgb);
            let back = hsv_to_rgb(hsv);
            assert!(
                close(rgb[0], back[0]) && close(rgb[1], back[1]) && close(rgb[2], back[2]),
                "roundtrip lost data: {rgb:?} -> {hsv:?} -> {back:?}",
            );
        }
    }
}
