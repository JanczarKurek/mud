use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::Player;
use crate::world::animation::{JustMoved, VisualOffset};
use crate::world::components::{
    ClientProjectedWorldObject, ClientRemotePlayerVisual, CombatHealthBar, DisplayedVitalStats,
    Facing, HealthBarDisplayPolicy, SpaceResident, TilePosition, ViewPosition, WorldVisual,
};
use crate::world::floors::{VisibleFloorRange, DIMMED_FLOOR_ALPHA};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::resources::{
    ClientRemotePlayerProjectionState, ClientWorldProjectionState, SpaceManager, ViewScrollOffset,
};
use crate::world::setup::{spawn_client_projected_world_object, spawn_client_remote_player};
use crate::world::WorldConfig;

pub fn sync_client_world_projection(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    mut world_config: ResMut<WorldConfig>,
    mut projection_state: ResMut<ClientWorldProjectionState>,
    mut projected_query: Query<(
        Entity,
        &ClientProjectedWorldObject,
        &mut DisplayedVitalStats,
        &mut ViewPosition,
        &mut WorldVisual,
        &mut Facing,
    )>,
) {
    let Some(current_space) = client_state.current_space.as_ref() else {
        info!(
            "sync_client_world_projection: current_space is None, skipping (world_objects={})",
            client_state.world_objects.len()
        );
        return;
    };

    if world_config.current_space_id != current_space.space_id
        || world_config.map_width != current_space.width
        || world_config.map_height != current_space.height
        || world_config.fill_floor_type != current_space.fill_floor_type
    {
        world_config.current_space_id = current_space.space_id;
        world_config.map_width = current_space.width;
        world_config.map_height = current_space.height;
        world_config.fill_floor_type = current_space.fill_floor_type.clone();
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
                object.state.as_deref(),
            );
            projection_state.entities.insert(object.object_id, entity);
            continue;
        };

        let Ok((
            query_entity,
            projected_object,
            mut displayed_vitals,
            mut view,
            mut world_visual,
            mut facing,
        )) = projected_query.get_mut(entity)
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
                object.state.as_deref(),
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
                object.state.as_deref(),
            );
            projection_state
                .entities
                .insert(object.object_id, replacement);
            continue;
        }

        view.space_id = object.position.space_id;
        let old_tile = view.tile;
        view.tile = object.position.tile_position;
        if old_tile != view.tile && old_tile.z == view.tile.z {
            let dx = view.tile.x - old_tile.x;
            let dy = view.tile.y - old_tile.y;
            commands.entity(query_entity).insert((
                JustMoved { dx, dy },
                VisualOffset {
                    current: Vec2::new(
                        -dx as f32 * world_config.tile_size,
                        -dy as f32 * world_config.tile_size,
                    ),
                    elapsed: 0.0,
                    duration: 0.18,
                },
            ));
        }
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
        if facing.0 != object.facing {
            facing.0 = object.facing;
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
    mut projected_query: Query<(
        Entity,
        &ClientRemotePlayerVisual,
        &mut DisplayedVitalStats,
        &mut ViewPosition,
        &mut WorldVisual,
        &mut Facing,
    )>,
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
            projection_state
                .entities
                .insert(remote_player.player_id, entity);
            continue;
        };

        let Ok((
            query_entity,
            projected_player,
            mut displayed_vitals,
            mut view,
            mut world_visual,
            mut facing,
        )) = projected_query.get_mut(entity)
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
            projection_state
                .entities
                .insert(remote_player.player_id, entity);
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

        view.space_id = remote_player.position.space_id;
        let old_tile = view.tile;
        view.tile = remote_player.position.tile_position;
        if old_tile != view.tile && old_tile.z == view.tile.z {
            let dx = view.tile.x - old_tile.x;
            let dy = view.tile.y - old_tile.y;
            if dx.abs() <= 1 && dy.abs() <= 1 {
                commands.entity(query_entity).insert((
                    JustMoved { dx, dy },
                    VisualOffset {
                        current: Vec2::new(
                            -dx as f32 * world_config.tile_size,
                            -dy as f32 * world_config.tile_size,
                        ),
                        elapsed: 0.0,
                        duration: 0.18,
                    },
                ));
            }
        }
        displayed_vitals.health = remote_player.vitals.health;
        displayed_vitals.max_health = remote_player.vitals.max_health;
        displayed_vitals.mana = remote_player.vitals.mana;
        displayed_vitals.max_mana = remote_player.vitals.max_mana;
        if let Some(definition) = definitions.get("player") {
            world_visual.z_index = definition.render.z_index;
        }
        if facing.0 != remote_player.facing {
            facing.0 = remote_player.facing;
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

/// Vertical spacing between floors in world-z space. Must exceed the span of
/// y-sort for any single floor (~1.5 on the largest authored maps) so every
/// entity on floor N renders above every entity on floor N-1.
pub const FLOOR_Z_STEP: f32 = 10.0;

/// Y-sorted entities live above all flat layers (ground, pickups).
/// Lower tile_y = lower on screen = closer to viewer = higher z.
/// Floor offsets are additive and dominate y-sort so upper floors always
/// render above lower ones when both are visible.
pub fn y_sort_z(tile_y: i32, floor: i32) -> f32 {
    floor as f32 * FLOOR_Z_STEP + 1.0 - tile_y as f32 * 0.01
}

/// Flat-layer z for a non-y-sorted entity (ground tiles, pickups). Combines
/// the definition-supplied z_index with the entity's floor so e.g. a
/// floor-plank on floor 1 never sorts behind grass on floor 0.
pub fn flat_floor_z(base_z_index: f32, floor: i32) -> f32 {
    floor as f32 * FLOOR_Z_STEP + base_z_index
}

pub fn sync_tile_transforms(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    view_scroll: Res<ViewScrollOffset>,
    visible_floors: Res<VisibleFloorRange>,
    mut query: Query<
        (
            &ViewPosition,
            &WorldVisual,
            &mut Transform,
            Option<&mut Sprite>,
            Option<&VisualOffset>,
            Option<&Facing>,
        ),
        Without<Player>,
    >,
) {
    let Some(player_position) = client_state.player_position else {
        return;
    };

    for (view, world_visual, mut transform, mut sprite, visual_offset, facing) in &mut query {
        let is_active = view.space_id == player_position.space_id;
        let floor_visible = visible_floors.contains(view.tile.z);

        let z = if !is_active || !floor_visible {
            -10_000.0
        } else if world_visual.y_sort {
            y_sort_z(view.tile.y, view.tile.z)
        } else {
            flat_floor_z(world_visual.z_index, view.tile.z)
        };

        // Rotated sprites use center anchoring — skip the bottom-center y-sort
        // shift so the sprite sits square on the tile after rotation.
        let anchor_y_offset = if world_visual.y_sort && !world_visual.rotation_by_facing {
            -world_config.tile_size * 0.5
        } else {
            0.0
        };

        let entity_offset = visual_offset.map_or(Vec2::ZERO, |o| o.current);

        transform.translation = Vec3::new(
            (view.tile.x - player_position.tile_position.x) as f32 * world_config.tile_size
                + view_scroll.current.x
                + entity_offset.x,
            (view.tile.y - player_position.tile_position.y) as f32 * world_config.tile_size
                + anchor_y_offset
                + view_scroll.current.y
                + entity_offset.y,
            z,
        );

        if world_visual.rotation_by_facing {
            let direction = facing.copied().unwrap_or_default().0;
            transform.rotation = Quat::from_rotation_z(direction.rotation_z_radians());
        }

        if let Some(sprite) = sprite.as_mut() {
            let alpha = if is_active && floor_visible && view.tile.z < visible_floors.player_floor {
                DIMMED_FLOOR_ALPHA
            } else {
                1.0
            };
            sprite.color.set_alpha(alpha);
        }
    }
}

pub fn sync_player_z(
    client_state: Res<ClientGameState>,
    mut query: Query<(&WorldVisual, &ViewPosition, &mut Transform, Option<&Facing>), With<Player>>,
) {
    let Ok((world_visual, view, mut transform, facing)) = query.single_mut() else {
        return;
    };

    if world_visual.y_sort {
        let _ = client_state.player_position;
        // Subtract half-tile epsilon so world objects at the same tile_y always render in front.
        let new_z = y_sort_z(view.tile.y, view.tile.z) - 0.005;
        if (transform.translation.z - new_z).abs() > 0.001 {
            info!(
                "player z update: tile_y={} tile_z={} z_index={} -> z={}",
                view.tile.y, view.tile.z, world_visual.z_index, new_z
            );
        }
        transform.translation.z = new_z;
    } else {
        info!(
            "player y_sort=false, z_index={}, z={}",
            world_visual.z_index, transform.translation.z
        );
    }

    if world_visual.rotation_by_facing {
        let direction = facing.copied().unwrap_or_default().0;
        transform.rotation = Quat::from_rotation_z(direction.rotation_z_radians());
    }
}

/// Mirrors authoritative `SpaceResident` + `TilePosition` onto the presentation-only
/// `ViewPosition` for every entity that is *not* a client-only projection. In
/// EmbeddedClient mode this covers NPCs, ground tiles, containers, and every other
/// server-spawned world object that renders locally. Projected entities
/// (`ClientProjectedWorldObject`, `ClientRemotePlayerVisual`) get their view written
/// by the projection sync systems instead. `Player` entities are handled by
/// `sync_authoritative_player_position_view`.
pub fn sync_authoritative_world_object_position_view(
    mut query: Query<
        (&SpaceResident, &TilePosition, &mut ViewPosition),
        (
            Without<crate::player::components::Player>,
            Without<ClientProjectedWorldObject>,
            Without<ClientRemotePlayerVisual>,
        ),
    >,
) {
    for (space_resident, tile_position, mut view) in &mut query {
        view.space_id = space_resident.space_id;
        view.tile = *tile_position;
    }
}

pub fn sync_combat_health_bars(
    health_bar_query: Query<(
        &DisplayedVitalStats,
        &HealthBarDisplayPolicy,
        &CombatHealthBar,
    )>,
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
    let Ok((mut fill_sprite, mut fill_transform)) = fill_query.get_mut(health_bar.fill_entity)
    else {
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
