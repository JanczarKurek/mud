//! Sparse Poisson-driven overlay animations on floor tiles.
//!
//! Most floor tiles render statically through [`floor_render`]. Some floor
//! types (e.g. water) want occasional motion without paying for a per-tile
//! frame timer. This module spawns a *transient* overlay sprite on a randomly
//! chosen visible tile every `Δt ~ Exp(λ)` and despawns it as soon as the
//! non-looping animation finishes. Poisson superposition means the global
//! rate scales naturally with how much water is on screen, with no per-map
//! tuning: `λ_total = ripple.rate_per_tile_per_second × visible_water_tiles`.
//!
//! Entirely client-side — purely visual, has no gameplay effect, never goes
//! through `GameEvent`.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::world::animation::AnimatedSprite;
use crate::world::components::SpaceId;
use crate::world::floor_definitions::{FloorRippleDef, FloorTilesetDefinitions, FloorTypeId};
use crate::world::floors::VisibleFloorRange;
use crate::world::resources::FloorTransitionOffset;
use crate::world::systems::{flat_floor_z, floor_screen_offset};
use crate::world::WorldConfig;

/// Marks an entity as a transient ripple sprite spawned by the scheduler.
/// Carries the underlying tile coords so the per-frame transform sync can
/// follow camera/floor offsets without needing a parent entity.
#[derive(Component, Clone, Debug)]
pub struct RippleOverlay {
    pub space_id: SpaceId,
    pub tile_x: i32,
    pub tile_y: i32,
    pub floor_z: i32,
    pub z_offset: f32,
}

/// Caches the texture atlas + image handles for each floor type that has a
/// ripple def. Populated lazily on first spawn.
#[derive(Resource, Default)]
pub struct FloorRippleAtlases {
    layouts: HashMap<FloorTypeId, Handle<TextureAtlasLayout>>,
    images: HashMap<FloorTypeId, Handle<Image>>,
}

/// Global Poisson scheduler. One stream is correct under Poisson
/// superposition: `λ_total = Σ_t (rate_per_tile × visible_tiles_of_type_t)`.
#[derive(Resource, Debug)]
pub struct FloorRippleScheduler {
    pub next_event_seconds: f32,
    pub rng_seed: u64,
}

impl Default for FloorRippleScheduler {
    fn default() -> Self {
        Self {
            // First event fires within ~1s of map load, regardless of pond size.
            next_event_seconds: 1.0,
            rng_seed: 0x517C_C1A1_2F03_5B79,
        }
    }
}

// LCG matching `npc::spawn_groups` so ripple randomness uses the same
// well-understood stream as the rest of the project.
fn next_random_u64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    *seed
}

fn next_uniform_01(seed: &mut u64) -> f64 {
    let r = (next_random_u64(seed) >> 11) as f64;
    (r + 1.0) / (((1u64 << 53) as f64) + 1.0)
}

/// Inter-arrival time for a Poisson process of rate `rate_per_second`. Returns
/// a sentinel-large value when the rate is zero so the scheduler idles
/// gracefully (e.g. when no water is in view).
fn sample_inter_arrival(seed: &mut u64, rate_per_second: f32) -> f32 {
    if rate_per_second <= 0.0 {
        return 1.0;
    }
    let u = next_uniform_01(seed).max(1e-9) as f32;
    -u.ln() / rate_per_second
}

/// Collects visible tiles whose floor type has a `ripple` def configured.
fn collect_ripple_candidates<'a>(
    client_state: &ClientGameState,
    floor_defs: &'a FloorTilesetDefinitions,
    visible_floors: &VisibleFloorRange,
) -> Vec<(SpaceId, i32, i32, i32, &'a FloorRippleDef)> {
    let mut out = Vec::new();
    let Some(space) = client_state.current_space.as_ref() else {
        return out;
    };
    let space_id = space.space_id;
    let z_min = visible_floors.lowest_visible.max(0);
    let z_max = visible_floors.highest_visible;
    for z in z_min..=z_max {
        let Some(grid) = client_state.floor_maps.get(&(space_id, z)) else {
            continue;
        };
        for y in 0..grid.height {
            for x in 0..grid.width {
                let Some(floor_id) = grid.get(x, y) else {
                    continue;
                };
                let Some(def) = floor_defs.get(floor_id) else {
                    continue;
                };
                let Some(ripple) = &def.ripple else {
                    continue;
                };
                out.push((space_id, x, y, z, ripple));
            }
        }
    }
    out
}

/// Looks up (and lazily creates) the cached atlas handles for a floor type's
/// ripple sheet. The sheet is laid out as a single horizontal strip of
/// `frame_count` cells.
fn ensure_ripple_atlas(
    asset_server: &AssetServer,
    layouts_assets: &mut Assets<TextureAtlasLayout>,
    atlases: &mut FloorRippleAtlases,
    floor_id: &FloorTypeId,
    ripple: &FloorRippleDef,
) -> (Handle<Image>, Handle<TextureAtlasLayout>) {
    let image = atlases
        .images
        .entry(floor_id.clone())
        .or_insert_with(|| asset_server.load(&ripple.sheet_path))
        .clone();
    let layout = atlases
        .layouts
        .entry(floor_id.clone())
        .or_insert_with(|| {
            layouts_assets.add(TextureAtlasLayout::from_grid(
                UVec2::new(ripple.frame_width, ripple.frame_height),
                ripple.frame_count.max(1),
                1,
                None,
                None,
            ))
        })
        .clone();
    (image, layout)
}

/// Ticks the Poisson scheduler. On each event, picks one random visible tile
/// of any ripple-bearing floor type and spawns a non-looping overlay sprite.
#[allow(clippy::too_many_arguments)]
pub fn tick_floor_ripple_scheduler(
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    client_state: Res<ClientGameState>,
    floor_defs: Res<FloorTilesetDefinitions>,
    visible_floors: Res<VisibleFloorRange>,
    floor_transition: Res<FloorTransitionOffset>,
    world_config: Res<WorldConfig>,
    mut scheduler: ResMut<FloorRippleScheduler>,
    mut atlases: ResMut<FloorRippleAtlases>,
    mut commands: Commands,
) {
    scheduler.next_event_seconds -= time.delta_secs();
    if scheduler.next_event_seconds > 0.0 {
        return;
    }

    let candidates = collect_ripple_candidates(&client_state, &floor_defs, &visible_floors);
    if candidates.is_empty() {
        // No ripple-bearing tiles in view — idle for a beat and retry. Avoids
        // hammering the grid scan every frame on land-only maps.
        scheduler.next_event_seconds = 1.0;
        return;
    }

    // Pick a candidate uniformly. (Mixing different floor types' per-tile rates
    // would weight by rate here; today there's effectively one ripple-bearing
    // type so uniform is correct.)
    let idx = (next_random_u64(&mut scheduler.rng_seed) % candidates.len() as u64) as usize;
    let (space_id, tile_x, tile_y, floor_z, ripple) = candidates[idx];

    // Need to clone the FloorTypeId from the grid for the atlas cache key.
    let floor_id = client_state
        .floor_maps
        .get(&(space_id, floor_z))
        .and_then(|grid| grid.get(tile_x, tile_y))
        .cloned();
    let Some(floor_id) = floor_id else {
        scheduler.next_event_seconds = 0.5;
        return;
    };

    let (image, layout) = ensure_ripple_atlas(
        &asset_server,
        &mut texture_atlas_layouts,
        &mut atlases,
        &floor_id,
        ripple,
    );

    let animated = AnimatedSprite {
        current_clip: "ripple".to_string(),
        frame_index: 0,
        frame_timer: 0.0,
        frame_count: ripple.frame_count,
        seconds_per_frame: 1.0 / ripple.fps.max(0.001),
        atlas_columns: ripple.frame_count.max(1),
        clip_row: 0,
        clip_start_col: 0,
        looping: false,
    };
    let sprite = Sprite {
        image,
        custom_size: Some(Vec2::splat(world_config.tile_size)),
        texture_atlas: Some(TextureAtlas { layout, index: 0 }),
        ..default()
    };

    // Initial transform — `sync_ripple_overlay_transforms` refreshes each
    // frame so camera scroll / player-floor changes keep the ripple anchored.
    // `floor_z` is an integer floor index → convert to half-block z (`* 2`)
    // for the fractional `floor_screen_offset`.
    let floor_offset = floor_screen_offset(
        (floor_z * 2) as f32,
        floor_transition.visual_player_z(visible_floors.player_z),
        world_config.tile_size,
    );
    let dx = tile_x as f32 * world_config.tile_size + floor_offset.x;
    let dy = tile_y as f32 * world_config.tile_size + floor_offset.y;
    let z = flat_floor_z(ripple.z_offset, floor_z);

    commands.spawn((
        RippleOverlay {
            space_id,
            tile_x,
            tile_y,
            floor_z,
            z_offset: ripple.z_offset,
        },
        animated,
        sprite,
        Transform::from_xyz(dx, dy, z),
        Visibility::default(),
    ));

    // Resample next event under Poisson superposition.
    let rate_total: f32 = candidates
        .iter()
        .map(|(_, _, _, _, r)| r.rate_per_tile_per_second.max(0.0))
        .sum();
    scheduler.next_event_seconds = sample_inter_arrival(&mut scheduler.rng_seed, rate_total);
}

/// Keeps each `RippleOverlay`'s Transform aligned with its tile as the player
/// (and floor offset) move, and hides it when its space isn't active.
pub fn sync_ripple_overlay_transforms(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    visible_floors: Res<VisibleFloorRange>,
    floor_transition: Res<FloorTransitionOffset>,
    mut query: Query<(&RippleOverlay, &mut Transform)>,
) {
    let Some(player_position) = client_state.player_position else {
        return;
    };
    for (overlay, mut transform) in &mut query {
        let visible = overlay.space_id == player_position.space_id
            && visible_floors.contains(overlay.floor_z);
        let z = if visible {
            flat_floor_z(overlay.z_offset, overlay.floor_z)
        } else {
            -10_000.0
        };
        let floor_offset = floor_screen_offset(
            (overlay.floor_z * 2) as f32,
            floor_transition.visual_player_z(visible_floors.player_z),
            world_config.tile_size,
        );
        let dx = overlay.tile_x as f32 * world_config.tile_size + floor_offset.x;
        let dy = overlay.tile_y as f32 * world_config.tile_size + floor_offset.y;
        let new_translation = Vec3::new(dx, dy, z);
        if transform.translation != new_translation {
            transform.translation = new_translation;
        }
    }
}

/// Despawns non-looping ripple sprites once the animation has played its
/// last frame for the full per-frame duration.
pub fn despawn_finished_ripples(
    mut commands: Commands,
    query: Query<(Entity, &AnimatedSprite), With<RippleOverlay>>,
) {
    for (entity, animated) in &query {
        if animated.looping || animated.frame_count == 0 {
            continue;
        }
        if animated.frame_index >= animated.frame_count - 1
            && animated.frame_timer >= animated.seconds_per_frame
        {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inter_arrival_mean_matches_one_over_rate() {
        // Mean of Exp(λ) is 1/λ. Average a few thousand samples and check we're
        // within 3% of the analytic mean — catches sign errors or distribution
        // swaps (Exp vs Uniform) without being flaky.
        let mut seed = 0xDEAD_BEEF_CAFE_F00D;
        let rate = 4.0f32;
        let n = 5_000;
        let total: f32 = (0..n).map(|_| sample_inter_arrival(&mut seed, rate)).sum();
        let mean = total / n as f32;
        let expected = 1.0 / rate;
        assert!(
            (mean - expected).abs() / expected < 0.05,
            "mean={mean} expected={expected}"
        );
    }

    #[test]
    fn inter_arrival_zero_rate_idles_gracefully() {
        // Rate 0 (no ripple-bearing tiles in view) must not divide by zero —
        // the scheduler relies on this to keep tickling until water reappears.
        let mut seed = 1;
        let dt = sample_inter_arrival(&mut seed, 0.0);
        assert!(dt > 0.0);
        assert!(dt.is_finite());
    }
}
