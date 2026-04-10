use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::{Player, VitalStats};
use crate::world::components::{
    ClientProjectedWorldObject, CombatHealthBar, OverworldObject, TilePosition, WorldVisual,
};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::resources::ClientWorldProjectionState;
use crate::world::setup::spawn_client_projected_world_object;
use crate::world::WorldConfig;

pub fn sync_client_world_projection(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    mut projection_state: ResMut<ClientWorldProjectionState>,
    mut projected_query: Query<
        (
            Entity,
            &ClientProjectedWorldObject,
            &mut TilePosition,
            &mut WorldVisual,
        ),
    >,
) {
    for object in client_state.world_objects.values() {
        let Some(&entity) = projection_state.entities.get(&object.object_id) else {
            let entity = spawn_client_projected_world_object(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                object.object_id,
                &object.definition_id,
                object.tile_position,
                object.is_npc,
            );
            projection_state.entities.insert(object.object_id, entity);
            continue;
        };

        let Ok((query_entity, projected_object, mut tile_position, mut world_visual)) =
            projected_query.get_mut(entity)
        else {
            let entity = spawn_client_projected_world_object(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                object.object_id,
                &object.definition_id,
                object.tile_position,
                object.is_npc,
            );
            projection_state.entities.insert(object.object_id, entity);
            continue;
        };

        if projected_object.definition_id != object.definition_id {
            commands.entity(query_entity).despawn();
            let replacement = spawn_client_projected_world_object(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                object.object_id,
                &object.definition_id,
                object.tile_position,
                object.is_npc,
            );
            projection_state.entities.insert(object.object_id, replacement);
            continue;
        }

        *tile_position = object.tile_position;
        if let Some(definition) = definitions.get(&object.definition_id) {
            world_visual.z_index = definition.render.z_index;
        }
    }

    let stale_object_ids = projection_state
        .entities
        .keys()
        .copied()
        .filter(|object_id| !client_state.world_objects.contains_key(object_id))
        .collect::<Vec<_>>();

    for object_id in stale_object_ids {
        if let Some(entity) = projection_state.entities.remove(&object_id) {
            commands.entity(entity).despawn();
        }
    }
}

pub fn sync_tile_transforms(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    mut query: Query<(&TilePosition, &WorldVisual, &mut Transform), Without<Player>>,
) {
    let Some(player_position) = client_state.player_tile_position else {
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
    player_bar_query: Query<(&VitalStats, &CombatHealthBar), With<Player>>,
    projected_bar_query: Query<(&ClientProjectedWorldObject, &CombatHealthBar)>,
    server_vitals_query: Query<(&OverworldObject, &VitalStats), Without<Player>>,
    mut visibility_query: Query<&mut Visibility>,
    mut fill_query: Query<(&mut Sprite, &mut Transform)>,
) {
    for (vital_stats, health_bar) in &player_bar_query {
        sync_health_bar(vital_stats, health_bar, &mut visibility_query, &mut fill_query);
    }

    for (projected_object, health_bar) in &projected_bar_query {
        let Some((_, vital_stats)) = server_vitals_query
            .iter()
            .find(|(object, _)| object.object_id == projected_object.object_id)
        else {
            continue;
        };

        sync_health_bar(vital_stats, health_bar, &mut visibility_query, &mut fill_query);
    }
}

fn sync_health_bar(
    vital_stats: &VitalStats,
    health_bar: &CombatHealthBar,
    visibility_query: &mut Query<&mut Visibility>,
    fill_query: &mut Query<(&mut Sprite, &mut Transform)>,
) {
    let Ok(mut root_visibility) = visibility_query.get_mut(health_bar.root_entity) else {
        return;
    };
    let Ok((mut fill_sprite, mut fill_transform)) = fill_query.get_mut(health_bar.fill_entity)
    else {
        return;
    };

    let is_damaged = vital_stats.health < vital_stats.max_health && vital_stats.max_health > 0.0;
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
