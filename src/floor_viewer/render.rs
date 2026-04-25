use std::collections::HashMap;

use bevy::prelude::*;

use crate::floor_viewer::plugin::{
    FloorTilesetAtlases, ViewMode, ViewModeKind, ViewerDirty, ViewerFloorMap,
};
use crate::world::floor_definitions::{FloorTilesetDefinitions, FloorTypeId};
use crate::world::floor_map::FloorMap;

pub const TILE_SIZE: f32 = 32.0;
pub const GRID_W: i32 = 24;
pub const GRID_H: i32 = 24;

const FLOOR_PRIORITY_STEP: f32 = 0.0001;

#[derive(Component)]
pub struct FloorRenderCell;

fn sample(grid: &FloorMap, x: i32, y: i32) -> Option<&FloorTypeId> {
    grid.get(x, y)
}

#[allow(clippy::too_many_arguments)]
pub fn rebuild_render_cells(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut atlases: ResMut<FloorTilesetAtlases>,
    map: Res<ViewerFloorMap>,
    mut dirty: ResMut<ViewerDirty>,
    view_mode: Res<ViewMode>,
    existing: Query<Entity, With<FloorRenderCell>>,
) {
    if !dirty.0 {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }

    let grid = &map.0;
    match view_mode.0 {
        ViewModeKind::Tiled => {
            for ry in 0..=grid.height {
                for rx in 0..=grid.width {
                    spawn_corner(
                        &mut commands,
                        &asset_server,
                        &mut texture_atlas_layouts,
                        &mut atlases,
                        &floor_defs,
                        rx,
                        ry,
                        grid,
                    );
                }
            }
        }
        ViewModeKind::Debug => {
            for y in 0..grid.height {
                for x in 0..grid.width {
                    spawn_debug_tile(&mut commands, &floor_defs, x, y, grid);
                }
            }
        }
    }
    dirty.0 = false;
}

fn spawn_debug_tile(
    commands: &mut Commands,
    floor_defs: &FloorTilesetDefinitions,
    x: i32,
    y: i32,
    grid: &FloorMap,
) {
    let Some(floor_id) = grid.get(x, y) else {
        return;
    };
    let Some(def) = floor_defs.get(floor_id) else {
        return;
    };
    let world_x = (x as f32 - GRID_W as f32 / 2.0 + 0.5) * TILE_SIZE;
    let world_y = (y as f32 - GRID_H as f32 / 2.0 + 0.5) * TILE_SIZE;
    let z = def.priority as f32 * FLOOR_PRIORITY_STEP;
    commands.spawn((
        FloorRenderCell,
        Sprite::from_color(def.debug_color(), Vec2::splat(TILE_SIZE)),
        Transform::from_xyz(world_x, world_y, z),
        Visibility::default(),
    ));
}

#[allow(clippy::too_many_arguments)]
fn spawn_corner(
    commands: &mut Commands,
    asset_server: &AssetServer,
    layouts_assets: &mut Assets<TextureAtlasLayout>,
    atlases: &mut FloorTilesetAtlases,
    floor_defs: &FloorTilesetDefinitions,
    rx: i32,
    ry: i32,
    grid: &FloorMap,
) {
    // Bitmask convention identical to the game: NW=1, NE=2, SW=4, SE=8.
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

    let world_x = (rx as f32 - GRID_W as f32 / 2.0) * TILE_SIZE;
    let world_y = (ry as f32 - GRID_H as f32 / 2.0) * TILE_SIZE;

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
                custom_size: Some(Vec2::splat(TILE_SIZE)),
                texture_atlas: Some(TextureAtlas {
                    layout: layout_handle,
                    index: (*mask as usize) & 0xF,
                }),
                ..default()
            }
        } else {
            Sprite::from_color(def.debug_color(), Vec2::splat(TILE_SIZE))
        };

        let z = def.priority as f32 * FLOOR_PRIORITY_STEP;
        commands.spawn((
            FloorRenderCell,
            sprite,
            Transform::from_xyz(world_x, world_y, z),
            Visibility::default(),
        ));
    }
}
