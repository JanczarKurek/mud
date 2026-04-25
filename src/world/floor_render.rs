use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::Hasher;

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::world::components::SpaceId;
use crate::world::floor_definitions::{FloorTilesetDefinitions, FloorTypeId};
use crate::world::floor_map::FloorMap;
use crate::world::resources::ViewScrollOffset;
use crate::world::systems::flat_floor_z;
use crate::world::WorldConfig;

/// Marks a presentation-only entity that represents one render-cell of one
/// floor type at a world-tile *corner*. Render cells live at half-tile offsets
/// (rx, ry) - 0.5 in world coordinates and read the 4 surrounding world tiles.
#[derive(Component, Clone, Debug)]
pub struct FloorRenderCell {
    pub space_id: SpaceId,
    pub z: i32,
    pub rx: i32,
    pub ry: i32,
    pub floor_type: FloorTypeId,
    pub priority: i32,
}

#[derive(Resource, Default)]
pub struct FloorTilesetAtlases {
    pub layouts: HashMap<FloorTypeId, Handle<TextureAtlasLayout>>,
    pub images: HashMap<FloorTypeId, Handle<Image>>,
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

    let mut bits_per_type: HashMap<&FloorTypeId, u8> = HashMap::new();
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

    for (floor_id, mask) in &bits_per_type {
        if *mask == 0 {
            continue;
        }
        let Some(def) = floor_defs.get(floor_id) else {
            continue;
        };

        let sprite = if let Some(atlas_path) = &def.atlas_path {
            let image_handle = atlases
                .images
                .entry((*floor_id).clone())
                .or_insert_with(|| asset_server.load(atlas_path))
                .clone();
            let layout_handle = atlases
                .layouts
                .entry((*floor_id).clone())
                .or_insert_with(|| {
                    let layout = TextureAtlasLayout::from_grid(
                        UVec2::splat(def.tile_size_px),
                        4,
                        4,
                        None,
                        None,
                    );
                    layouts_assets.add(layout)
                })
                .clone();
            Sprite {
                image: image_handle,
                custom_size: Some(Vec2::splat(world_config.tile_size)),
                texture_atlas: Some(TextureAtlas {
                    layout: layout_handle,
                    index: (*mask as usize) & 0xF,
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
                floor_type: (*floor_id).clone(),
                priority: def.priority,
            },
            sprite,
            Transform::from_xyz(0.0, 0.0, flat_floor_z(floor_priority_z(def.priority), z)),
            Visibility::default(),
        ));
    }
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
            flat_floor_z(floor_priority_z(cell.priority), cell.z)
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
