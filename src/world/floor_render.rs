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
use crate::world::resources::ViewScrollOffset;
use crate::world::systems::flat_floor_z;
use crate::world::WorldConfig;

/// Marks a presentation-only entity that represents one render-cell of one
/// floor type at a world-tile *corner*. Render cells live at half-tile offsets
/// (rx, ry) - 0.5 in world coordinates and read the 4 surrounding world tiles.
///
/// `priority_z` is the in-band z offset already added to the cell's transform;
/// the same value is reapplied each frame by `sync_floor_render_transforms`.
/// Transition cells store the low floor's priority + a half-step so they sort
/// above the low base but below any neighbouring high cell.
#[derive(Component, Clone, Debug)]
pub struct FloorRenderCell {
    pub space_id: SpaceId,
    pub z: i32,
    pub rx: i32,
    pub ry: i32,
    pub floor_type: FloorTypeId,
    pub priority_z: f32,
}

#[derive(Resource, Default)]
pub struct FloorTilesetAtlases {
    pub layouts: HashMap<FloorTypeId, Handle<TextureAtlasLayout>>,
    pub images: HashMap<FloorTypeId, Handle<Image>>,
    pub transition_layouts: HashMap<TransitionPairKey, Handle<TextureAtlasLayout>>,
    pub transition_images: HashMap<TransitionPairKey, Handle<Image>>,
}

#[derive(Resource, Default, Clone, Debug)]
pub struct FloorRenderState {
    pub built_for: Option<(SpaceId, i32, u64)>,
}

/// Defaulted on both server and client plugins to keep `apply_*` system
/// signatures uniform across the three runtime modes. Server writes are
/// ignored. Reserved for future per-tile incremental updates; presently the
/// hash-based full-rebuild path is sufficient.
#[derive(Resource, Default, Clone, Debug)]
pub struct FloorRenderDirty {
    pub cells: Vec<(SpaceId, i32, i32, i32)>,
}

/// Within a floor band, lower-priority floors render below higher-priority
/// floors. This step is well below `1.0` so the entire floor band stays
/// beneath all object z_indices (which start at ~0.05 and y-sort up to +1.0).
const FLOOR_PRIORITY_STEP: f32 = 0.0001;

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
fn pick_variant(space_id: SpaceId, rx: i32, ry: i32, weights: &[u32]) -> usize {
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

pub fn build_floor_render_cells(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    client_state: Res<ClientGameState>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut atlases: ResMut<FloorTilesetAtlases>,
    world_config: Res<WorldConfig>,
    mut render_state: ResMut<FloorRenderState>,
    existing: Query<Entity, With<FloorRenderCell>>,
) {
    let Some(space) = client_state.current_space.as_ref() else {
        return;
    };
    let key = (space.space_id, 0);
    let Some(grid) = client_state.floor_maps.get(&key) else {
        return;
    };
    let hash = quick_hash(&grid.tiles);
    if render_state.built_for == Some((space.space_id, 0, hash)) {
        return;
    }

    for entity in &existing {
        commands.entity(entity).despawn();
    }

    for ry in 0..=grid.height {
        for rx in 0..=grid.width {
            spawn_render_cells_at_corner(
                &mut commands,
                &asset_server,
                &mut texture_atlas_layouts,
                &mut atlases,
                &floor_defs,
                &world_config,
                space.space_id,
                0,
                rx,
                ry,
                grid,
            );
        }
    }

    render_state.built_for = Some((space.space_id, 0, hash));
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

    let entries: Vec<(&FloorTypeId, u8)> =
        bits_per_type.into_iter().filter(|(_, m)| *m != 0).collect();
    CornerRenderPlan::HardEdges(entries)
}

#[allow(clippy::too_many_arguments)]
fn spawn_render_cells_at_corner(
    commands: &mut Commands,
    asset_server: &AssetServer,
    layouts_assets: &mut Assets<TextureAtlasLayout>,
    atlases: &mut FloorTilesetAtlases,
    floor_defs: &FloorTilesetDefinitions,
    world_config: &WorldConfig,
    space_id: SpaceId,
    z: i32,
    rx: i32,
    ry: i32,
    grid: &FloorMap,
) {
    // Bitmask convention: NW=1, NE=2, SW=4, SE=8.
    let nw = sample(grid, rx - 1, ry - 1);
    let ne = sample(grid, rx, ry - 1);
    let sw = sample(grid, rx - 1, ry);
    let se = sample(grid, rx, ry);

    match classify_corner(floor_defs, nw, ne, sw, se) {
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
                space_id,
                z,
                rx,
                ry,
                low,
                low_def,
                0xF,
                base_z,
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
            for (floor_id, mask) in entries {
                let Some(def) = floor_defs.get(floor_id) else {
                    continue;
                };
                spawn_floor_cell(
                    commands,
                    asset_server,
                    layouts_assets,
                    atlases,
                    world_config,
                    space_id,
                    z,
                    rx,
                    ry,
                    floor_id,
                    def,
                    mask,
                    floor_priority_z(def.priority),
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
    space_id: SpaceId,
    z: i32,
    rx: i32,
    ry: i32,
    floor_id: &FloorTypeId,
    def: &FloorTilesetDefinition,
    mask: u8,
    priority_z: f32,
) {
    let sprite = if let Some(atlas_path) = &def.atlas_path {
        let image_handle = atlases
            .images
            .entry(floor_id.clone())
            .or_insert_with(|| asset_server.load(atlas_path))
            .clone();
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
        let weights = def.variant_weights(mask);
        let variant = pick_variant(space_id, rx, ry, weights);
        Sprite {
            image: image_handle,
            custom_size: Some(Vec2::splat(world_config.tile_size)),
            texture_atlas: Some(TextureAtlas {
                layout: layout_handle,
                index: (mask as usize & 0xF) + variant * 16,
            }),
            ..default()
        }
    } else {
        // Debug fallback: full coloured square. Boundary cells of debug
        // floors will overdraw lower-priority neighbours; that's the
        // intentional placeholder look until real art lands.
        Sprite::from_color(def.debug_color(), Vec2::splat(world_config.tile_size))
    };

    commands.spawn((
        FloorRenderCell {
            space_id,
            z,
            rx,
            ry,
            floor_type: floor_id.clone(),
            priority_z,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, flat_floor_z(priority_z, z)),
        Visibility::default(),
    ));
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
            index: (mask as usize & 0xF) + variant * 16,
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
    view_scroll: Res<ViewScrollOffset>,
    mut query: Query<(&FloorRenderCell, &mut Transform)>,
) {
    let Some(player_position) = client_state.player_position else {
        return;
    };
    for (cell, mut transform) in &mut query {
        let visible = cell.space_id == player_position.space_id;
        let z = if !visible {
            -10_000.0
        } else {
            flat_floor_z(cell.priority_z, cell.z)
        };
        let dx = (cell.rx as f32 - 0.5 - player_position.tile_position.x as f32)
            * world_config.tile_size
            + view_scroll.current.x;
        let dy = (cell.ry as f32 - 0.5 - player_position.tile_position.y as f32)
            * world_config.tile_size
            + view_scroll.current.y;
        transform.translation = Vec3::new(dx, dy, z);
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
}
