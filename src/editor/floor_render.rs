//! Editor-side floor terrain rendering.
//!
//! The in-game floor renderer (`world::floor_render::build_floor_render_cells`)
//! reads from `ClientGameState` and positions cells relative to the local
//! player, neither of which exist in `ClientAppState::MapEditor`. This module
//! drives the same `FloorRenderCell` entities from the server-side `FloorMaps`
//! resource (which the FloorBrush mutates directly) and positions them via
//! `EditorCamera`.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::editor::resources::{EditorCamera, EditorContext};
use crate::world::components::{SpaceId, TilePosition};
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::floor_render::{
    floor_grid_hash, rebuild_floor_render_cells_for_grid, spawn_render_cells_at_corner,
    FloorRenderCell, FloorRenderDirty, FloorRenderState, FloorTilesetAtlases,
};
use crate::world::systems::flat_floor_z;
use crate::world::WorldConfig;

#[derive(Resource, Default, Clone, Debug)]
pub struct EditorFloorRenderState {
    pub built_for: Option<(SpaceId, u64)>,
}

/// Drives the editor's `FloorRenderCell` entities. Three paths:
/// - **Full rebuild** when the active space changed or no cells exist yet.
/// - **Incremental** when `FloorRenderDirty` has tile coords for the active
///   space — only the 4 corner cells around each dirty tile are respawned, so
///   a single drag-paint isn't a 4096-cell teardown of the whole grid.
/// - **No-op** otherwise. A trailing hash check guards against external
///   mutations (e.g. a wholesale `FloorMap` swap on map switch) that bypass
///   the dirty queue.
#[allow(clippy::too_many_arguments)]
pub fn editor_build_floor_render_cells(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    floor_defs: Res<FloorTilesetDefinitions>,
    mut atlases: ResMut<FloorTilesetAtlases>,
    world_config: Res<WorldConfig>,
    floor_maps: Res<FloorMaps>,
    editor_context: Res<EditorContext>,
    mut render_state: ResMut<EditorFloorRenderState>,
    mut floor_dirty: ResMut<FloorRenderDirty>,
    existing: Query<(Entity, &FloorRenderCell)>,
) {
    let space_id = editor_context.space_id;
    let z = TilePosition::GROUND_FLOOR;
    let Some(grid) = floor_maps.get(space_id, z) else {
        return;
    };

    // Drain dirty tiles for this (space, z); leave others (e.g. background
    // spaces being edited via Python) for whoever's responsible to consume.
    let mut dirty_tiles: Vec<(i32, i32)> = Vec::new();
    floor_dirty.cells.retain(|(s, dz, x, y)| {
        if *s == space_id && *dz == z {
            dirty_tiles.push((*x, *y));
            false
        } else {
            true
        }
    });

    let need_full_rebuild = render_state
        .built_for
        .map_or(true, |(prev_space, _)| prev_space != space_id);

    if need_full_rebuild {
        for (entity, _) in &existing {
            commands.entity(entity).despawn();
        }
        rebuild_floor_render_cells_for_grid(
            &mut commands,
            &asset_server,
            &mut texture_atlas_layouts,
            &mut atlases,
            &floor_defs,
            &world_config,
            space_id,
            z,
            grid,
        );
    } else if !dirty_tiles.is_empty() {
        // Each tile change touches the 4 corners that read it (NW/NE/SW/SE).
        let mut corners: HashSet<(i32, i32)> = HashSet::new();
        for (tx, ty) in &dirty_tiles {
            for dy in 0..=1 {
                for dx in 0..=1 {
                    corners.insert((tx + dx, ty + dy));
                }
            }
        }
        for (entity, cell) in &existing {
            if cell.space_id == space_id
                && cell.z == z
                && corners.contains(&(cell.rx, cell.ry))
            {
                commands.entity(entity).despawn();
            }
        }
        for (rx, ry) in corners {
            if rx < 0 || ry < 0 || rx > grid.width || ry > grid.height {
                continue;
            }
            spawn_render_cells_at_corner(
                &mut commands,
                &asset_server,
                &mut texture_atlas_layouts,
                &mut atlases,
                &floor_defs,
                &world_config,
                space_id,
                z,
                rx,
                ry,
                grid,
            );
        }
    } else {
        let hash = floor_grid_hash(&grid.tiles);
        if render_state.built_for == Some((space_id, hash)) {
            return;
        }
        for (entity, _) in &existing {
            commands.entity(entity).despawn();
        }
        rebuild_floor_render_cells_for_grid(
            &mut commands,
            &asset_server,
            &mut texture_atlas_layouts,
            &mut atlases,
            &floor_defs,
            &world_config,
            space_id,
            z,
            grid,
        );
    }

    render_state.built_for = Some((space_id, floor_grid_hash(&grid.tiles)));
}

pub fn editor_sync_floor_render_transforms(
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    mut query: Query<(&FloorRenderCell, &mut Transform)>,
) {
    let effective_size = world_config.tile_size * editor_camera.zoom_level;
    for (cell, mut transform) in &mut query {
        let visible = cell.space_id == editor_context.space_id;
        let z = if !visible {
            -10_000.0
        } else {
            flat_floor_z(cell.priority_z, cell.z)
        };
        let dx = (cell.rx as f32 - 0.5 + cell.local_offset.x - editor_camera.center.x)
            * effective_size;
        let dy = (cell.ry as f32 - 0.5 + cell.local_offset.y - editor_camera.center.y)
            * effective_size;
        transform.translation = Vec3::new(dx, dy, z);
        transform.scale = Vec3::splat(editor_camera.zoom_level);
    }
}

/// Despawn all editor-spawned floor cells when leaving the map editor so an
/// in-game session that follows starts from a clean slate. Also resets the
/// in-game `FloorRenderState` so the next `build_floor_render_cells` tick
/// re-spawns the cells we just removed.
pub fn cleanup_editor_floor_cells(
    mut commands: Commands,
    cells: Query<Entity, With<FloorRenderCell>>,
    mut editor_render_state: ResMut<EditorFloorRenderState>,
    mut ingame_render_state: ResMut<FloorRenderState>,
) {
    for entity in &cells {
        commands.entity(entity).despawn();
    }
    editor_render_state.built_for = None;
    ingame_render_state.built_for = None;
}
