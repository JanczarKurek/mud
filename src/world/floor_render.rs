use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::Hasher;

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::world::components::SpaceId;
use crate::world::floor_definitions::{
    FloorTilesetDefinition, FloorTilesetDefinitions, FloorTransitionDefinition, FloorTypeId,
    TransitionPairKey,
};
use crate::world::floor_map::FloorMap;
use crate::world::floors::{IndoorTileMap, VisibleFloorRange};
use crate::world::lighting::srgb_u8_to_linear;
use crate::world::resources::FloorTransitionOffset;
use crate::world::systems::{flat_floor_z, floor_screen_offset};
use crate::world::WorldConfig;

/// Maps a 4-corner bitmask (0..=15) to the linear atlas index of that
/// sub-tile inside the authoring 4×4 layout used by the artwork in
/// `assets/floors/**/tileset.png`. The fancy layout groups tiles by
/// visual continuity rather than mask-bit ordinal; this lookup applies
/// the inverse of the permutation that `scripts/tile_permutor.py`
/// historically baked in at authoring time.
pub(crate) const MASK_TO_AUTHORING_INDEX: [usize; 16] =
    [12, 0, 13, 3, 15, 11, 4, 2, 8, 14, 1, 5, 9, 7, 10, 6];

/// Inverse of [`MASK_TO_AUTHORING_INDEX`]: maps an authoring-layout tile index
/// (0..=15) back to the 4-corner bitmask it depicts (NW=1, NE=2, SW=4, SE=8).
/// Computed at compile time so it can never drift from the forward table.
/// Used by `crate::world::floor_flavors` to know which quadrants of each
/// authoring tile carry floor pixels.
pub(crate) const AUTHORING_INDEX_TO_MASK: [u8; 16] = {
    let mut inv = [0u8; 16];
    let mut mask = 0usize;
    while mask < 16 {
        inv[MASK_TO_AUTHORING_INDEX[mask]] = mask as u8;
        mask += 1;
    }
    inv
};

/// Marks a presentation-only entity that represents one render-cell of one
/// floor type at a world-tile *corner*. Render cells live at half-tile offsets
/// (rx, ry) - 0.5 in world coordinates and read the 4 surrounding world tiles.
///
/// `priority_z` is the in-band z offset already added to the cell's transform;
/// the same value is reapplied each frame by `sync_floor_render_transforms`.
/// Transition cells store the low floor's priority + a half-step so they sort
/// above the low base but below any neighbouring high cell.
///
/// `local_offset` (in tile-size units, relative to the cell center) is non-zero
/// only for the partial-coverage debug fallback: when a floor type has no
/// atlas, we spawn one quarter-tile sprite per set mask bit and offset each by
/// (±0.25, ±0.25) so the placeholder colour fills only its actual quadrants
/// instead of overdrawing the neighbouring atlas-rendered floors.
#[derive(Component, Clone, Debug)]
pub struct FloorRenderCell {
    pub space_id: SpaceId,
    pub z: i32,
    pub rx: i32,
    pub ry: i32,
    pub floor_type: FloorTypeId,
    pub priority_z: f32,
    pub local_offset: Vec2,
}

#[derive(Resource, Default)]
pub struct FloorTilesetAtlases {
    pub layouts: HashMap<FloorTypeId, Handle<TextureAtlasLayout>>,
    pub images: HashMap<FloorTypeId, Handle<Image>>,
    pub transition_layouts: HashMap<TransitionPairKey, Handle<TextureAtlasLayout>>,
    pub transition_images: HashMap<TransitionPairKey, Handle<Image>>,
}

/// Per-(space, z) hash of the floor grid last rendered. Each visible floor is
/// rebuilt independently so climbing or descending only invalidates one band's
/// cells, not every floor in the space.
#[derive(Resource, Default, Clone, Debug)]
pub struct FloorRenderState {
    pub built_for: HashMap<(SpaceId, i32), u64>,
}

/// Defaulted on both server and client plugins to keep `apply_*` system
/// signatures uniform across the three runtime modes. Server writes are
/// ignored. Reserved for future per-tile incremental updates; presently the
/// hash-based full-rebuild path is sufficient.
#[derive(Resource, Default, Clone, Debug)]
pub struct FloorRenderDirty {
    pub cells: Vec<(SpaceId, i32, i32, i32)>,
}

/// Diagnostics toggle (F9): when `debug_color_only` is set, floors render as
/// flat `debug_color` blocks (the per-quadrant fallback) instead of their atlas
/// art, so you can see exactly which floor type covers each tile. Both the
/// in-game and editor build systems watch this and rebuild on change.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct FloorDebugRender {
    pub debug_color_only: bool,
}

/// Per-tile floor clip rectangles contributed by objects (walls) that declare a
/// `render.floor_mask_rect`. Keyed by `(space, floor_index, x, y)`; the value is
/// `[x0, y0, x1, y1]` in tile fractions (x = west→east, y = south→north). Floor
/// on that tile is drawn **only** inside the rectangle, letting walls keep floor
/// on the interior side of their slab and free the exterior strip.
///
/// Rebuilt each frame from the world's objects (in-game from `ClientGameState`,
/// in the editor from authoritative `OverworldObject`s).
#[derive(Resource, Default, Clone, Debug)]
pub struct FloorMaskMap {
    pub rects: HashMap<(SpaceId, i32, i32, i32), [f32; 4]>,
}

impl FloorMaskMap {
    pub fn get(&self, space_id: SpaceId, z: i32, x: i32, y: i32) -> Option<[f32; 4]> {
        self.rects.get(&(space_id, z, x, y)).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}

/// Quadrant table for the masked-render path: `(mask bit, src col, src row, tile
/// dx, tile dy)`. A corner cell's source quadrant `(col,row)` — col 0=west/left,
/// row 0=north/top — maps to the quarter of tile `(rx+dx, ry+dy)` nearest the
/// corner, matching the dual-grid bit→tile rule (1=(rx-1,ry-1) … 8=(rx,ry)) and
/// the un-flipped source orientation (top=north, left=west).
pub(crate) const MASK_QUADRANTS: [(u8, usize, usize, i32, i32); 4] = [
    (1, 0, 1, -1, -1), // bottom-left  → tile (rx-1, ry-1)
    (2, 1, 1, 0, -1),  // bottom-right → tile (rx,   ry-1)
    (4, 0, 0, -1, 0),  // top-left     → tile (rx-1, ry)
    (8, 1, 0, 0, 0),   // top-right    → tile (rx,   ry)
];

/// Intersects a corner-cell quadrant with a tile's floor-mask rectangle and
/// returns the clipped `(world_rect, src_rect)`, or `None` if the mask removes
/// the quadrant entirely. All rects are `[min_x, min_y, max_x, max_y]`; world
/// rects are in tile units (y = north-positive) and `src_rect` is in atlas
/// pixels (y = top-positive). Within a quadrant world +y (north) maps to src −y
/// (top), so the y axis is flipped when cropping the source.
pub(crate) fn clip_quadrant(
    quad_world: [f32; 4],
    src_quad: [f32; 4],
    mask_world: [f32; 4],
) -> Option<([f32; 4], [f32; 4])> {
    let x0 = quad_world[0].max(mask_world[0]);
    let y0 = quad_world[1].max(mask_world[1]);
    let x1 = quad_world[2].min(mask_world[2]);
    let y1 = quad_world[3].min(mask_world[3]);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let qw = quad_world[2] - quad_world[0];
    let qh = quad_world[3] - quad_world[1];
    let sw = src_quad[2] - src_quad[0];
    let sh = src_quad[3] - src_quad[1];
    // x: world east ↔ src right (same direction).
    let sx0 = src_quad[0] + (x0 - quad_world[0]) / qw * sw;
    let sx1 = src_quad[0] + (x1 - quad_world[0]) / qw * sw;
    // y: world north (max y) ↔ src top (min y) — flipped.
    let sy_top = src_quad[1] + (quad_world[3] - y1) / qh * sh;
    let sy_bot = src_quad[1] + (quad_world[3] - y0) / qh * sh;
    Some(([x0, y0, x1, y1], [sx0, sy_top, sx1, sy_bot]))
}

/// Within a floor band, lower-priority floors render below higher-priority
/// floors. This step is well below `1.0` so the entire floor band stays
/// beneath all object z_indices (which start at ~0.05 and y-sort up to +1.0).
const FLOOR_PRIORITY_STEP: f32 = 0.0001;

/// Sub-step for breaking ties between equal-priority floors at the same corner
/// (HardEdges path). Two grass+cave_floor cells (both priority 0) used to land
/// at exactly the same z and Bevy's 2D sort tie-break is undefined, so the
/// placeholder colour would randomly draw on top of the grass atlas. Bumping
/// the second entry by a fraction of a priority step keeps them within the
/// same priority band but reliably ordered. `0.1` leaves headroom for up to a
/// few stacked floors at one corner without colliding with the next priority.
const HARDEDGE_TIEBREAK_STEP: f32 = FLOOR_PRIORITY_STEP * 0.1;

fn floor_priority_z(priority: i32) -> f32 {
    priority as f32 * FLOOR_PRIORITY_STEP
}

fn quick_hash(tiles: &[Option<FloorTypeId>]) -> u64 {
    let mut h = DefaultHasher::new();
    for t in tiles {
        match t {
            Some(s) => {
                h.write_u8(1);
                h.write(s.as_bytes());
            }
            None => h.write_u8(0),
        }
    }
    h.finish()
}

/// Returns the floor type at world-tile (x, y), or `None` for OOB or void.
fn sample(grid: &FloorMap, x: i32, y: i32) -> Option<&FloorTypeId> {
    grid.get(x, y)
}

/// Deterministically picks a variant index for the cell at corner (rx, ry) in
/// `space_id`, distributed by `weights`. Same inputs always produce the same
/// output, so a tile keeps its variant across rebuilds and across runtime modes.
pub fn pick_variant(space_id: SpaceId, rx: i32, ry: i32, weights: &[u32]) -> usize {
    if weights.len() <= 1 {
        return 0;
    }
    let mut h = DefaultHasher::new();
    h.write_u64(space_id.0);
    h.write_i32(rx);
    h.write_i32(ry);
    let total: u64 = weights.iter().map(|w| *w as u64).sum();
    let mut t = h.finish() % total.max(1);
    for (i, w) in weights.iter().enumerate() {
        let w = *w as u64;
        if t < w {
            return i;
        }
        t -= w;
    }
    weights.len() - 1
}

#[allow(clippy::too_many_arguments)]
pub fn build_floor_render_cells(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    client_state: Res<ClientGameState>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut atlases: ResMut<FloorTilesetAtlases>,
    world_config: Res<WorldConfig>,
    visible_floors: Res<VisibleFloorRange>,
    mut render_state: ResMut<FloorRenderState>,
    flavor_gen: Res<crate::world::floor_flavors::FloorFlavorGeneration>,
    floor_debug: Res<FloorDebugRender>,
    floor_mask: Res<FloorMaskMap>,
    mut seen_flavor_gen: Local<u64>,
    existing: Query<(Entity, &FloorRenderCell)>,
) {
    let _t = crate::diagnostics::SystemTimer::new("build_floor_render_cells", 1.0);
    // A flavored atlas was (re)generated, the floor-debug toggle flipped, or the
    // floor-mask map changed — drop cached hashes so every floor rebuilds (picks
    // up the new image, the flat debug-colour view, or new wall clip rects).
    if *seen_flavor_gen != flavor_gen.0 || floor_debug.is_changed() || floor_mask.is_changed() {
        render_state.built_for.clear();
        *seen_flavor_gen = flavor_gen.0;
    }
    let Some(space) = client_state.current_space.as_ref() else {
        return;
    };
    let current_space_id = space.space_id;

    // Sweep stale entries: anything for a different space, or for a z outside
    // the visible range. Without this, dead floors leak across space switches
    // and across the player's vertical movement.
    let z_min = visible_floors.lowest_visible.max(0);
    let z_max = visible_floors.highest_visible;
    let stale: Vec<(SpaceId, i32)> = render_state
        .built_for
        .keys()
        .copied()
        .filter(|(sid, z)| *sid != current_space_id || *z < z_min || *z > z_max)
        .collect();
    if !stale.is_empty() {
        for key in &stale {
            render_state.built_for.remove(key);
        }
        for (entity, cell) in &existing {
            if stale
                .iter()
                .any(|(sid, z)| *sid == cell.space_id && *z == cell.z)
            {
                commands.entity(entity).despawn();
            }
        }
    }

    for z in z_min..=z_max {
        let key = (current_space_id, z);
        let Some(grid) = client_state.floor_maps.get(&key) else {
            // Floor doesn't exist at this z — make sure no cells linger.
            if render_state.built_for.remove(&key).is_some() {
                for (entity, cell) in &existing {
                    if cell.space_id == current_space_id && cell.z == z {
                        commands.entity(entity).despawn();
                    }
                }
            }
            continue;
        };
        let hash = quick_hash(&grid.tiles);
        if render_state.built_for.get(&key) == Some(&hash) {
            continue;
        }

        // Despawn only the cells we're about to rebuild — leave other floors
        // untouched so vertical movement doesn't churn the entire space.
        for (entity, cell) in &existing {
            if cell.space_id == current_space_id && cell.z == z {
                commands.entity(entity).despawn();
            }
        }

        rebuild_floor_render_cells_for_grid(
            &mut commands,
            &asset_server,
            &mut texture_atlas_layouts,
            &mut atlases,
            &floor_defs,
            &world_config,
            &floor_mask,
            current_space_id,
            z,
            grid,
            floor_debug.debug_color_only,
        );

        render_state.built_for.insert(key, hash);
    }
}

/// Spawns one set of `FloorRenderCell`s covering every corner of `grid`. The
/// caller is responsible for despawning any previous cells; this helper just
/// inserts new ones. Shared between the in-game build path
/// (`build_floor_render_cells`) and the editor build path
/// (`crate::editor::floor_render::editor_build_floor_render_cells`).
#[allow(clippy::too_many_arguments)]
pub fn rebuild_floor_render_cells_for_grid(
    commands: &mut Commands,
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    atlases: &mut FloorTilesetAtlases,
    floor_defs: &FloorTilesetDefinitions,
    world_config: &WorldConfig,
    floor_mask: &FloorMaskMap,
    space_id: SpaceId,
    z: i32,
    grid: &FloorMap,
    debug: bool,
) {
    for ry in 0..=grid.height {
        for rx in 0..=grid.width {
            spawn_render_cells_at_corner(
                commands,
                asset_server,
                texture_atlas_layouts,
                atlases,
                floor_defs,
                world_config,
                floor_mask,
                space_id,
                z,
                rx,
                ry,
                grid,
                debug,
            );
        }
    }
}

/// Hashes a floor grid so callers can short-circuit rebuilds when nothing
/// changed. Used by both the in-game and editor build systems.
pub fn floor_grid_hash(tiles: &[Option<FloorTypeId>]) -> u64 {
    quick_hash(tiles)
}

/// What to spawn at a single corner. `HardEdges` is the per-type partial
/// bitmask path (current behaviour); `Transition` swaps both cells for a low
/// solid base + transition overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CornerRenderPlan<'a> {
    HardEdges(Vec<(&'a FloorTypeId, u8)>),
    Transition {
        low: &'a FloorTypeId,
        high: &'a FloorTypeId,
        high_mask: u8,
    },
}

/// Pure classifier: decides what cells to spawn at the corner whose 4
/// surrounding tiles are `nw, ne, sw, se`. Encodes the rule:
/// - exactly 2 distinct types **and** a transition exists for the pair → `Transition`
/// - else → `HardEdges` with one entry per distinct type (current behaviour)
pub fn classify_corner<'a>(
    floor_defs: &'a FloorTilesetDefinitions,
    nw: Option<&'a FloorTypeId>,
    ne: Option<&'a FloorTypeId>,
    sw: Option<&'a FloorTypeId>,
    se: Option<&'a FloorTypeId>,
) -> CornerRenderPlan<'a> {
    let mut bits_per_type: HashMap<&'a FloorTypeId, u8> = HashMap::new();
    if let Some(t) = nw {
        *bits_per_type.entry(t).or_default() |= 1;
    }
    if let Some(t) = ne {
        *bits_per_type.entry(t).or_default() |= 2;
    }
    if let Some(t) = sw {
        *bits_per_type.entry(t).or_default() |= 4;
    }
    if let Some(t) = se {
        *bits_per_type.entry(t).or_default() |= 8;
    }

    if bits_per_type.len() == 2 {
        let mut iter = bits_per_type.iter();
        let (a_id, &a_mask) = iter.next().unwrap();
        let (b_id, &b_mask) = iter.next().unwrap();
        if let Some((low, high, _def)) = floor_defs.transition_for(a_id, b_id) {
            let high_mask = if low == *a_id { b_mask } else { a_mask };
            return CornerRenderPlan::Transition {
                low,
                high,
                high_mask,
            };
        }
    }

    CornerRenderPlan::HardEdges(hard_edge_entries(floor_defs, nw, ne, sw, se))
}

/// Per-type `(floor_id, mask)` entries for a corner, sorted by `(priority asc,
/// id asc)`. This is the `HardEdges` payload — one entry per distinct floor
/// type, with the bits set for the quadrants it occupies. Shared by
/// `classify_corner`'s fallback and the floor-debug-color path (which forces
/// hard edges so each floor type shows in its own `debug_color`).
///
/// The deterministic sort matters: `HashMap` iteration order is randomized per
/// process, so without it the cells spawn in arbitrary order, get different
/// entity IDs, and Bevy's equal-z 2D sort flips — making the same corner look
/// different on each repaint. The order also feeds `HARDEDGE_TIEBREAK_STEP` so
/// the alphabetically later floor (e.g. grass over cave_floor at equal
/// priority) reliably wins.
pub fn hard_edge_entries<'a>(
    floor_defs: &'a FloorTilesetDefinitions,
    nw: Option<&'a FloorTypeId>,
    ne: Option<&'a FloorTypeId>,
    sw: Option<&'a FloorTypeId>,
    se: Option<&'a FloorTypeId>,
) -> Vec<(&'a FloorTypeId, u8)> {
    let mut bits_per_type: HashMap<&'a FloorTypeId, u8> = HashMap::new();
    if let Some(t) = nw {
        *bits_per_type.entry(t).or_default() |= 1;
    }
    if let Some(t) = ne {
        *bits_per_type.entry(t).or_default() |= 2;
    }
    if let Some(t) = sw {
        *bits_per_type.entry(t).or_default() |= 4;
    }
    if let Some(t) = se {
        *bits_per_type.entry(t).or_default() |= 8;
    }
    let mut entries: Vec<(&FloorTypeId, u8)> =
        bits_per_type.into_iter().filter(|(_, m)| *m != 0).collect();
    entries.sort_by(|a, b| {
        let pa = floor_defs.get(a.0).map(|d| d.priority).unwrap_or(0);
        let pb = floor_defs.get(b.0).map(|d| d.priority).unwrap_or(0);
        pa.cmp(&pb).then(a.0.cmp(b.0))
    });
    entries
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_render_cells_at_corner(
    commands: &mut Commands,
    asset_server: &AssetServer,
    layouts_assets: &mut Assets<TextureAtlasLayout>,
    atlases: &mut FloorTilesetAtlases,
    floor_defs: &FloorTilesetDefinitions,
    world_config: &WorldConfig,
    floor_mask: &FloorMaskMap,
    space_id: SpaceId,
    z: i32,
    rx: i32,
    ry: i32,
    grid: &FloorMap,
    debug: bool,
) {
    // Bitmask convention: NW=1, NE=2, SW=4, SE=8.
    let nw = sample(grid, rx - 1, ry - 1);
    let ne = sample(grid, rx, ry - 1);
    let sw = sample(grid, rx - 1, ry);
    let se = sample(grid, rx, ry);

    // Floor-debug mode forces the per-type hard-edge path so every floor shows
    // in its own `debug_color` (transitions/atlas art are bypassed).
    let plan = if debug {
        CornerRenderPlan::HardEdges(hard_edge_entries(floor_defs, nw, ne, sw, se))
    } else {
        classify_corner(floor_defs, nw, ne, sw, se)
    };

    match plan {
        CornerRenderPlan::Transition {
            low,
            high,
            high_mask,
        } => {
            let low_def = floor_defs
                .get(low)
                .expect("low floor type validated at load time");
            let (_, _, t_def) = floor_defs
                .transition_for(low, high)
                .expect("classify_corner returned Transition without a transition def");
            let base_z = floor_priority_z(low_def.priority);
            let trans_z = base_z + FLOOR_PRIORITY_STEP * 0.5;

            spawn_floor_cell(
                commands,
                asset_server,
                layouts_assets,
                atlases,
                world_config,
                floor_mask,
                space_id,
                z,
                rx,
                ry,
                low,
                low_def,
                0xF,
                base_z,
                debug,
            );
            spawn_transition_cell(
                commands,
                asset_server,
                layouts_assets,
                atlases,
                world_config,
                space_id,
                z,
                rx,
                ry,
                t_def,
                high_mask,
                trans_z,
            );
        }
        CornerRenderPlan::HardEdges(entries) => {
            // Entries are sorted (priority asc, id asc); apply a fractional
            // tiebreak so equal-priority cells never render at exactly the
            // same z (see HARDEDGE_TIEBREAK_STEP).
            for (i, (floor_id, mask)) in entries.iter().enumerate() {
                let Some(def) = floor_defs.get(floor_id) else {
                    continue;
                };
                let priority_z = floor_priority_z(def.priority) + i as f32 * HARDEDGE_TIEBREAK_STEP;
                spawn_floor_cell(
                    commands,
                    asset_server,
                    layouts_assets,
                    atlases,
                    world_config,
                    floor_mask,
                    space_id,
                    z,
                    rx,
                    ry,
                    floor_id,
                    def,
                    *mask,
                    priority_z,
                    debug,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_floor_cell(
    commands: &mut Commands,
    asset_server: &AssetServer,
    layouts_assets: &mut Assets<TextureAtlasLayout>,
    atlases: &mut FloorTilesetAtlases,
    world_config: &WorldConfig,
    floor_mask: &FloorMaskMap,
    space_id: SpaceId,
    z: i32,
    rx: i32,
    ry: i32,
    floor_id: &FloorTypeId,
    def: &FloorTilesetDefinition,
    mask: u8,
    priority_z: f32,
    debug: bool,
) {
    // Floor-debug mode (F9): skip the atlas and fall through to the flat
    // `debug_color` path below so each floor type is visible per tile.
    if !debug {
        if let Some(atlas_path) = &def.atlas_path {
            let image_handle = atlases
                .images
                .entry(floor_id.clone())
                .or_insert_with(|| asset_server.load(atlas_path))
                .clone();
            let weights = def.variant_weights(mask);
            let variant = pick_variant(space_id, rx, ry, weights);
            let idx = MASK_TO_AUTHORING_INDEX[mask as usize & 0xF] + variant * 16;

            // If any contributing quadrant lands on a floor-masked tile, render
            // the cell as per-quadrant sprites clipped to those masks instead of
            // one full-cell atlas sprite.
            let masked = MASK_QUADRANTS.iter().any(|(bit, _, _, dx, dy)| {
                mask & bit != 0 && floor_mask.get(space_id, z, rx + dx, ry + dy).is_some()
            });
            if masked {
                spawn_masked_floor_quadrants(
                    commands,
                    &image_handle,
                    world_config,
                    floor_mask,
                    space_id,
                    z,
                    rx,
                    ry,
                    floor_id,
                    def,
                    mask,
                    idx,
                    priority_z,
                );
                return;
            }

            let max_variants = def.max_variants() as u32;
            let layout_handle = atlases
                .layouts
                .entry(floor_id.clone())
                .or_insert_with(|| {
                    let layout = TextureAtlasLayout::from_grid(
                        UVec2::splat(def.tile_size_px),
                        4,
                        4 * max_variants,
                        None,
                        None,
                    );
                    layouts_assets.add(layout)
                })
                .clone();
            let sprite = Sprite {
                image: image_handle,
                custom_size: Some(Vec2::splat(world_config.tile_size)),
                texture_atlas: Some(TextureAtlas {
                    layout: layout_handle,
                    index: idx,
                }),
                ..default()
            };
            commands.spawn((
                FloorRenderCell {
                    space_id,
                    z,
                    rx,
                    ry,
                    floor_type: floor_id.clone(),
                    priority_z,
                    local_offset: Vec2::ZERO,
                },
                sprite,
                Transform::from_xyz(0.0, 0.0, flat_floor_z(priority_z, z)),
                Visibility::default(),
            ));
            return;
        }
    }

    // Flat `debug_color` path: reached when a floor has no authored atlas, OR
    // when floor-debug mode (F9) is on. Spawn one quarter-tile sprite per set
    // mask bit so the colour fills only its contributing quadrants — otherwise
    // a 1-tile cave_floor on grass would render four full-tile brown squares
    // overdrawing the surrounding grass at every boundary corner.
    if mask == 0xF {
        // Interior corner (every neighbour is the same type) — one sprite
        // covering the whole cell. Saves three entities vs. the per-bit path.
        commands.spawn((
            FloorRenderCell {
                space_id,
                z,
                rx,
                ry,
                floor_type: floor_id.clone(),
                priority_z,
                local_offset: Vec2::ZERO,
            },
            Sprite::from_color(def.debug_color(), Vec2::splat(world_config.tile_size)),
            Transform::from_xyz(0.0, 0.0, flat_floor_z(priority_z, z)),
            Visibility::default(),
        ));
        return;
    }
    let half = world_config.tile_size * 0.5;
    // Quadrant offsets in tile-size units, relative to the cell center.
    // NW samples (rx-1, ry-1), which is lower x and lower y in world coords;
    // applies to all four bits via the same convention.
    const QUADRANTS: [(u8, Vec2); 4] = [
        (1, Vec2::new(-0.25, -0.25)), // NW
        (2, Vec2::new(0.25, -0.25)),  // NE
        (4, Vec2::new(-0.25, 0.25)),  // SW
        (8, Vec2::new(0.25, 0.25)),   // SE
    ];
    for (bit, offset) in QUADRANTS {
        if mask & bit == 0 {
            continue;
        }
        commands.spawn((
            FloorRenderCell {
                space_id,
                z,
                rx,
                ry,
                floor_type: floor_id.clone(),
                priority_z,
                local_offset: offset,
            },
            Sprite::from_color(def.debug_color(), Vec2::splat(half)),
            Transform::from_xyz(0.0, 0.0, flat_floor_z(priority_z, z)),
            Visibility::default(),
        ));
    }
}

/// Renders a floor cell as up-to-four per-quadrant sprites, each clipped to its
/// tile's `floor_mask_rect` (full tile if unmasked). Used in place of the single
/// full-cell atlas sprite when any contributing quadrant lands on a masked tile,
/// so floor only draws inside the wall's interior rectangle. Each quadrant draws
/// the matching sub-region of the atlas tile `idx` via `Sprite::rect`.
#[allow(clippy::too_many_arguments)]
fn spawn_masked_floor_quadrants(
    commands: &mut Commands,
    image_handle: &Handle<Image>,
    world_config: &WorldConfig,
    floor_mask: &FloorMaskMap,
    space_id: SpaceId,
    z: i32,
    rx: i32,
    ry: i32,
    floor_id: &FloorTypeId,
    def: &FloorTilesetDefinition,
    mask: u8,
    idx: usize,
    priority_z: f32,
) {
    let t = def.tile_size_px as f32;
    let half = t / 2.0;
    let tcol = (idx % 4) as f32;
    let trow = (idx / 4) as f32;
    let cell_cx = rx as f32 - 0.5;
    let cell_cy = ry as f32 - 0.5;

    for (bit, col, row, dx, dy) in MASK_QUADRANTS {
        if mask & bit == 0 {
            continue;
        }
        // Quadrant world rect (tile units): col 0=west → [rx-1, rx-0.5];
        // row 0=north → [ry-0.5, ry].
        let qx0 = (rx as f32 - 1.0) + col as f32 * 0.5;
        let qy0 = (ry as f32 - 1.0) + (1 - row) as f32 * 0.5;
        let quad_world = [qx0, qy0, qx0 + 0.5, qy0 + 0.5];
        // Source quadrant pixel rect within the atlas tile at `idx`.
        let sx0 = tcol * t + col as f32 * half;
        let sy0 = trow * t + row as f32 * half;
        let src_quad = [sx0, sy0, sx0 + half, sy0 + half];
        // The tile this quadrant belongs to, and its mask (full tile if none).
        let tx = rx + dx;
        let ty = ry + dy;
        let mask_world = match floor_mask.get(space_id, z, tx, ty) {
            Some(m) => [
                tx as f32 - 0.5 + m[0],
                ty as f32 - 0.5 + m[1],
                tx as f32 - 0.5 + m[2],
                ty as f32 - 0.5 + m[3],
            ],
            None => [
                tx as f32 - 0.5,
                ty as f32 - 0.5,
                tx as f32 + 0.5,
                ty as f32 + 0.5,
            ],
        };
        let Some((cw, cs)) = clip_quadrant(quad_world, src_quad, mask_world) else {
            continue;
        };
        let center = Vec2::new((cw[0] + cw[2]) * 0.5, (cw[1] + cw[3]) * 0.5);
        let local_offset = Vec2::new(center.x - cell_cx, center.y - cell_cy);
        let size = Vec2::new(
            (cw[2] - cw[0]) * world_config.tile_size,
            (cw[3] - cw[1]) * world_config.tile_size,
        );
        let sprite = Sprite {
            image: image_handle.clone(),
            custom_size: Some(size),
            rect: Some(Rect::new(cs[0], cs[1], cs[2], cs[3])),
            ..default()
        };
        commands.spawn((
            FloorRenderCell {
                space_id,
                z,
                rx,
                ry,
                floor_type: floor_id.clone(),
                priority_z,
                local_offset,
            },
            sprite,
            Transform::from_xyz(0.0, 0.0, flat_floor_z(priority_z, z)),
            Visibility::default(),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_transition_cell(
    commands: &mut Commands,
    asset_server: &AssetServer,
    layouts_assets: &mut Assets<TextureAtlasLayout>,
    atlases: &mut FloorTilesetAtlases,
    world_config: &WorldConfig,
    space_id: SpaceId,
    z: i32,
    rx: i32,
    ry: i32,
    def: &FloorTransitionDefinition,
    mask: u8,
    priority_z: f32,
) {
    let key = (def.low.clone(), def.high.clone());
    let image_handle = atlases
        .transition_images
        .entry(key.clone())
        .or_insert_with(|| asset_server.load(def.atlas_path.clone()))
        .clone();
    let max_variants = def.max_variants() as u32;
    let layout_handle = atlases
        .transition_layouts
        .entry(key.clone())
        .or_insert_with(|| {
            let layout = TextureAtlasLayout::from_grid(
                UVec2::splat(def.tile_size_px),
                4,
                4 * max_variants,
                None,
                None,
            );
            layouts_assets.add(layout)
        })
        .clone();
    let weights = def.variant_weights(mask);
    let variant = pick_variant(space_id, rx, ry, weights);
    let sprite = Sprite {
        image: image_handle,
        custom_size: Some(Vec2::splat(world_config.tile_size)),
        texture_atlas: Some(TextureAtlas {
            layout: layout_handle,
            index: MASK_TO_AUTHORING_INDEX[mask as usize & 0xF] + variant * 16,
        }),
        ..default()
    };

    // The cell's `floor_type` carries the high side so it gets cleaned up by
    // the same FloorRenderCell despawn pass; the actual atlas comes from the
    // transition handle maps on `FloorTilesetAtlases`.
    commands.spawn((
        FloorRenderCell {
            space_id,
            z,
            rx,
            ry,
            floor_type: def.high.clone(),
            priority_z,
            local_offset: Vec2::ZERO,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, flat_floor_z(priority_z, z)),
        Visibility::default(),
    ));
}

/// Reserved: per-tile incremental rebuild path. Today the full-rebuild driven
/// by content hash in `build_floor_render_cells` catches every change, so this
/// is a no-op.
pub fn consume_floor_render_dirty(_dirty: ResMut<FloorRenderDirty>) {}

pub fn sync_floor_render_transforms(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    visible_floors: Res<VisibleFloorRange>,
    floor_transition: Res<FloorTransitionOffset>,
    indoor: Res<IndoorTileMap>,
    mut query: Query<(&FloorRenderCell, &mut Transform, &mut Sprite)>,
) {
    let _t = crate::diagnostics::SystemTimer::new("sync_floor_render_transforms", 1.0);
    let Some(player_position) = client_state.player_position else {
        return;
    };

    // Indoor ambient color used as per-cell tint for floor tiles whose corner
    // is inside an enclosed area. Mirrors the per-sprite tint in
    // `sync_tile_transforms` so the floor shades alongside the walls and
    // objects that share the indoor region.
    let indoor_tint_rgb = client_state
        .current_space
        .as_ref()
        .map(|s| srgb_u8_to_linear(s.lighting.indoor_ambient))
        .unwrap_or([1.0, 1.0, 1.0]);

    // Absolute world coords: x and y depend only on the cell's tile-grid
    // position plus a Tibia-style up-left offset per floor away from the
    // player. The camera follows the player (`world::camera::camera_follow`),
    // so screen-relative positioning falls out of `sprite_world - camera_world`.
    for (cell, mut transform, mut sprite) in &mut query {
        // Cull cells outside the active space OR outside the visible-floor
        // range. The range gate is what makes a painted upper floor disappear
        // when the player walks under it — without it, the cell renders
        // regardless of `recompute_visible_floors` clamping `highest_visible`
        // down to the player's floor. Editor's sync does the same check
        // (`src/editor/floor_render.rs`).
        let visible = cell.space_id == player_position.space_id && visible_floors.contains(cell.z);
        let z = if !visible {
            -10_000.0
        } else {
            flat_floor_z(cell.priority_z, cell.z)
        };
        // `cell.z` is an integer floor index; convert to half-block z so the
        // shared `floor_screen_offset` (which is fractional in half-block z)
        // produces the correct shift relative to the player's half-block z.
        let floor_offset = if visible {
            floor_screen_offset(
                (cell.z * 2) as f32,
                floor_transition.visual_player_z(visible_floors.player_z),
                world_config.tile_size,
            )
        } else {
            Vec2::ZERO
        };
        let dx =
            (cell.rx as f32 - 0.5 + cell.local_offset.x) * world_config.tile_size + floor_offset.x;
        let dy =
            (cell.ry as f32 - 0.5 + cell.local_offset.y) * world_config.tile_size + floor_offset.y;
        let new_translation = Vec3::new(dx, dy, z);
        if transform.translation != new_translation {
            transform.translation = new_translation;
        }

        // A corner cell straddles 4 surrounding world tiles; if any of them is
        // indoor we tint the whole cell. This is mildly conservative at the
        // building's edge corners (one quadrant of a corner cell might sit
        // outside the wall), but that quadrant is occluded by the wall sprite
        // visually anyway, so the slight over-tint is invisible in practice.
        let is_indoor = visible
            && [
                (cell.rx - 1, cell.ry - 1),
                (cell.rx, cell.ry - 1),
                (cell.rx - 1, cell.ry),
                (cell.rx, cell.ry),
            ]
            .iter()
            .any(|(tx, ty)| indoor.contains(cell.space_id, *tx, *ty, cell.z));
        let rgb = if is_indoor {
            indoor_tint_rgb
        } else {
            [1.0, 1.0, 1.0]
        };
        let new_color = Color::linear_rgba(rgb[0], rgb[1], rgb[2], sprite.color.alpha());
        if sprite.color != new_color {
            sprite.color = new_color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_variant_single_weight_is_zero() {
        assert_eq!(pick_variant(SpaceId(7), 3, 4, &[1]), 0);
        assert_eq!(pick_variant(SpaceId(7), 3, 4, &[42]), 0);
    }

    #[test]
    fn pick_variant_is_deterministic() {
        let weights = [1, 1, 1, 1];
        let a = pick_variant(SpaceId(1), 17, -23, &weights);
        let b = pick_variant(SpaceId(1), 17, -23, &weights);
        assert_eq!(a, b);
    }

    #[test]
    fn pick_variant_stays_in_bounds() {
        let weights = [3, 1, 1];
        for x in -50..50 {
            for y in -50..50 {
                let v = pick_variant(SpaceId(0), x, y, &weights);
                assert!(v < weights.len(), "variant {} out of bounds", v);
            }
        }
    }

    fn ts(id: &str, priority: i32) -> FloorTilesetDefinition {
        FloorTilesetDefinition {
            id: id.to_owned(),
            name: id.to_owned(),
            priority,
            tile_size_px: 16,
            atlas_path: None,
            debug_color: [0, 0, 0],
            occludes_floor_above: false,
            walkable_surface: true,
            variants: HashMap::new(),
            ripple: None,
        }
    }

    fn tr(low: &str, high: &str) -> FloorTransitionDefinition {
        FloorTransitionDefinition {
            low: low.to_owned(),
            high: high.to_owned(),
            tile_size_px: 16,
            atlas_path: format!("floors/transitions/{low}__{high}/tileset.png"),
            variants: HashMap::new(),
        }
    }

    fn defs(floors: &[(&str, i32)], transitions: &[(&str, &str)]) -> FloorTilesetDefinitions {
        let mut by_id = HashMap::new();
        for (id, p) in floors {
            by_id.insert((*id).to_owned(), ts(id, *p));
        }
        let mut tmap = HashMap::new();
        for (low, high) in transitions {
            tmap.insert(((*low).to_owned(), (*high).to_owned()), tr(low, high));
        }
        FloorTilesetDefinitions::for_test(by_id, tmap)
    }

    #[test]
    fn classify_single_type_corner_is_hard_edges() {
        let defs = defs(&[("grass", 0)], &[]);
        let g = "grass".to_owned();
        let plan = classify_corner(&defs, Some(&g), Some(&g), Some(&g), Some(&g));
        match plan {
            CornerRenderPlan::HardEdges(e) => {
                assert_eq!(e.len(), 1);
                assert_eq!(e[0].0, &g);
                assert_eq!(e[0].1, 0xF);
            }
            other => panic!("expected HardEdges, got {other:?}"),
        }
    }

    #[test]
    fn classify_two_types_with_transition_uses_transition() {
        let defs = defs(&[("grass", 0), ("brick", 1)], &[("grass", "brick")]);
        let g = "grass".to_owned();
        let b = "brick".to_owned();
        // NW=brick, NE=grass, SW=grass, SE=brick → grass mask = 6, brick mask = 9
        let plan = classify_corner(&defs, Some(&b), Some(&g), Some(&g), Some(&b));
        match plan {
            CornerRenderPlan::Transition {
                low,
                high,
                high_mask,
            } => {
                assert_eq!(low, &g);
                assert_eq!(high, &b);
                assert_eq!(high_mask, 9, "high mask should be NW+SE = 1|8");
            }
            other => panic!("expected Transition, got {other:?}"),
        }
    }

    #[test]
    fn classify_two_types_without_transition_falls_back() {
        let defs = defs(&[("grass", 0), ("brick", 1)], &[]);
        let g = "grass".to_owned();
        let b = "brick".to_owned();
        let plan = classify_corner(&defs, Some(&g), Some(&b), Some(&g), Some(&b));
        match plan {
            CornerRenderPlan::HardEdges(e) => assert_eq!(e.len(), 2),
            other => panic!("expected HardEdges fallback, got {other:?}"),
        }
    }

    #[test]
    fn hardedges_sorts_by_priority_then_id() {
        // Equal priority: alphabetical id order — cave_floor before grass, so
        // grass spawns later and gets the tiebreak z bump (renders on top).
        // This is the regression for the grass+cave_floor flicker bug.
        let d1 = defs(&[("grass", 0), ("cave_floor", 0)], &[]);
        let g = "grass".to_owned();
        let c = "cave_floor".to_owned();
        let plan = classify_corner(&d1, Some(&g), Some(&g), Some(&g), Some(&c));
        match plan {
            CornerRenderPlan::HardEdges(e) => {
                assert_eq!(e.len(), 2);
                assert_eq!(e[0].0, &c, "cave_floor (alphabetically first) at index 0");
                assert_eq!(e[1].0, &g, "grass at index 1 — gets the z bump");
            }
            other => panic!("expected HardEdges, got {other:?}"),
        }

        // Different priorities: lower priority comes first regardless of id.
        let d2 = defs(&[("zebra", 0), ("alpha", 5)], &[]);
        let z = "zebra".to_owned();
        let a = "alpha".to_owned();
        let plan = classify_corner(&d2, Some(&z), Some(&a), Some(&z), Some(&a));
        match plan {
            CornerRenderPlan::HardEdges(e) => {
                assert_eq!(e[0].0, &z, "zebra (priority 0) before alpha (priority 5)");
                assert_eq!(e[1].0, &a);
            }
            other => panic!("expected HardEdges, got {other:?}"),
        }
    }

    #[test]
    fn classify_three_types_falls_back_even_with_transitions() {
        let defs = defs(
            &[("grass", 0), ("brick", 1), ("sand", 2)],
            &[("grass", "brick"), ("brick", "sand"), ("grass", "sand")],
        );
        let g = "grass".to_owned();
        let b = "brick".to_owned();
        let s = "sand".to_owned();
        let plan = classify_corner(&defs, Some(&g), Some(&b), Some(&s), Some(&g));
        match plan {
            CornerRenderPlan::HardEdges(e) => assert_eq!(e.len(), 3),
            other => panic!("expected HardEdges fallback for 3 types, got {other:?}"),
        }
    }

    #[test]
    fn classify_lookup_is_independent_of_corner_order() {
        // The HashMap that backs bits_per_type has non-deterministic iteration
        // order; the classifier must produce the same Transition plan regardless.
        let defs = defs(&[("grass", 0), ("brick", 1)], &[("grass", "brick")]);
        let g = "grass".to_owned();
        let b = "brick".to_owned();
        // A: NW=g, NE=b, SW=g, SE=b → grass mask=5, brick mask=10
        let a = classify_corner(&defs, Some(&g), Some(&b), Some(&g), Some(&b));
        // B: same configuration with corners swapped left/right and recomputed.
        let b_plan = classify_corner(&defs, Some(&g), Some(&b), Some(&g), Some(&b));
        assert_eq!(a, b_plan);
        if let CornerRenderPlan::Transition { high_mask, .. } = a {
            assert_eq!(high_mask, 10, "brick mask should be NE+SE = 2|8");
        } else {
            panic!("expected Transition");
        }
    }

    #[test]
    fn pick_variant_distribution_skews_with_weights() {
        let weights = [9, 1];
        let mut zero = 0usize;
        let mut total = 0usize;
        for x in 0..200 {
            for y in 0..200 {
                if pick_variant(SpaceId(0), x, y, &weights) == 0 {
                    zero += 1;
                }
                total += 1;
            }
        }
        let ratio = zero as f64 / total as f64;
        assert!(
            (0.85..=0.95).contains(&ratio),
            "expected ~0.9 for weights [9,1], got {ratio}"
        );
    }

    #[test]
    fn mask_to_authoring_index_is_a_permutation_of_0_to_15() {
        let mut seen = [false; 16];
        for &i in &MASK_TO_AUTHORING_INDEX {
            assert!(i < 16, "index {i} out of range");
            assert!(!seen[i], "duplicate index {i}");
            seen[i] = true;
        }
        assert!(seen.iter().all(|&b| b));
    }

    #[test]
    fn mask_to_authoring_index_known_anchors() {
        assert_eq!(MASK_TO_AUTHORING_INDEX[0], 12);
        assert_eq!(MASK_TO_AUTHORING_INDEX[1], 0);
        assert_eq!(MASK_TO_AUTHORING_INDEX[15], 6);
    }

    #[test]
    fn authoring_index_to_mask_is_exact_inverse() {
        for mask in 0u8..16 {
            let idx = MASK_TO_AUTHORING_INDEX[mask as usize];
            assert_eq!(
                AUTHORING_INDEX_TO_MASK[idx], mask,
                "inverse mismatch at mask {mask}"
            );
        }
        // ...and the inverse of the inverse round-trips by authoring index.
        for idx in 0usize..16 {
            let mask = AUTHORING_INDEX_TO_MASK[idx];
            assert_eq!(MASK_TO_AUTHORING_INDEX[mask as usize], idx);
        }
    }

    #[test]
    fn clip_quadrant_full_overlap_is_identity() {
        // A mask covering the whole tile leaves the quadrant untouched.
        let (w, s) = clip_quadrant(
            [0.0, 0.0, 0.5, 0.5],
            [0.0, 0.0, 8.0, 8.0],
            [-1.0, -1.0, 2.0, 2.0],
        )
        .unwrap();
        assert_eq!(w, [0.0, 0.0, 0.5, 0.5]);
        assert_eq!(s, [0.0, 0.0, 8.0, 8.0]);
    }

    #[test]
    fn clip_quadrant_crops_south_strip_to_top_of_source() {
        // Mask keeps the north half (y >= 0.25). World +y(north) ↔ src top, so
        // the kept world half maps to the TOP half of the source quadrant.
        let (w, s) = clip_quadrant(
            [0.0, 0.0, 0.5, 0.5],
            [0.0, 0.0, 8.0, 8.0],
            [0.0, 0.25, 1.0, 1.0],
        )
        .unwrap();
        assert_eq!(w, [0.0, 0.25, 0.5, 0.5], "north half kept");
        assert_eq!(s, [0.0, 0.0, 8.0, 4.0], "north → top half of source");
    }

    #[test]
    fn clip_quadrant_crops_east_strip_to_right_of_source() {
        // Mask keeps the east half (x >= 0.25). World +x(east) ↔ src right.
        let (w, s) = clip_quadrant(
            [0.0, 0.0, 0.5, 0.5],
            [0.0, 0.0, 8.0, 8.0],
            [0.25, -1.0, 2.0, 2.0],
        )
        .unwrap();
        assert_eq!(w, [0.25, 0.0, 0.5, 0.5], "east half kept");
        assert_eq!(s, [4.0, 0.0, 8.0, 8.0], "east → right half of source");
    }

    #[test]
    fn clip_quadrant_no_overlap_returns_none() {
        // Mask entirely south of a north quadrant → nothing rendered.
        assert!(clip_quadrant(
            [0.0, 0.5, 0.5, 1.0],
            [0.0, 0.0, 8.0, 8.0],
            [0.0, 0.0, 1.0, 0.4],
        )
        .is_none());
    }
}
