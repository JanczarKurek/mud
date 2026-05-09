use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::combat::components::{AttackKind, AttackProfile, CombatTarget};
use crate::npc::components::{
    HostileBehavior, Npc, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
};
use crate::player::components::Player;
use crate::world::components::{Collider, Facing, SpaceId, SpaceResident, TilePosition};
use crate::world::direction::Direction;

/// Spatial index of static blocker tiles, rebuilt at the top of
/// `update_roaming_npcs`. Replaces a per-NPC × per-candidate-tile linear scan
/// of every collider in the world (~thousands), which produced a 20+ ms spike
/// every step interval when all NPCs synchronized on the same frame.
type BlockerIndex = HashSet<(SpaceId, TilePosition)>;
type NpcTileIndex = HashMap<(SpaceId, TilePosition), Entity>;

pub fn update_roaming_npcs(
    time: Res<Time>,
    blocker_query: Query<(&SpaceResident, &TilePosition), (With<Collider>, Without<Npc>)>,
    player_query: Query<(Entity, &SpaceResident, &TilePosition), (With<Player>, Without<Npc>)>,
    mut npc_query: Query<
        (
            Entity,
            &SpaceResident,
            &mut TilePosition,
            &RoamingBehavior,
            Option<&HostileBehavior>,
            Option<&AttackProfile>,
            Option<&mut CombatTarget>,
            &mut RoamingStepTimer,
            &mut RoamingRandomState,
            Option<&mut Facing>,
        ),
        (With<Npc>, Without<Player>),
    >,
    mut commands: Commands,
) {
    let players = player_query
        .iter()
        .map(|(entity, resident, tile_position)| (entity, resident.space_id, *tile_position))
        .collect::<Vec<_>>();

    let blockers: BlockerIndex = blocker_query
        .iter()
        .map(|(resident, position)| (resident.space_id, *position))
        .collect();

    let npc_tiles: NpcTileIndex = npc_query
        .iter()
        .map(|(entity, resident, tile_position, ..)| {
            ((resident.space_id, *tile_position), entity)
        })
        .collect();

    for (
        entity,
        resident,
        mut tile_position,
        behavior,
        hostile_behavior,
        attack_profile,
        combat_target,
        mut timer,
        mut random_state,
        mut facing,
    ) in &mut npc_query
    {
        timer.remaining_seconds = (timer.remaining_seconds - time.delta_secs()).max(0.0);
        if timer.remaining_seconds > 0.0 {
            continue;
        }

        let nearest_player = players
            .iter()
            .copied()
            .filter(|(_, space_id, _)| *space_id == resident.space_id)
            .min_by_key(|(_, _, position)| chebyshev_distance(*tile_position, *position));
        let player_position = nearest_player.map(|(_, _, position)| position);
        let chase_target = select_chase_target(
            entity,
            resident.space_id,
            *tile_position,
            hostile_behavior,
            combat_target.as_deref(),
            &players,
            &mut commands,
        );

        if let Some(target_position) = choose_roaming_step(
            entity,
            resident.space_id,
            *tile_position,
            behavior,
            hostile_behavior,
            attack_profile,
            chase_target,
            &mut random_state,
            &blockers,
            player_position,
            &npc_tiles,
        ) {
            let old_position = *tile_position;
            *tile_position = target_position;
            if let Some(direction) = Direction::from_delta(
                target_position.x - old_position.x,
                target_position.y - old_position.y,
            ) {
                if let Some(facing) = facing.as_mut() {
                    facing.0 = direction;
                }
            }
        }

        timer.remaining_seconds = behavior.step_interval_seconds;
    }
}

fn choose_roaming_step(
    entity: Entity,
    space_id: SpaceId,
    tile_position: TilePosition,
    behavior: &RoamingBehavior,
    hostile_behavior: Option<&HostileBehavior>,
    attack_profile: Option<&AttackProfile>,
    chase_target: Option<TilePosition>,
    random_state: &mut RoamingRandomState,
    blockers: &BlockerIndex,
    player_position: Option<TilePosition>,
    npc_tiles: &NpcTileIndex,
) -> Option<TilePosition> {
    if let Some(chase_target) = chase_target {
        if let (
            Some(AttackProfile {
                kind: AttackKind::Ranged { range_tiles },
            }),
            Some(hostile),
        ) = (attack_profile, hostile_behavior)
        {
            return choose_kiting_step(
                entity,
                space_id,
                tile_position,
                chase_target,
                *range_tiles,
                hostile.disengage_distance_tiles,
                blockers,
                player_position,
                npc_tiles,
            );
        }

        return choose_chase_step(
            entity,
            space_id,
            tile_position,
            chase_target,
            blockers,
            player_position,
            npc_tiles,
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
            tile_position.z,
        );

        return choose_seek_step(
            entity,
            space_id,
            tile_position,
            return_target,
            blockers,
            player_position,
            npc_tiles,
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
        let target_position = TilePosition::new(
            tile_position.x + delta.x,
            tile_position.y + delta.y,
            tile_position.z,
        );

        if blockers.contains(&(space_id, target_position)) {
            continue;
        }

        if player_position.is_some_and(|player_position| player_position == target_position) {
            continue;
        }

        if npc_tiles
            .get(&(space_id, target_position))
            .is_some_and(|other| *other != entity)
        {
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
    space_id: SpaceId,
    tile_position: TilePosition,
    hostile_behavior: Option<&HostileBehavior>,
    combat_target: Option<&CombatTarget>,
    players: &[(Entity, SpaceId, TilePosition)],
    commands: &mut Commands,
) -> Option<TilePosition> {
    let Some(hostile_behavior) = hostile_behavior else {
        return None;
    };
    let Some((player_entity, player_position)) = players
        .iter()
        .copied()
        .filter(|(_, player_space_id, _)| *player_space_id == space_id)
        .min_by_key(|(_, _, position)| chebyshev_distance(tile_position, *position))
        .map(|(entity, _, position)| (entity, position))
    else {
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
    space_id: SpaceId,
    tile_position: TilePosition,
    chase_target: TilePosition,
    blockers: &BlockerIndex,
    player_position: Option<TilePosition>,
    npc_tiles: &NpcTileIndex,
) -> Option<TilePosition> {
    if chebyshev_distance(tile_position, chase_target) <= 1 {
        return None;
    }

    choose_seek_step(
        entity,
        space_id,
        tile_position,
        chase_target,
        blockers,
        player_position,
        npc_tiles,
        true,
    )
}

fn choose_kiting_step(
    entity: Entity,
    space_id: SpaceId,
    tile_position: TilePosition,
    chase_target: TilePosition,
    range_tiles: i32,
    disengage_distance_tiles: i32,
    blockers: &BlockerIndex,
    player_position: Option<TilePosition>,
    npc_tiles: &NpcTileIndex,
) -> Option<TilePosition> {
    let preferred_cap = (disengage_distance_tiles - 1).max(0);
    let preferred = (range_tiles / 2).clamp(0, preferred_cap);
    let tolerance: i32 = 1;
    let distance = chebyshev_distance(tile_position, chase_target);

    if distance > preferred + tolerance {
        return choose_chase_step(
            entity,
            space_id,
            tile_position,
            chase_target,
            blockers,
            player_position,
            npc_tiles,
        );
    }

    if distance < preferred - tolerance {
        let away_goal = TilePosition::new(
            2 * tile_position.x - chase_target.x,
            2 * tile_position.y - chase_target.y,
            tile_position.z,
        );
        return choose_seek_step(
            entity,
            space_id,
            tile_position,
            away_goal,
            blockers,
            player_position,
            npc_tiles,
            true,
        );
    }

    None
}

fn choose_seek_step(
    entity: Entity,
    space_id: SpaceId,
    tile_position: TilePosition,
    seek_target: TilePosition,
    blockers: &BlockerIndex,
    player_position: Option<TilePosition>,
    npc_tiles: &NpcTileIndex,
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
        let candidate = TilePosition::new(
            tile_position.x + delta.x,
            tile_position.y + delta.y,
            tile_position.z,
        );
        (
            chebyshev_distance(candidate, seek_target),
            i32::from(delta.x != 0 && delta.y != 0),
        )
    });

    for delta in candidate_offsets {
        let target_position = TilePosition::new(
            tile_position.x + delta.x,
            tile_position.y + delta.y,
            tile_position.z,
        );
        if is_blocked_position(
            entity,
            space_id,
            target_position,
            blockers,
            player_position,
            npc_tiles,
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
    space_id: SpaceId,
    target_position: TilePosition,
    blockers: &BlockerIndex,
    player_position: Option<TilePosition>,
    npc_tiles: &NpcTileIndex,
    respect_player_tile: bool,
) -> bool {
    if blockers.contains(&(space_id, target_position)) {
        return true;
    }

    if respect_player_tile
        && player_position.is_some_and(|player_position| player_position == target_position)
    {
        return true;
    }

    npc_tiles
        .get(&(space_id, target_position))
        .is_some_and(|other| *other != entity)
}

fn chebyshev_distance(a: TilePosition, b: TilePosition) -> i32 {
    if a.z != b.z {
        return i32::MAX;
    }
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::combat::components::{AttackProfile, CombatTarget};
    use crate::npc::components::{
        HostileBehavior, Npc, RoamBounds, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
    };
    use crate::player::components::{
        ChatLog, Inventory, Player, PlayerId, PlayerIdentity, VitalStats,
    };
    use crate::world::components::{Collider, SpaceResident};

    const TEST_SPACE: crate::world::components::SpaceId = crate::world::components::SpaceId(0);

    fn spawn_player(app: &mut App, id: u64, position: TilePosition) -> Entity {
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(id)),
                Inventory::default(),
                ChatLog::default(),
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                position,
                VitalStats::full(10.0, 0.0),
            ))
            .id()
    }

    fn spawn_archer(app: &mut App, position: TilePosition, range: i32) -> Entity {
        app.world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                position,
                RoamingBehavior {
                    bounds: RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    step_interval_seconds: 0.1,
                },
                HostileBehavior {
                    detect_distance_tiles: 20,
                    disengage_distance_tiles: 20,
                },
                AttackProfile::ranged(range),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
            ))
            .id()
    }

    #[test]
    fn hostile_npc_targets_the_nearest_player() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        app.world_mut().spawn((
            Player,
            PlayerIdentity::new(PlayerId(1)),
            Inventory::default(),
            ChatLog::default(),
            SpaceResident {
                space_id: crate::world::components::SpaceId(0),
            },
            TilePosition::ground(5, 5),
            VitalStats::full(10.0, 0.0),
        ));
        let near_player = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(2)),
                Inventory::default(),
                ChatLog::default(),
                SpaceResident {
                    space_id: crate::world::components::SpaceId(0),
                },
                TilePosition::ground(2, 2),
                VitalStats::full(10.0, 0.0),
            ))
            .id();

        let npc = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: crate::world::components::SpaceId(0),
                },
                TilePosition::ground(1, 1),
                RoamingBehavior {
                    bounds: RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 10,
                        max_y: 10,
                    },
                    step_interval_seconds: 0.1,
                },
                HostileBehavior {
                    detect_distance_tiles: 10,
                    disengage_distance_tiles: 12,
                },
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
            ))
            .id();

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert_eq!(
            app.world().get::<CombatTarget>(npc).unwrap().entity,
            near_player
        );
    }

    #[test]
    fn archer_retreats_when_player_too_close() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        spawn_player(&mut app, 1, TilePosition::ground(5, 6));
        let archer = spawn_archer(&mut app, TilePosition::ground(5, 5), 6);

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        let archer_position = *app.world().get::<TilePosition>(archer).unwrap();
        assert!(
            chebyshev_distance(archer_position, TilePosition::ground(5, 6)) >= 2,
            "archer should retreat; ended at {archer_position:?}"
        );
    }

    #[test]
    fn archer_holds_at_preferred_distance() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        spawn_player(&mut app, 1, TilePosition::ground(5, 8));
        let archer = spawn_archer(&mut app, TilePosition::ground(5, 5), 6);

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert_eq!(
            *app.world().get::<TilePosition>(archer).unwrap(),
            TilePosition::ground(5, 5),
            "archer at preferred distance should stand still"
        );
    }

    #[test]
    fn archer_holds_within_dead_band() {
        for player_y in [7, 9] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);

            spawn_player(&mut app, 1, TilePosition::ground(5, player_y));
            let archer = spawn_archer(&mut app, TilePosition::ground(5, 5), 6);

            app.add_systems(Update, update_roaming_npcs);
            app.update();

            assert_eq!(
                *app.world().get::<TilePosition>(archer).unwrap(),
                TilePosition::ground(5, 5),
                "archer should hold when player at y={player_y} (distance inside dead-band)"
            );
        }
    }

    #[test]
    fn archer_chases_when_player_flees_past_band() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        spawn_player(&mut app, 1, TilePosition::ground(5, 10));
        let archer = spawn_archer(&mut app, TilePosition::ground(5, 5), 6);

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        let archer_position = *app.world().get::<TilePosition>(archer).unwrap();
        assert_eq!(
            chebyshev_distance(archer_position, TilePosition::ground(5, 10)),
            4,
            "archer should close one tile; ended at {archer_position:?}"
        );
    }

    #[test]
    fn archer_cornered_stands_still() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        spawn_player(&mut app, 1, TilePosition::ground(5, 6));
        let archer = spawn_archer(&mut app, TilePosition::ground(5, 5), 6);

        for (x, y) in [(4, 4), (5, 4), (6, 4), (4, 5), (6, 5), (4, 6), (6, 6)] {
            app.world_mut().spawn((
                Collider,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(x, y),
            ));
        }

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert_eq!(
            *app.world().get::<TilePosition>(archer).unwrap(),
            TilePosition::ground(5, 5),
            "cornered archer with no retreat tile should stand still"
        );
    }

    #[test]
    fn melee_npc_closes_to_adjacent() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        spawn_player(&mut app, 1, TilePosition::ground(5, 8));
        let npc = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(5, 5),
                RoamingBehavior {
                    bounds: RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    step_interval_seconds: 0.1,
                },
                HostileBehavior {
                    detect_distance_tiles: 20,
                    disengage_distance_tiles: 20,
                },
                AttackProfile::melee(),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
            ))
            .id();

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        let npc_position = *app.world().get::<TilePosition>(npc).unwrap();
        assert_eq!(
            chebyshev_distance(npc_position, TilePosition::ground(5, 8)),
            2,
            "melee NPC should close one tile; ended at {npc_position:?}"
        );
    }

    #[test]
    fn npc_does_not_chase_player_on_different_floor() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);

        // Player is one tile away but on floor 1; NPC is on floor 0 and must
        // not acquire them as a target (z-mismatched distance is infinite).
        spawn_player(&mut app, 1, TilePosition::new(5, 6, 1));
        let npc = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(5, 5),
                RoamingBehavior {
                    bounds: RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    step_interval_seconds: 0.1,
                },
                HostileBehavior {
                    detect_distance_tiles: 20,
                    disengage_distance_tiles: 20,
                },
                AttackProfile::melee(),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
            ))
            .id();

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "NPC should not target a player on a different floor"
        );
    }
}

fn next_random_index(random_state: &mut RoamingRandomState, modulo: usize) -> usize {
    random_state.seed = random_state
        .seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1);
    ((random_state.seed >> 32) as usize) % modulo
}
