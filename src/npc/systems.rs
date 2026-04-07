use bevy::prelude::*;

use crate::npc::components::{Npc, RoamingBehavior, RoamingRandomState, RoamingStepTimer};
use crate::player::components::Player;
use crate::world::components::{Collider, TilePosition};

pub fn update_roaming_npcs(
    time: Res<Time>,
    blocker_query: Query<&TilePosition, (With<Collider>, Without<Npc>)>,
    player_query: Query<&TilePosition, (With<Player>, Without<Npc>)>,
    mut npc_query: Query<
        (
            Entity,
            &mut TilePosition,
            &RoamingBehavior,
            &mut RoamingStepTimer,
            &mut RoamingRandomState,
        ),
        (With<Npc>, Without<Player>),
    >,
) {
    let player_position = player_query.iter().next().copied();

    let npc_positions: Vec<(Entity, TilePosition)> = npc_query
        .iter()
        .map(|(entity, tile_position, ..)| (entity, *tile_position))
        .collect();

    for (entity, mut tile_position, behavior, mut timer, mut random_state) in &mut npc_query {
        timer.remaining_seconds = (timer.remaining_seconds - time.delta_secs()).max(0.0);
        if timer.remaining_seconds > 0.0 {
            continue;
        }

        if let Some(target_position) = choose_roaming_step(
            entity,
            *tile_position,
            behavior,
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
    random_state: &mut RoamingRandomState,
    blocker_query: &Query<&TilePosition, (With<Collider>, Without<Npc>)>,
    player_position: Option<TilePosition>,
    npc_positions: &[(Entity, TilePosition)],
) -> Option<TilePosition> {
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

        if !behavior
            .bounds
            .contains(target_position.x, target_position.y)
        {
            continue;
        }

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

        return Some(target_position);
    }

    None
}

fn next_random_index(random_state: &mut RoamingRandomState, modulo: usize) -> usize {
    random_state.seed = random_state
        .seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1);
    ((random_state.seed >> 32) as usize) % modulo
}
