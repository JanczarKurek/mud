//! Editor-side floor terrain rendering.
//!
//! The in-game floor renderer (`world::floor_render::build_floor_render_cells`)
//! reads from `ClientGameState` and positions cells relative to the local
//! player, neither of which exist in `ClientAppState::MapEditor`. This module
//! drives the same `FloorRenderCell` entities from the server-side `FloorMaps`
//! resource (which the FloorBrush mutates directly) and positions them via
//! `EditorCamera`.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::editor::resources::{EditorCamera, EditorContext, EditorState};
use crate::world::components::SpaceId;
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::floor_map::FloorMaps;
use crate::world::floor_render::{
    floor_grid_hash, rebuild_floor_render_cells_for_grid, spawn_render_cells_at_corner,
    FloorRenderCell, FloorRenderDirty, FloorRenderState, FloorTilesetAtlases,
};
use crate::world::systems::{flat_floor_z, floor_screen_offset};
use crate::world::WorldConfig;

#[derive(Resource, Default, Clone, Debug)]
pub struct EditorFloorRenderState {
    /// One hash per `(space, z)` floor grid, mirroring the in-game
    /// `FloorRenderState`. Multi-floor rendering means the editor now ships
    /// cells for every floor in the active space at once — not just the
    /// editing floor — so the up/down stack stays visible while you author.
    pub built_for: HashMap<(SpaceId, i32), u64>,
}

/// Drives the editor's `FloorRenderCell` entities. Three paths:
/// - **Full rebuild** when the active space changed or no cells exist yet.
/// - **Incremental** when `FloorRenderDirty` has tile coords for the active
///   space — only the 4 corner cells around each dirty tile are respawned, so
///   a single drag-paint isn't a 4096-cell teardown of the whole grid.
/// - **No-op** otherwise. A trailing hash check guards against external
///   mutations (e.g. a wholesale `FloorMap` swap on map switch) that bypass
///   the dirty queue.
/// Mirror of `world::floor_render::build_floor_render_cells`, adapted to the
/// editor's data sources: walks every `FloorMap` in the active space (so
/// upper / lower stories stay visible alongside the active floor) and
/// rebuilds cells per `(space, z)` when the grid's hash changes. A drained
/// `FloorRenderDirty` queue drives an incremental corner-only rebuild for
/// the paint-drag case, exactly like the in-game path.
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
    _editor_state: Res<EditorState>,
    mut render_state: ResMut<EditorFloorRenderState>,
    mut floor_dirty: ResMut<FloorRenderDirty>,
    existing: Query<(Entity, &FloorRenderCell)>,
) {
    let space_id = editor_context.space_id;

    // Sweep entries that no longer match the active space or whose z no
    // longer has a backing FloorMap (e.g. after switching maps, or after a
    // user deletes an upper floor).
    let live_keys: HashSet<(SpaceId, i32)> = floor_maps
        .iter()
        .filter_map(|(sid, z, _)| (sid == space_id).then_some((sid, z)))
        .collect();
    let stale: Vec<(SpaceId, i32)> = render_state
        .built_for
        .keys()
        .copied()
        .filter(|key| !live_keys.contains(key))
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

    // Drain dirty tiles for the active space; partition by z.
    let mut dirty_by_z: HashMap<i32, Vec<(i32, i32)>> = HashMap::new();
    floor_dirty.cells.retain(|(s, dz, x, y)| {
        if *s == space_id {
            dirty_by_z.entry(*dz).or_default().push((*x, *y));
            false
        } else {
            true
        }
    });

    for (sid, z, grid) in floor_maps.iter() {
        if sid != space_id {
            continue;
        }
        let key = (sid, z);
        let hash = floor_grid_hash(&grid.tiles);

        // Incremental: a paint-drag on this z touched a handful of tiles;
        // respawn only the 4 corners that read each one. Skip when the hash
        // matches (no-op frame) or when we have no prior build (cold path
        // below).
        let dirty_tiles = dirty_by_z.remove(&z).unwrap_or_default();
        let has_prior = render_state.built_for.contains_key(&key);
        if !dirty_tiles.is_empty() && has_prior {
            let mut corners: HashSet<(i32, i32)> = HashSet::new();
            for (tx, ty) in &dirty_tiles {
                for dy in 0..=1 {
                    for dx in 0..=1 {
                        corners.insert((tx + dx, ty + dy));
                    }
                }
            }
            for (entity, cell) in &existing {
                if cell.space_id == sid
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
                    sid,
                    z,
                    rx,
                    ry,
                    grid,
                );
            }
            render_state.built_for.insert(key, hash);
            continue;
        }

        // Full rebuild path: this z is new, or its hash changed under us.
        if render_state.built_for.get(&key) == Some(&hash) {
            continue;
        }
        for (entity, cell) in &existing {
            if cell.space_id == sid && cell.z == z {
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
            sid,
            z,
            grid,
        );
        render_state.built_for.insert(key, hash);
    }
}

pub fn editor_sync_floor_render_transforms(
    world_config: Res<WorldConfig>,
    editor_camera: Res<EditorCamera>,
    editor_context: Res<EditorContext>,
    editor_state: Res<EditorState>,
    mut query: Query<(&FloorRenderCell, &mut Transform)>,
) {
    let effective_size = world_config.tile_size * editor_camera.zoom_level;
    // Editor camera perspective is "standing on the active editing floor".
    // Pass that floor's raw half-block z as `player_z` so the active floor
    // sits centered and other floors shift like they do in-game — keeps the
    // editor's diagonal floor stack identical to what the player will see.
    let player_z = editor_state.active_object_raw_z() as f32;
    for (cell, mut transform) in &mut query {
        let visible = cell.space_id == editor_context.space_id;
        let z_sort = if !visible {
            -10_000.0
        } else {
            flat_floor_z(cell.priority_z, cell.z)
        };
        // `cell.z` is in floor-index units; the floor_screen_offset math
        // operates on raw half-block z, hence the `* 2`. Mirrors the in-game
        // sync in `world::floor_render::sync_floor_render_transforms`.
        let floor_offset = if visible {
            floor_screen_offset((cell.z * 2) as f32, player_z, effective_size)
        } else {
            Vec2::ZERO
        };
        let dx = (cell.rx as f32 - 0.5 + cell.local_offset.x - editor_camera.center.x)
            * effective_size
            + floor_offset.x;
        let dy = (cell.ry as f32 - 0.5 + cell.local_offset.y - editor_camera.center.y)
            * effective_size
            + floor_offset.y;
        transform.translation = Vec3::new(dx, dy, z_sort);
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
    editor_render_state.built_for.clear();
    ingame_render_state.built_for.clear();
}
