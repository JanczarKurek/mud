use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::Player;
use crate::world::animation::{JustMoved, VisualOffset};
use crate::world::components::{
    ClientProjectedWorldObject, ClientRemotePlayerVisual, CombatHealthBar, DisplayedVitalStats,
    Facing, HealthBarDisplayPolicy, SpaceResident, StackOffset, TilePosition, ViewPosition,
    WorldVisual,
};
use crate::world::direction::Direction;
use crate::world::floors::{
    is_indoor_tile, should_apply_indoor_tint, IndoorTileMap, VisibleFloorRange,
};
use crate::world::lighting::srgb_u8_to_linear;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::resources::{
    ClientRemotePlayerProjectionState, ClientWorldProjectionState, SpaceManager,
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
    mut last_had_space: Local<bool>,
    mut projected_query: Query<(
        Entity,
        &ClientProjectedWorldObject,
        &mut DisplayedVitalStats,
        &mut ViewPosition,
        &mut WorldVisual,
        &mut Facing,
    )>,
) {
    let _t = crate::diagnostics::SystemTimer::new("sync_client_world_projection", 1.0);
    let Some(current_space) = client_state.current_space.as_ref() else {
        // Log only on the transition Some→None (or on the first frame, when
        // `*last_had_space` defaults to false). Otherwise this fires every
        // tick the client sits without an authoritative space — most often
        // the brief pre-bootstrap window after entering `InGame`.
        if *last_had_space {
            info!(
                "sync_client_world_projection: current_space cleared (world_objects={})",
                client_state.world_objects.len()
            );
            *last_had_space = false;
        }
        return;
    };
    if !*last_had_space {
        info!(
            "sync_client_world_projection: current_space set to {} ({}) — projecting {} world objects",
            current_space.space_id.0,
            current_space.authored_id,
            client_state.world_objects.len()
        );
        *last_had_space = true;
    }

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
                object.quantity,
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
                object.quantity,
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
                object.quantity,
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
            world_visual.display_height = definition.render.display_height;
            world_visual.stack_order = definition.render.stack_order;
            world_visual.hide_when_inside_facing = definition.render.hide_when_inside_facing;
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

/// How many tiles each floor shifts on screen relative to the player's floor
/// to convey depth. Higher floors shift up-LEFT (negative x, positive y in
/// Bevy 2D world coords with y-up); lower floors shift down-RIGHT. Half a
/// tile per floor reads as a clear depth cue without the jarring full-grid
/// jump of a 1.0 shift.
pub const FLOOR_SCREEN_OFFSET_TILES: f32 = 0.5;

/// Alpha applied to objects flagged `hide_when_inside_facing = South|East`
/// when the player is inside an enclosed area. Faint silhouette rather than
/// a hard hide so the architecture stays legible.
pub const WALL_INSIDE_ALPHA: f32 = 0.15;

/// Up-left screen offset for a sprite on `floor` while the player stands on
/// `player_floor`. Adds to a sprite's translation; floor cells and entity
/// sprites use the same offset so they stay aligned per floor.
pub fn floor_screen_offset(floor: i32, player_floor: i32, tile_size: f32) -> Vec2 {
    let d = (floor - player_floor) as f32;
    Vec2::new(
        -d * FLOOR_SCREEN_OFFSET_TILES * tile_size,
        d * FLOOR_SCREEN_OFFSET_TILES * tile_size,
    )
}

/// Y-sorted entities live above all flat layers (ground, pickups).
/// Lower tile_y = lower on screen = closer to viewer = higher z.
/// `stack_index` breaks ties for tall objects stacked on the same tile
/// (chest atop barrel). Each step is well below the 0.01 row spacing, so
/// up to ~9 deep stacks remain correctly sorted.
/// Floor offsets are additive and dominate y-sort so upper floors always
/// render above lower ones when both are visible.
pub fn y_sort_z(tile_y: i32, floor: i32, stack_index: i32) -> f32 {
    floor as f32 * FLOOR_Z_STEP + 1.0 - tile_y as f32 * 0.01 + stack_index as f32 * 0.001
}

/// Flat-layer z for a non-y-sorted entity (ground tiles, pickups). Combines
/// the definition-supplied z_index with the entity's floor so e.g. a
/// floor-plank on floor 1 never sorts behind grass on floor 0.
pub fn flat_floor_z(base_z_index: f32, floor: i32) -> f32 {
    floor as f32 * FLOOR_Z_STEP + base_z_index
}

/// Compute the y-pixel offset for each tall (`display_height > 0`) object so
/// that multiple objects sharing a tile stack vertically instead of
/// overlapping. Deterministic across frames: entities sort by `(stack_order
/// asc, object_id asc)` and both keys are stable.
///
/// Runs `.after(sync_*_projection).before(sync_tile_transforms)` so the
/// transforms in the same frame pick up the new offsets.
pub fn compute_stack_offsets(
    world_config: Res<WorldConfig>,
    mut query: Query<(
        Entity,
        &ViewPosition,
        &WorldVisual,
        &ClientProjectedWorldObject,
        &mut StackOffset,
    )>,
) {
    let _t = crate::diagnostics::SystemTimer::new("compute_stack_offsets", 1.0);

    // Snapshot the inputs needed for sorting: we can't borrow the query
    // both immutably (for grouping) and mutably (for writing) at the same
    // time, so collect once.
    #[derive(Clone)]
    struct Member {
        entity: Entity,
        stack_order: i32,
        object_id: u64,
        display_height: f32,
    }

    let mut groups: std::collections::HashMap<
        (crate::world::components::SpaceId, i32, i32, i32),
        Vec<Member>,
    > = std::collections::HashMap::new();

    for (entity, view, world_visual, projected, _) in &query {
        if world_visual.display_height <= 0.0 {
            continue;
        }
        groups
            .entry((view.space_id, view.tile.x, view.tile.y, view.tile.z))
            .or_default()
            .push(Member {
                entity,
                stack_order: world_visual.stack_order,
                object_id: projected.object_id,
                display_height: world_visual.display_height,
            });
    }

    // Pixel offset per entity. Anything not touched defaults to 0 below.
    let mut offsets: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    for members in groups.values_mut() {
        members.sort_by(|a, b| {
            a.stack_order
                .cmp(&b.stack_order)
                .then(a.object_id.cmp(&b.object_id))
        });
        let mut cumulative_pixels = 0.0_f32;
        for member in members.iter() {
            offsets.insert(member.entity, cumulative_pixels);
            cumulative_pixels += member.display_height * world_config.tile_size;
        }
    }

    for (entity, _, _, _, mut offset) in &mut query {
        let new_value = offsets.get(&entity).copied().unwrap_or(0.0);
        if (offset.0 - new_value).abs() > f32::EPSILON {
            offset.0 = new_value;
        }
    }
}

pub fn sync_tile_transforms(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    visible_floors: Res<VisibleFloorRange>,
    definitions: Res<OverworldObjectDefinitions>,
    indoor: Res<IndoorTileMap>,
    mut query: Query<
        (
            &ViewPosition,
            &WorldVisual,
            &mut Transform,
            Option<&mut Sprite>,
            Option<&VisualOffset>,
            Option<&Facing>,
            Option<&StackOffset>,
        ),
        Without<Player>,
    >,
) {
    let _t = crate::diagnostics::SystemTimer::new("sync_tile_transforms", 1.0);
    let Some(player_position) = client_state.player_position else {
        return;
    };

    // Wall-fade predicate is player-only, not per-entity — hoist out of the
    // loop to avoid an O(N · world_objects) sweep when there are many tall
    // objects on screen.
    let player_is_inside = is_indoor_tile(
        &client_state,
        &definitions,
        player_position.space_id,
        player_position.tile_position.x,
        player_position.tile_position.y,
        player_position.tile_position.z,
    );

    // Indoor ambient color used as per-sprite tint for floor cells, back-wall
    // sprites (N/W), and objects sitting on indoor tiles. Replaces the indoor
    // half of the fullscreen darkness overlay, which now returns transparent
    // for indoor pixels so tinting sorts correctly per-sprite.
    let indoor_tint_rgb = client_state
        .current_space
        .as_ref()
        .map(|s| srgb_u8_to_linear(s.lighting.indoor_ambient))
        .unwrap_or([1.0, 1.0, 1.0]);

    // Absolute world coords: x and y depend on the entity's own tile, plus a
    // floor-relative up-left offset, plus per-entity `VisualOffset` lerp and
    // any stack offset. Camera follow handles the player-centered scroll, so
    // stable entities still never get marked changed when nothing moves.
    for (view, world_visual, mut transform, mut sprite, visual_offset, facing, stack_offset) in
        &mut query
    {
        let is_active = view.space_id == player_position.space_id;
        let floor_visible = visible_floors.contains(view.tile.z);

        let z = if !is_active || !floor_visible {
            -10_000.0
        } else if world_visual.y_sort {
            // Stack index is folded into the sub-row tiebreak — see y_sort_z.
            let stack_index =
                (stack_offset.map_or(0.0, |s| s.0) / world_config.tile_size.max(1.0)) as i32;
            y_sort_z(view.tile.y, view.tile.z, stack_index)
        } else {
            flat_floor_z(world_visual.z_index, view.tile.z)
        };

        // Bottom-anchored sprites (y-sorted characters AND tall props with
        // display_height) sit with their bottom on the lower edge of the
        // tile, so the transform y needs a half-tile shift to compensate
        // for the BOTTOM_CENTER anchor. Rotated sprites stay center-anchored
        // so rotation pivots around the sprite center.
        let bottom_anchored = (world_visual.y_sort || world_visual.display_height > 0.0)
            && !world_visual.rotation_by_facing;
        let anchor_y_offset = if bottom_anchored {
            -world_config.tile_size * 0.5
        } else {
            0.0
        };

        let entity_offset = visual_offset.map_or(Vec2::ZERO, |o| o.current);
        let stack_y = stack_offset.map_or(0.0, |s| s.0);
        let floor_offset = floor_screen_offset(
            view.tile.z,
            visible_floors.player_floor,
            world_config.tile_size,
        );

        let new_translation = Vec3::new(
            view.tile.x as f32 * world_config.tile_size + entity_offset.x + floor_offset.x,
            view.tile.y as f32 * world_config.tile_size
                + anchor_y_offset
                + entity_offset.y
                + stack_y
                + floor_offset.y,
            z,
        );
        if transform.translation != new_translation {
            transform.translation = new_translation;
        }

        if world_visual.rotation_by_facing {
            let direction = facing.copied().unwrap_or_default().0;
            let new_rotation = Quat::from_rotation_z(direction.rotation_z_radians());
            if transform.rotation != new_rotation {
                transform.rotation = new_rotation;
            }
        }

        if let Some(sprite) = sprite.as_mut() {
            let mut new_alpha = 1.0;
            let mut is_faded_camera_wall = false;
            // Camera-facing walls on the player's floor (or the ceiling floor
            // directly above) fade when the player is inside an enclosed area
            // so the interior stays legible. North/west walls remain visible
            // because they sit "behind" the player and never obstruct view.
            if is_active && player_is_inside {
                if let Some(facing_dir) = world_visual.hide_when_inside_facing {
                    let camera_facing = matches!(facing_dir, Direction::South | Direction::East);
                    let same_building = view.tile.z == player_position.tile_position.z
                        || view.tile.z == player_position.tile_position.z + 1;
                    if camera_facing && same_building {
                        new_alpha = WALL_INSIDE_ALPHA;
                        is_faded_camera_wall = true;
                    }
                }
            }

            // Indoor tint: applies to back walls (N/W) and to any object
            // anchored on an indoor tile. Camera-facing walls being alpha-faded
            // shouldn't also get tinted — they're meant to read as outdoor.
            let apply_tint = is_active
                && !is_faded_camera_wall
                && should_apply_indoor_tint(
                    &indoor,
                    view.space_id,
                    view.tile.x,
                    view.tile.y,
                    view.tile.z,
                    world_visual.hide_when_inside_facing,
                );
            let new_rgb = if apply_tint {
                indoor_tint_rgb
            } else {
                [1.0, 1.0, 1.0]
            };
            let new_color = Color::linear_rgba(new_rgb[0], new_rgb[1], new_rgb[2], new_alpha);
            if sprite.color != new_color {
                sprite.color = new_color;
            }
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
        let new_z = y_sort_z(view.tile.y, view.tile.z, 0) - 0.005;
        if (transform.translation.z - new_z).abs() > 0.001 {
            info!(
                "player z update: tile_y={} tile_z={} z_index={} -> z={}",
                view.tile.y, view.tile.z, world_visual.z_index, new_z
            );
            transform.translation.z = new_z;
        }
    }

    if world_visual.rotation_by_facing {
        let direction = facing.copied().unwrap_or_default().0;
        let new_rotation = Quat::from_rotation_z(direction.rotation_z_radians());
        if transform.rotation != new_rotation {
            transform.rotation = new_rotation;
        }
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
    let _t = crate::diagnostics::SystemTimer::new("sync_combat_health_bars", 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_screen_offset_zero_at_player_floor() {
        // A sprite on the same floor as the player has no offset — Tibia view
        // pivots around the player.
        let offset = floor_screen_offset(2, 2, 48.0);
        assert_eq!(offset, Vec2::ZERO);
    }

    #[test]
    fn floor_screen_offset_up_left_for_higher_z() {
        // One floor above the player → shift half a tile up-LEFT.
        // Bevy world coords: y is up, so up-left = (-x, +y).
        let offset = floor_screen_offset(3, 2, 48.0);
        assert_eq!(offset.x, -24.0);
        assert_eq!(offset.y, 24.0);
    }

    #[test]
    fn floor_screen_offset_down_right_for_lower_z() {
        // One floor below the player → shift half a tile down-right.
        let offset = floor_screen_offset(1, 2, 48.0);
        assert_eq!(offset.x, 24.0);
        assert_eq!(offset.y, -24.0);
    }

    #[test]
    fn y_sort_z_stack_index_breaks_ties() {
        let bottom = y_sort_z(10, 0, 0);
        let middle = y_sort_z(10, 0, 1);
        let top = y_sort_z(10, 0, 2);
        // Stack index bumps z so the upper item renders on top of the lower.
        assert!(top > middle);
        assert!(middle > bottom);
    }

    #[test]
    fn y_sort_z_stack_step_smaller_than_row_step() {
        // Stack tiebreak must not cross the row-spacing of 0.01, or stacked
        // objects on one tile would invert their sort versus a neighbour on
        // the next tile_y.
        let stack_bump = y_sort_z(10, 0, 1) - y_sort_z(10, 0, 0);
        let row_bump = y_sort_z(9, 0, 0) - y_sort_z(10, 0, 0);
        assert!(stack_bump < row_bump);
    }
}
