//! Programmatic floor **flavors**: in-memory transforms of base floor tilesets.
//!
//! A *flavor* derives a new floor type from an existing tileset by transforming
//! its atlas pixels at load time. The first flavor, [`FloorFlavor::Flooring`],
//! squares each tile off to its grid footprint so the floor lines up flush with
//! walls (the base art's organic, slightly-oversized autotile edges otherwise
//! read as "too large" against the square wall grid).
//!
//! Derived floors are addressed by `derive_floor_id(base, flavor)` and render
//! through the normal floor path (`crate::world::floor_render`): the only
//! difference is the atlas image, which this module generates and stores in
//! [`FloorTilesetAtlases::images`] keyed by the derived id. Everything else —
//! layout, variants, priority, transitions — is shared with the base via the
//! flavor-aware `FloorTilesetDefinitions::get`.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension};

use crate::world::floor_definitions::{
    derive_floor_id, split_floor_id, FloorFlavor, FloorTilesetDefinitions, FloorTypeId,
};
use crate::world::floor_render::{
    FloorTilesetAtlases, AUTHORING_INDEX_TO_MASK, MASK_TO_AUTHORING_INDEX,
};

/// Bumped each time [`generate_floor_flavor_atlases`] registers new flavor
/// atlases. Both floor-render build systems (in-game and editor) watch this and
/// invalidate their cached `built_for` hashes on a change, so any cells already
/// drawn with a base/placeholder handle rebuild against the generated atlas.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct FloorFlavorGeneration(pub u64);

/// Quadrant table: `(mask bit, dest x-half, dest y-half)` — which half-tile
/// quadrant of an authoring tile each corner-mask bit drives.
///
/// Derived from the dual-grid render geometry. A corner cell is centered on a
/// tile *corner*; its source quadrants map to screen quadrants with grid +y =
/// world +y (up, per `world::systems` tile→world). The renderer's bit→tile
/// mapping (`spawn_render_cells_at_corner`) is 1=(rx-1,ry-1), 2=(rx,ry-1),
/// 4=(rx-1,ry), 8=(rx,ry); projecting each tile to its cell quadrant:
///   bit 1 → cell lower-left  → source bottom-left  (0, 1)
///   bit 2 → cell lower-right → source bottom-right (1, 1)
///   bit 4 → cell upper-left  → source top-left     (0, 0)
///   bit 8 → cell upper-right → source top-right    (1, 0)
const QUADRANTS: [(u8, usize, usize); 4] = [(1, 0, 1), (2, 1, 1), (4, 0, 0), (8, 1, 0)];

/// Copies one `half`×`half` quadrant of `src` (top-left at `src_xy`) into `dst`
/// (top-left at `dst_xy`). Both buffers are row-major RGBA8, `w` pixels wide.
fn copy_quadrant(
    src: &[u8],
    dst: &mut [u8],
    w: usize,
    half: usize,
    src_xy: (usize, usize),
    dst_xy: (usize, usize),
) {
    let span = half * 4;
    for yy in 0..half {
        let s = ((src_xy.1 + yy) * w + src_xy.0) * 4;
        let d = ((dst_xy.1 + yy) * w + dst_xy.0) * 4;
        dst[d..d + span].copy_from_slice(&src[s..s + span]);
    }
}

/// Copies a whole `t`×`t` tile from `src` to `dst` at the same position.
fn copy_tile(src: &[u8], dst: &mut [u8], w: usize, t: usize, x0: usize, y0: usize) {
    let span = t * 4;
    for yy in 0..t {
        let i = ((y0 + yy) * w + x0) * 4;
        dst[i..i + span].copy_from_slice(&src[i..i + span]);
    }
}

/// True if every pixel of the `t`×`t` tile at `(x0, y0)` is fully transparent.
fn tile_is_empty(data: &[u8], w: usize, x0: usize, y0: usize, t: usize) -> bool {
    for yy in 0..t {
        for xx in 0..t {
            if data[((y0 + yy) * w + (x0 + xx)) * 4 + 3] != 0 {
                return false;
            }
        }
    }
    true
}

/// Applies the [`FloorFlavor::Flooring`] treatment to a base atlas buffer
/// (row-major RGBA8, 4 bytes per pixel). Returns a new buffer of identical
/// length.
///
/// Goal: each rendered floor tile is a **solid square** of the floor's interior
/// texture that fills its grid cell exactly, so it meets the wall footprints
/// (which occupy a full tile each — see `scripts/gen_cave_wall_sprite.py`) flush
/// with no overhang and no gap.
///
/// For every authoring tile, each quadrant its autotile mask marks as floor is
/// filled from the variant block's **solid interior tile** (mask `0xF`), and
/// every unset quadrant is cleared to transparent. Because the dual-grid draws
/// each corner cell **point-reflected** (the cell center lands on a tile
/// corner), the source is the interior tile's *opposite* quadrant — that lands
/// right-side-up once the four cells assemble into a tile. A block whose
/// interior tile is empty is left verbatim (never blanked). Identity copy on any
/// unexpected atlas layout.
pub fn apply_flooring(data: &[u8], width: u32, height: u32, tile_size_px: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let t = tile_size_px as usize;
    if t == 0 || w == 0 || h == 0 || w % t != 0 || h % t != 0 || data.len() < w * h * 4 {
        return data.to_vec();
    }
    let cols = w / t;
    let rows = h / t;
    // The authoring layout is a 4-wide grid of 4-row variant blocks.
    if cols != 4 || rows % 4 != 0 {
        return data.to_vec();
    }
    let half = t / 2;
    let blocks = rows / 4;
    let mut out = vec![0u8; w * h * 4];

    // Block-local position of the solid interior tile (mask 0xF).
    let interior_j = MASK_TO_AUTHORING_INDEX[0xF];
    let interior_col = interior_j % 4;
    let interior_row_in_block = interior_j / 4;

    for block in 0..blocks {
        let interior_x0 = interior_col * t;
        let interior_y0 = (block * 4 + interior_row_in_block) * t;

        if tile_is_empty(data, w, interior_x0, interior_y0, t) {
            // Degenerate block — preserve the original art verbatim.
            for row_in_block in 0..4 {
                for col in 0..4 {
                    copy_tile(
                        data,
                        &mut out,
                        w,
                        t,
                        col * t,
                        (block * 4 + row_in_block) * t,
                    );
                }
            }
            continue;
        }

        for row_in_block in 0..4 {
            for col in 0..4 {
                let mask = AUTHORING_INDEX_TO_MASK[row_in_block * 4 + col];
                let dst_x0 = col * t;
                let dst_y0 = (block * 4 + row_in_block) * t;
                for &(bit, qx, qy) in &QUADRANTS {
                    if mask & bit == 0 {
                        continue; // unset quadrant → stays transparent (scrap cut)
                    }
                    // Interior tile's OPPOSITE quadrant (point-reflection).
                    copy_quadrant(
                        data,
                        &mut out,
                        w,
                        half,
                        (interior_x0 + (1 - qx) * half, interior_y0 + (1 - qy) * half),
                        (dst_x0 + qx * half, dst_y0 + qy * half),
                    );
                }
            }
        }
    }
    out
}

/// Cross-frame state for [`generate_floor_flavor_atlases`]. `pending` maps a
/// derived floor id to the still-loading base atlas handle it reads from.
#[derive(Default)]
pub struct FlavorGenState {
    initialized: bool,
    pending: HashMap<FloorTypeId, Handle<Image>>,
}

/// Presentation-side system that derives a flavored atlas image for every
/// `base × non-base flavor` and registers it under the derived id in
/// [`FloorTilesetAtlases::images`]. Runs before `build_floor_render_cells`.
///
/// Generation is deferred until each base atlas has CPU-side pixel data in
/// `Assets<Image>` (the default image-load usage keeps a main-world copy). Once
/// an atlas is produced it is `insert`ed (overwriting any base handle a derived
/// cell may have cached early), and `FloorRenderState` is invalidated so those
/// cells rebuild against the processed image.
pub fn generate_floor_flavor_atlases(
    asset_server: Res<AssetServer>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut images: ResMut<Assets<Image>>,
    mut atlases: ResMut<FloorTilesetAtlases>,
    mut generation: ResMut<FloorFlavorGeneration>,
    mut state: Local<FlavorGenState>,
) {
    if !state.initialized {
        state.initialized = true;
        for def in floor_defs.iter() {
            let Some(path) = def.atlas_path.as_ref() else {
                continue;
            };
            for flavor in FloorFlavor::non_base() {
                let derived = derive_floor_id(&def.id, flavor);
                state.pending.insert(derived, asset_server.load(path));
            }
        }
    }
    if state.pending.is_empty() {
        return;
    }

    // Collect the derived ids whose base atlas now has readable pixels.
    let ready: Vec<FloorTypeId> = state
        .pending
        .iter()
        .filter(|(_, handle)| images.get(*handle).and_then(|i| i.data.as_ref()).is_some())
        .map(|(id, _)| id.clone())
        .collect();
    if ready.is_empty() {
        return;
    }

    for derived in ready {
        let handle = state.pending.remove(&derived).expect("just collected");
        let (base_id, flavor) = split_floor_id(&derived);
        let tile_size_px = floor_defs
            .get(base_id)
            .map(|d| d.tile_size_px)
            .unwrap_or(16);

        let (pixels, format, width, height) = {
            let Some(src) = images.get(&handle) else {
                continue;
            };
            let Some(data) = src.data.as_ref() else {
                continue;
            };
            let width = src.width();
            let height = src.height();
            let format = src.texture_descriptor.format;
            let pixels = match flavor {
                FloorFlavor::Flooring => apply_flooring(data, width, height, tile_size_px),
                FloorFlavor::Base => data.clone(),
            };
            (pixels, format, width, height)
        };

        let mut image = Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            format,
            RenderAssetUsages::default(),
        );
        // Crisp pixel art — match the base atlas sampler.
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
        atlases.images.insert(derived, images.add(image));
    }

    // Signal the render build systems to rebuild so any cells already drawn with
    // a base/placeholder handle pick up the freshly generated flavor atlas.
    generation.0 += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::floor_render::MASK_TO_AUTHORING_INDEX;

    fn idx(w: usize, x: usize, y: usize) -> usize {
        (y * w + x) * 4
    }

    fn px(data: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = idx(w, x, y);
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    }

    fn set_px(data: &mut [u8], w: usize, x: usize, y: usize, rgba: [u8; 4]) {
        let i = idx(w, x, y);
        data[i..i + 4].copy_from_slice(&rgba);
    }

    // Distinct per-quadrant interior colours so the opposite-quadrant copy is
    // observable. All opaque so "no transparent pixel" assertions are meaningful.
    const I_TL: [u8; 4] = [11, 0, 0, 255];
    const I_TR: [u8; 4] = [0, 22, 0, 255];
    const I_BL: [u8; 4] = [0, 0, 33, 255];
    const I_BR: [u8; 4] = [44, 44, 0, 255];
    const SCRAP: [u8; 4] = [99, 99, 99, 255];

    /// Builds an 8×8 single-variant-block atlas (tile_size_px = 2): the solid
    /// interior tile (mask 0xF) with four distinct quadrant colours, plus the
    /// `8` (only `(rx,ry)` set) edge tile carrying a stray scrap in an unset
    /// quadrant.
    fn build_atlas() -> (Vec<u8>, u32, u32, u32) {
        let t = 2usize;
        let w = 8usize;
        let h = 8usize;
        let mut data = vec![0u8; w * h * 4];

        // Interior tile: block-local index MASK_TO_AUTHORING_INDEX[0xF] (=6) →
        // col 2, row 1 → pixel origin (4, 2). Each pixel is one quadrant.
        let ix = (MASK_TO_AUTHORING_INDEX[0xF] % 4) * t;
        let iy = (MASK_TO_AUTHORING_INDEX[0xF] / 4) * t;
        set_px(&mut data, w, ix, iy, I_TL); // (4,2)
        set_px(&mut data, w, ix + 1, iy, I_TR); // (5,2)
        set_px(&mut data, w, ix, iy + 1, I_BL); // (4,3)
        set_px(&mut data, w, ix + 1, iy + 1, I_BR); // (5,3)

        // mask=8 edge tile → authoring index 8 → col 0, row 2 → origin (0, 4).
        // Stray scrap in its top-left quadrant (an unset quadrant for mask 8).
        let ex = (MASK_TO_AUTHORING_INDEX[8] % 4) * t;
        let ey = (MASK_TO_AUTHORING_INDEX[8] / 4) * t;
        set_px(&mut data, w, ex, ey, SCRAP); // (0,4)

        (data, w as u32, h as u32, t as u32)
    }

    #[test]
    fn flooring_fills_set_quadrant_from_opposite_interior_and_clears_unset() {
        let (data, w, h, t) = build_atlas();
        let out = apply_flooring(&data, w, h, t);
        let wi = w as usize;

        // mask=8 sets only bit 8 → dest quadrant (1,0) = top-right at (1,4).
        // Source is the interior's OPPOSITE quadrant (0,1) = bottom-left = I_BL.
        assert_eq!(px(&out, wi, 1, 4), I_BL, "set quadrant ← opposite interior");
        // The three unset quadrants (incl. the scrap) are cleared.
        assert_eq!(px(&out, wi, 0, 4), [0, 0, 0, 0], "scrap cut from unset TL");
        assert_eq!(px(&out, wi, 0, 5), [0, 0, 0, 0], "unset BL stays clear");
        assert_eq!(px(&out, wi, 1, 5), [0, 0, 0, 0], "unset BR stays clear");
    }

    #[test]
    fn flooring_point_reflects_solid_interior_tile() {
        // The fully-interior tile (mask 0xF) fills every quadrant from the
        // opposite source, i.e. becomes the interior tile point-reflected. The
        // dual-grid reflects again at render time → a correctly-oriented solid
        // tile (this is what eliminates the "missing middle").
        let (data, w, h, t) = build_atlas();
        let out = apply_flooring(&data, w, h, t);
        let wi = w as usize;
        let ix = (MASK_TO_AUTHORING_INDEX[0xF] % 4) * 2;
        let iy = (MASK_TO_AUTHORING_INDEX[0xF] / 4) * 2;
        assert_eq!(px(&out, wi, ix, iy), I_BR, "TL ← opposite BR");
        assert_eq!(px(&out, wi, ix + 1, iy), I_BL, "TR ← opposite BL");
        assert_eq!(px(&out, wi, ix, iy + 1), I_TR, "BL ← opposite TR");
        assert_eq!(px(&out, wi, ix + 1, iy + 1), I_TL, "BR ← opposite TL");
    }

    #[test]
    fn flooring_solid_tile_is_fully_opaque() {
        // With an opaque interior, every quadrant of a mask-0xF tile is filled,
        // leaving no transparent gap (guards the seam/missing-middle regression).
        let (data, w, h, t) = build_atlas();
        let out = apply_flooring(&data, w, h, t);
        let wi = w as usize;
        let ix = (MASK_TO_AUTHORING_INDEX[0xF] % 4) * 2;
        let iy = (MASK_TO_AUTHORING_INDEX[0xF] / 4) * 2;
        for dy in 0..2 {
            for dx in 0..2 {
                assert_ne!(px(&out, wi, ix + dx, iy + dy)[3], 0, "no transparent gap");
            }
        }
    }

    #[test]
    fn flooring_leaves_block_with_empty_interior_untouched() {
        // If a block's interior tile is empty, the block is copied verbatim so
        // the flavor never blanks art it can't interpret.
        let t = 2usize;
        let w = 8usize;
        let h = 8usize;
        let mut data = vec![0u8; w * h * 4];
        // Put content somewhere but leave the interior tile (4,2)..(5,3) empty.
        set_px(&mut data, w, 0, 0, SCRAP);
        let out = apply_flooring(&data, w as u32, h as u32, t as u32);
        assert_eq!(out, data, "empty-interior block preserved verbatim");
    }

    #[test]
    fn flooring_is_identity_on_unexpected_layout() {
        // 3-wide atlas is not the 4-column authoring layout → returned verbatim.
        let data = vec![7u8; 3 * 4 * 4 * 4];
        let out = apply_flooring(&data, 3 * 4, 4 * 4, 4);
        assert_eq!(out, data);
    }

    #[test]
    fn flooring_preserves_buffer_length() {
        let (data, w, h, t) = build_atlas();
        let out = apply_flooring(&data, w, h, t);
        assert_eq!(out.len(), data.len());
    }
}
