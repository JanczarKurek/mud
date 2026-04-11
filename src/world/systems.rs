use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::Player;
use crate::world::components::{
    ClientProjectedWorldObject, ClientRemotePlayerVisual, CombatHealthBar, DisplayedVitalStats,
    HealthBarDisplayPolicy, SpaceResident, TilePosition, WorldVisual,
};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::resources::{ClientRemotePlayerProjectionState, ClientWorldProjectionState, SpaceManager};
use crate::world::setup::{spawn_client_projected_world_object, spawn_client_remote_player};
use crate::world::WorldConfig;

pub fn sync_client_world_projection(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    mut world_config: ResMut<WorldConfig>,
    mut projection_state: ResMut<ClientWorldProjectionState>,
    mut projected_query: Query<
        (
            Entity,
            &ClientProjectedWorldObject,
            &mut DisplayedVitalStats,
            &mut SpaceResident,
            &mut TilePosition,
            &mut WorldVisual,
        ),
    >,
) {
    let Some(current_space) = client_state.current_space.as_ref() else {
        return;
    };

    if world_config.current_space_id != current_space.space_id
        || world_config.map_width != current_space.width
        || world_config.map_height != current_space.height
        || world_config.fill_object_type != current_space.fill_object_type
    {
        world_config.current_space_id = current_space.space_id;
        world_config.map_width = current_space.width;
        world_config.map_height = current_space.height;
        world_config.fill_object_type = current_space.fill_object_type.clone();
    }

    projection_state.active_space_id = Some(current_space.space_id);

    for object in client_state.world_objects.values() {
        let Some(&entity) = projection_state.entities.get(&object.object_id) else {
            let entity = spawn_client_projected_world_object(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                object.object_id,
                &object.definition_id,
                object.position,
                object.is_npc,
            );
            projection_state.entities.insert(object.object_id, entity);
            continue;
        };

        let Ok((
            query_entity,
            projected_object,
            mut displayed_vitals,
            mut space_resident,
            mut tile_position,
            mut world_visual,
        )) =
            projected_query.get_mut(entity)
        else {
            let entity = spawn_client_projected_world_object(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                object.object_id,
                &object.definition_id,
                object.position,
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
                object.position,
                object.is_npc,
            );
            projection_state.entities.insert(object.object_id, replacement);
            continue;
        }

        space_resident.space_id = object.position.space_id;
        *tile_position = object.position.tile_position;
        if let Some(vitals) = object.vitals {
            displayed_vitals.health = vitals.health;
            displayed_vitals.max_health = vitals.max_health;
            displayed_vitals.mana = vitals.mana;
            displayed_vitals.max_mana = vitals.max_mana;
        } else {
            *displayed_vitals = DisplayedVitalStats::default();
        }
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

pub fn sync_remote_player_projection(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    mut projection_state: ResMut<ClientRemotePlayerProjectionState>,
    mut projected_query: Query<
        (
            Entity,
            &ClientRemotePlayerVisual,
            &mut DisplayedVitalStats,
            &mut SpaceResident,
            &mut TilePosition,
            &mut WorldVisual,
        ),
    >,
) {
    for remote_player in client_state.remote_players.values() {
        let Some(&entity) = projection_state.entities.get(&remote_player.player_id) else {
            let entity = spawn_client_remote_player(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                remote_player.player_id,
                remote_player.object_id,
                remote_player.position,
            );
            projection_state.entities.insert(remote_player.player_id, entity);
            continue;
        };

        let Ok((
            query_entity,
            projected_player,
            mut displayed_vitals,
            mut space_resident,
            mut tile_position,
            mut world_visual,
        )) =
            projected_query.get_mut(entity)
        else {
            let entity = spawn_client_remote_player(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                remote_player.player_id,
                remote_player.object_id,
                remote_player.position,
            );
            projection_state.entities.insert(remote_player.player_id, entity);
            continue;
        };

        if projected_player.object_id != remote_player.object_id {
            commands.entity(query_entity).despawn();
            let replacement = spawn_client_remote_player(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                remote_player.player_id,
                remote_player.object_id,
                remote_player.position,
            );
            projection_state
                .entities
                .insert(remote_player.player_id, replacement);
            continue;
        }

        space_resident.space_id = remote_player.position.space_id;
        *tile_position = remote_player.position.tile_position;
        displayed_vitals.health = remote_player.vitals.health;
        displayed_vitals.max_health = remote_player.vitals.max_health;
        displayed_vitals.mana = remote_player.vitals.mana;
        displayed_vitals.max_mana = remote_player.vitals.max_mana;
        if let Some(definition) = definitions.get("player") {
            world_visual.z_index = definition.render.z_index;
        }
    }

    let stale_player_ids = projection_state
        .entities
        .keys()
        .copied()
        .filter(|player_id| !client_state.remote_players.contains_key(player_id))
        .collect::<Vec<_>>();

    for player_id in stale_player_ids {
        if let Some(entity) = projection_state.entities.remove(&player_id) {
            commands.entity(entity).despawn();
        }
    }
}

pub fn sync_tile_transforms(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    mut query: Query<(&SpaceResident, &TilePosition, &WorldVisual, &mut Transform), Without<Player>>,
) {
    let Some(player_position) = client_state.player_position else {
        return;
    };

    for (space_resident, tile_position, world_visual, mut transform) in &mut query {
        let is_active = space_resident.space_id == player_position.space_id;
        transform.translation = Vec3::new(
            (tile_position.x - player_position.tile_position.x) as f32 * world_config.tile_size,
            (tile_position.y - player_position.tile_position.y) as f32 * world_config.tile_size,
            if is_active {
                world_visual.z_index
            } else {
                -10_000.0
            },
        );
    }
}

pub fn sync_combat_health_bars(
    health_bar_query: Query<(&DisplayedVitalStats, &HealthBarDisplayPolicy, &CombatHealthBar)>,
    mut visibility_query: Query<&mut Visibility>,
    mut fill_query: Query<(&mut Sprite, &mut Transform)>,
) {
    for (displayed_vitals, display_policy, health_bar) in &health_bar_query {
        sync_displayed_health_bar(
            displayed_vitals,
            display_policy,
            health_bar,
            &mut visibility_query,
            &mut fill_query,
        );
    }
}

pub fn cleanup_empty_ephemeral_spaces(
    mut commands: Commands,
    mut space_manager: ResMut<SpaceManager>,
    player_query: Query<&SpaceResident, With<Player>>,
    resident_query: Query<(Entity, &SpaceResident), Without<Player>>,
) {
    let occupied_spaces = player_query
        .iter()
        .map(|resident| resident.space_id)
        .collect::<std::collections::HashSet<_>>();

    let stale_spaces = space_manager
        .spaces
        .values()
        .filter(|space| !space.permanence.is_persistent())
        .filter(|space| !occupied_spaces.contains(&space.id))
        .map(|space| space.id)
        .collect::<Vec<_>>();

    for space_id in stale_spaces {
        for (entity, resident) in &resident_query {
            if resident.space_id == space_id {
                commands.entity(entity).despawn();
            }
        }
        let _ = space_manager.remove_space(space_id);
    }
}

fn sync_displayed_health_bar(
    vital_stats: &DisplayedVitalStats,
    display_policy: &HealthBarDisplayPolicy,
    health_bar: &CombatHealthBar,
    visibility_query: &mut Query<&mut Visibility>,
    fill_query: &mut Query<(&mut Sprite, &mut Transform)>,
) {
    let Ok(mut root_visibility) = visibility_query.get_mut(health_bar.root_entity) else {
        return;
    };
    let Ok((mut fill_sprite, mut fill_transform)) = fill_query.get_mut(health_bar.fill_entity) else {
        return;
    };

    if vital_stats.max_health <= 0.0
        || (!display_policy.always_visible && vital_stats.health >= vital_stats.max_health)
    {
        *root_visibility = Visibility::Hidden;
        return;
    }

    *root_visibility = Visibility::Visible;
    let ratio = (vital_stats.health / vital_stats.max_health).clamp(0.0, 1.0);
    fill_sprite.custom_size = Some(Vec2::new(health_bar.fill_width * ratio, 3.0));
    fill_transform.translation.x = -health_bar.fill_width * (1.0 - ratio) * 0.5;
}
