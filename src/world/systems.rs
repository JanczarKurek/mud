use bevy::prelude::*;

use crate::player::components::{Player, VitalStats};
use crate::world::components::{CombatHealthBar, TilePosition, WorldVisual};
use crate::world::WorldConfig;

pub fn sync_tile_transforms(
    world_config: Res<WorldConfig>,
    player_query: Query<&TilePosition, With<Player>>,
    mut query: Query<(&TilePosition, &WorldVisual, &mut Transform), Without<Player>>,
) {
    let Ok(player_position) = player_query.single() else {
        return;
    };

    for (tile_position, world_visual, mut transform) in &mut query {
        transform.translation = Vec3::new(
            (tile_position.x - player_position.x) as f32 * world_config.tile_size,
            (tile_position.y - player_position.y) as f32 * world_config.tile_size,
            world_visual.z_index,
        );
    }
}

pub fn sync_combat_health_bars(
    bar_query: Query<(&VitalStats, &CombatHealthBar)>,
    mut visibility_query: Query<&mut Visibility>,
    mut fill_query: Query<(&mut Sprite, &mut Transform)>,
) {
    for (vital_stats, health_bar) in &bar_query {
        let Ok(mut root_visibility) = visibility_query.get_mut(health_bar.root_entity) else {
            continue;
        };
        let Ok((mut fill_sprite, mut fill_transform)) = fill_query.get_mut(health_bar.fill_entity)
        else {
            continue;
        };

        let is_damaged =
            vital_stats.health < vital_stats.max_health && vital_stats.max_health > 0.0;
        *root_visibility = if is_damaged {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

        let health_ratio = (vital_stats.health / vital_stats.max_health).clamp(0.0, 1.0);
        let fill_width = (health_ratio * health_bar.fill_width).max(0.0);
        if let Some(custom_size) = &mut fill_sprite.custom_size {
            custom_size.x = fill_width;
        }
        fill_transform.translation.x = -(health_bar.fill_width - fill_width) * 0.5;
    }
}
