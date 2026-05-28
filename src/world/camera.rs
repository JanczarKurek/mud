use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::Player;
use crate::world::components::WorldVisual;
use crate::world::resources::ViewScrollOffset;
use crate::world::WorldConfig;

/// Positions the 2D camera so the player tile sits at screen center, with a
/// smooth lerp during the 0.18 s post-step window driven by `ViewScrollOffset`.
///
/// This replaces the legacy "world scrolls around a fixed camera" scheme — that
/// scheme wrote a fresh `Transform` to every world entity every frame
/// (~5,500 entities) just to translate them by the per-frame scroll, which
/// dominated `propagate_parent_transforms` cost. Moving the camera is a single
/// `Transform` write per frame; sprites stay at their absolute tile positions
/// and only become "changed" when they actually move on the grid.
///
/// To preserve the visual invariant that the player stays at screen center
/// during the scroll (the Tibia look — world flows past a planted player), the
/// player sprite tracks the camera. That's still only one extra `Transform`
/// write per frame, vs. thousands.
pub fn camera_follow(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    view_scroll: Res<ViewScrollOffset>,
    mut camera_q: Query<&mut Transform, (With<Camera2d>, Without<Player>)>,
    mut player_q: Query<(&mut Transform, &WorldVisual), (With<Player>, Without<Camera2d>)>,
) {
    let _t = crate::diagnostics::SystemTimer::new("camera_follow", 1.0);
    let Some(player_pos) = client_state.player_position else {
        return;
    };
    let snapped = view_scroll.snapped();
    // Camera tracks the player's pixel-aligned world position MINUS the
    // residual scroll offset. As `tick_view_scroll` decays the offset to 0,
    // the camera arrives at the new tile center. Pixel snapping is preserved
    // because `player_tile * tile_size` and `snapped` are both integers.
    let target_x = player_pos.tile_position.x as f32 * world_config.tile_size - snapped.x;
    let target_y = player_pos.tile_position.y as f32 * world_config.tile_size - snapped.y;

    if let Ok(mut camera) = camera_q.single_mut() {
        let new = Vec3::new(target_x, target_y, camera.translation.z);
        if camera.translation != new {
            camera.translation = new;
        }
    }

    // Player follows the camera so it visually stays at screen center during
    // the scroll lerp. Anchor offset matches the convention used at spawn time
    // (`src/player/setup.rs:269`) and in `sync_tile_transforms` for y-sorted
    // sprites: bottom-center anchor needs `-tile_size/2` to land on the tile.
    //
    // The player has no z-based visual offset itself — the world tiles around
    // it carry the perspective shift via `floor_screen_offset(view_z,
    // player_z)`, which is fractional in `player_z` so half-block climbs (z=0
    // → z=1) shift the world by half a floor and the player appears to rise.
    if let Ok((mut player_transform, world_visual)) = player_q.single_mut() {
        let anchor_y = if world_visual.y_sort {
            -world_config.tile_size * 0.5
        } else {
            0.0
        };
        let new = Vec3::new(
            target_x,
            target_y + anchor_y,
            player_transform.translation.z,
        );
        if player_transform.translation != new {
            player_transform.translation = new;
        }
    }
}
