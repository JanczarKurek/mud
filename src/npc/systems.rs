use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::npc::components::{
    HostileBehavior, Npc, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
};
use crate::player::components::Player;
use crate::world::components::{Collider, TilePosition};

pub fn update_roaming_npcs(
    time: Res<Time>,
    blocker_query: Query<&TilePosition, (With<Collider>, Without<Npc>)>,
    player_query: Query<(Entity, &TilePosition), (With<Player>, Without<Npc>)>,
    mut npc_query: Query<
        (
            Entity,
            &mut TilePosition,
            &RoamingBehavior,
            Option<&HostileBehavior>,
            Option<&mut CombatTarget>,
            &mut RoamingStepTimer,
            &mut RoamingRandomState,
        ),
        (With<Npc>, Without<Player>),
    >,
    mut commands: Commands,
) {
    let player = player_query
        .iter()
        .next()
        .map(|(entity, tile_position)| (entity, *tile_position));

    let npc_positions: Vec<(Entity, TilePosition)> = npc_query
        .iter()
        .map(|(entity, tile_position, ..)| (entity, *tile_position))
        .collect();

    for (
        entity,
        mut tile_position,
        behavior,
        hostile_behavior,
        combat_target,
        mut timer,
        mut random_state,
    ) in &mut npc_query
    {
        timer.remaining_seconds = (timer.remaining_seconds - time.delta_secs()).max(0.0);
        if timer.remaining_seconds > 0.0 {
            continue;
        }

        let player_position = player.map(|(_, position)| position);
        let chase_target = select_chase_target(
            entity,
            *tile_position,
            hostile_behavior,
            combat_target.as_deref(),
            player,
            &mut commands,
        );

        if let Some(target_position) = choose_roaming_step(
            entity,
            *tile_position,
            behavior,
            chase_target,
            &mut random_state,
            &blocker_query,
            player_position,
            &npc_positions,
        ) {
            *tile_position = target_position;
        }

        timer.remaining_seconds = behavior.step_interval_seconds;
    }
}

fn choose_roaming_step(
    entity: Entity,
    tile_position: TilePosition,
    behavior: &RoamingBehavior,
    chase_target: Option<TilePosition>,
    random_state: &mut RoamingRandomState,
    blocker_query: &Query<&TilePosition, (With<Collider>, Without<Npc>)>,
    player_position: Option<TilePosition>,
    npc_positions: &[(Entity, TilePosition)],
) -> Option<TilePosition> {
    if let Some(chase_target) = chase_target {
        return choose_chase_step(
            entity,
            tile_position,
            chase_target,
            blocker_query,
            player_position,
            npc_positions,
        );
    }

    if !behavior.bounds.contains(tile_position.x, tile_position.y) {
        let return_target = TilePosition::new(
            tile_position
                .x
                .clamp(behavior.bounds.min_x, behavior.bounds.max_x),
            tile_position
                .y
                .clamp(behavior.bounds.min_y, behavior.bounds.max_y),
        );

        return choose_seek_step(
            entity,
            tile_position,
            return_target,
            blocker_query,
            player_position,
            npc_positions,
            true,
        );
    }

    let offsets = [
        IVec2::new(0, 1),
        IVec2::new(1, 0),
        IVec2::new(0, -1),
        IVec2::new(-1, 0),
    ];

    let start_index = next_random_index(random_state, offsets.len());

    for offset_index in 0..offsets.len() {
        let delta = offsets[(start_index + offset_index) % offsets.len()];
        let target_position =
            TilePosition::new(tile_position.x + delta.x, tile_position.y + delta.y);

        if blocker_query
            .iter()
            .any(|blocker_position| *blocker_position == target_position)
        {
            continue;
        }

        if player_position.is_some_and(|player_position| player_position == target_position) {
            continue;
        }

        if npc_positions.iter().any(|(other_entity, other_position)| {
            *other_entity != entity && *other_position == target_position
        }) {
            continue;
        }

        if !behavior
            .bounds
            .contains(target_position.x, target_position.y)
        {
            continue;
        }

        return Some(target_position);
    }

    None
}

fn select_chase_target(
    entity: Entity,
    tile_position: TilePosition,
    hostile_behavior: Option<&HostileBehavior>,
    combat_target: Option<&CombatTarget>,
    player: Option<(Entity, TilePosition)>,
    commands: &mut Commands,
) -> Option<TilePosition> {
    let Some(hostile_behavior) = hostile_behavior else {
        return None;
    };
    let Some((player_entity, player_position)) = player else {
        commands.entity(entity).remove::<CombatTarget>();
        return None;
    };

    let distance = chebyshev_distance(tile_position, player_position);

    if combat_target.is_some_and(|target| target.entity == player_entity) {
        if distance > hostile_behavior.disengage_distance_tiles {
            commands.entity(entity).remove::<CombatTarget>();
            return None;
        }

        return Some(player_position);
    }

    if distance <= hostile_behavior.detect_distance_tiles {
        commands.entity(entity).insert(CombatTarget {
            entity: player_entity,
        });
        return Some(player_position);
    }

    None
}

fn choose_chase_step(
    entity: Entity,
    tile_position: TilePosition,
    chase_target: TilePosition,
    blocker_query: &Query<&TilePosition, (With<Collider>, Without<Npc>)>,
    player_position: Option<TilePosition>,
    npc_positions: &[(Entity, TilePosition)],
) -> Option<TilePosition> {
    if chebyshev_distance(tile_position, chase_target) <= 1 {
        return None;
    }

    choose_seek_step(
        entity,
        tile_position,
        chase_target,
        blocker_query,
        player_position,
        npc_positions,
        true,
    )
}

fn choose_seek_step(
    entity: Entity,
    tile_position: TilePosition,
    seek_target: TilePosition,
    blocker_query: &Query<&TilePosition, (With<Collider>, Without<Npc>)>,
    player_position: Option<TilePosition>,
    npc_positions: &[(Entity, TilePosition)],
    respect_player_tile: bool,
) -> Option<TilePosition> {
    let mut candidate_offsets = Vec::new();
    for y in -1..=1 {
        for x in -1..=1 {
            if x == 0 && y == 0 {
                continue;
            }

            candidate_offsets.push(IVec2::new(x, y));
        }
    }

    candidate_offsets.sort_by_key(|delta| {
        let candidate = TilePosition::new(tile_position.x + delta.x, tile_position.y + delta.y);
        (
            chebyshev_distance(candidate, seek_target),
            i32::from(delta.x != 0 && delta.y != 0),
        )
    });

    for delta in candidate_offsets {
        let target_position =
            TilePosition::new(tile_position.x + delta.x, tile_position.y + delta.y);
        if is_blocked_position(
            entity,
            target_position,
            blocker_query,
            player_position,
            npc_positions,
            respect_player_tile,
        ) {
            continue;
        }

        return Some(target_position);
    }

    None
}

fn is_blocked_position(
    entity: Entity,
    target_position: TilePosition,
    blocker_query: &Query<&TilePosition, (With<Collider>, Without<Npc>)>,
    player_position: Option<TilePosition>,
    npc_positions: &[(Entity, TilePosition)],
    respect_player_tile: bool,
) -> bool {
    if blocker_query
        .iter()
        .any(|blocker_position| *blocker_position == target_position)
    {
        return true;
    }

    if respect_player_tile
        && player_position.is_some_and(|player_position| player_position == target_position)
    {
        return true;
    }

    npc_positions.iter().any(|(other_entity, other_position)| {
        *other_entity != entity && *other_position == target_position
    })
}

fn chebyshev_distance(a: TilePosition, b: TilePosition) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

fn next_random_index(random_state: &mut RoamingRandomState, modulo: usize) -> usize {
    random_state.seed = random_state
        .seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1);
    ((random_state.seed >> 32) as usize) % modulo
}
