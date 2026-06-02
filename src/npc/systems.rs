use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::prelude::*;

use crate::combat::components::{AttackKind, AttackProfile, CombatTarget};
use crate::game::shop::Shopkeeper;
use crate::magic::effects::MagicEffects;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents, SpeechBubbleStyle};
use crate::npc::components::{
    AiMemory, AiState, Barks, HostileBehavior, Npc, RoamingBehavior, RoamingRandomState,
    RoamingStepTimer,
};
use crate::world::components::OverworldObject;
use crate::player::components::Player;
use crate::world::components::{Collider, Facing, SpaceId, SpaceResident, TilePosition};
use crate::world::direction::Direction;

/// Shopkeepers stop wandering when a player is within this many tiles, so the
/// trade context menu and any open trade panel don't snap closed every time a
/// peaceful NPC takes a random step. Two tiles is one beyond the chebyshev-1
/// adjacency the trade flow already requires for `InitiateTrade`.
const SHOPKEEPER_PAUSE_RADIUS_TILES: i32 = 2;

/// Maximum A* node expansions before we give up and fall back to greedy.
/// In open terrain the Chebyshev heuristic keeps expansions close to
/// O(distance); the cap bounds worst-case routing around obstacles.
const ASTAR_EXPANSION_CAP: usize = 400;

/// Probability per Wander tick that an NPC with mutter lines speaks one. Low
/// by design — most ticks should be silent so the bubble overlay stays a
/// flavor punctuation rather than constant chatter.
const MUTTER_PROBABILITY: f32 = 0.05;

/// Minimum seconds between two bubbles from the same NPC. Caps spam even
/// when several rolls succeed in a row, and prevents an aggro bark from
/// being immediately stepped on by a mutter.
const BUBBLE_COOLDOWN_SECONDS: f32 = 8.0;

/// Spatial index of static blocker tiles, rebuilt at the top of
/// `update_roaming_npcs`. Replaces a per-NPC × per-candidate-tile linear scan
/// of every collider in the world (~thousands), which produced a 20+ ms spike
/// every step interval when all NPCs synchronized on the same frame.
type BlockerIndex = HashSet<(SpaceId, TilePosition)>;
type NpcTileIndex = HashMap<(SpaceId, TilePosition), Entity>;
type PlayerTileSet = HashSet<(SpaceId, TilePosition)>;

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
            &mut RoamingStepTimer,
            &mut RoamingRandomState,
            &mut AiState,
            &mut AiMemory,
            Option<&mut Facing>,
            Has<Shopkeeper>,
            Option<&MagicEffects>,
            Option<&Barks>,
            Option<&OverworldObject>,
        ),
        (With<Npc>, Without<Player>),
    >,
    mut pending_steps: ResMut<crate::world::step_triggers::PendingStepEvents>,
    mut ui_events: Option<ResMut<PendingGameUiEvents>>,
    mut commands: Commands,
) {
    let elapsed = time.elapsed_secs();

    let players: Vec<(Entity, SpaceId, TilePosition)> = player_query
        .iter()
        .map(|(entity, resident, tile_position)| (entity, resident.space_id, *tile_position))
        .collect();

    let blockers: BlockerIndex = blocker_query
        .iter()
        .map(|(resident, position)| (resident.space_id, *position))
        .collect();

    let npc_tiles: NpcTileIndex = npc_query
        .iter()
        .map(|(entity, resident, tile_position, ..)| ((resident.space_id, *tile_position), entity))
        .collect();

    let player_tiles: PlayerTileSet = players
        .iter()
        .map(|(_, space_id, position)| (*space_id, *position))
        .collect();

    for (
        entity,
        resident,
        mut tile_position,
        behavior,
        hostile_behavior,
        attack_profile,
        mut timer,
        mut random_state,
        mut ai_state,
        mut ai_memory,
        mut facing,
        is_shopkeeper,
        magic_effects,
        barks,
        overworld_object,
    ) in &mut npc_query
    {
        timer.remaining_seconds = (timer.remaining_seconds - time.delta_secs()).max(0.0);
        if timer.remaining_seconds > 0.0 {
            continue;
        }

        let slow_multiplier = magic_effects.map_or(1.0, |e| e.npc_step_multiplier());

        // Sleeping or paralyzed NPC: skip the AI tick entirely. Sleep wakes
        // on damage via `apply_pending_damage`; Paralyze only expires on its
        // own timer. The NPC keeps any existing CombatTarget so it re-engages
        // the moment the CC drops.
        if magic_effects.is_some_and(|effects| effects.is_asleep() || effects.is_paralyzed()) {
            timer.remaining_seconds =
                sample_step_interval(behavior, &mut random_state) * slow_multiplier;
            continue;
        }

        // Shopkeeper pause is orthogonal to the FSM: peaceful NPCs face the
        // nearest nearby player so context menus / trade UI stay live.
        if is_shopkeeper {
            let nearest = players
                .iter()
                .copied()
                .filter(|(_, space_id, _)| *space_id == resident.space_id)
                .min_by_key(|(_, _, position)| chebyshev_distance(*tile_position, *position))
                .map(|(_, _, position)| position);
            if let Some(target) = nearest {
                if chebyshev_distance(*tile_position, target) <= SHOPKEEPER_PAUSE_RADIUS_TILES {
                    if let Some(facing) = facing.as_mut() {
                        if let Some(direction) = Direction::from_delta(
                            target.x - tile_position.x,
                            target.y - tile_position.y,
                        ) {
                            if facing.0 != direction {
                                facing.0 = direction;
                            }
                        }
                    }
                    timer.remaining_seconds =
                        sample_step_interval(behavior, &mut random_state) * slow_multiplier;
                    continue;
                }
            }
        }

        let outcome = step_ai(StepAiInput {
            entity,
            space_id: resident.space_id,
            tile_position: *tile_position,
            current_state: *ai_state,
            memory: *ai_memory,
            behavior,
            hostile_behavior,
            attack_profile,
            players: &players,
            blockers: &blockers,
            npc_tiles: &npc_tiles,
            player_tiles: &player_tiles,
            random_state: &mut random_state,
            elapsed,
            barks,
        });

        *ai_state = outcome.next_state;
        *ai_memory = outcome.next_memory;

        match outcome.target {
            TargetChange::Set(target) => {
                commands
                    .entity(entity)
                    .insert(CombatTarget { entity: target });
            }
            TargetChange::Clear => {
                commands.entity(entity).remove::<CombatTarget>();
            }
            TargetChange::Keep => {}
        }

        if let Some(new_position) = outcome.move_to {
            let old_position = *tile_position;
            *tile_position = new_position;
            pending_steps.push(crate::world::step_triggers::StepEvent {
                entity,
                space_id: resident.space_id,
                tile: new_position,
            });
            if let Some(direction) = Direction::from_delta(
                new_position.x - old_position.x,
                new_position.y - old_position.y,
            ) {
                if let Some(facing) = facing.as_mut() {
                    facing.0 = direction;
                }
            }
        }

        if let Some(bark) = outcome.bark {
            if let (Some(ui_events), Some(overworld_object)) =
                (ui_events.as_deref_mut(), overworld_object)
            {
                ui_events.push_broadcast(GameUiEvent::SpeechBubble {
                    speaker_object_id: overworld_object.object_id,
                    text: bark.text,
                    style: bark.style,
                });
                ai_memory.last_bark_seconds = elapsed;
            }
        }

        let base_interval = sample_step_interval(behavior, &mut random_state);
        let interval = if outcome.idle_pause {
            base_interval * 1.5
        } else {
            base_interval
        };
        timer.remaining_seconds = interval * slow_multiplier;
    }
}

// ---------------------------------------------------------------------------
// FSM
// ---------------------------------------------------------------------------

struct StepAiInput<'a> {
    entity: Entity,
    space_id: SpaceId,
    tile_position: TilePosition,
    current_state: AiState,
    memory: AiMemory,
    behavior: &'a RoamingBehavior,
    hostile_behavior: Option<&'a HostileBehavior>,
    attack_profile: Option<&'a AttackProfile>,
    players: &'a [(Entity, SpaceId, TilePosition)],
    blockers: &'a BlockerIndex,
    npc_tiles: &'a NpcTileIndex,
    player_tiles: &'a PlayerTileSet,
    random_state: &'a mut RoamingRandomState,
    elapsed: f32,
    barks: Option<&'a Barks>,
}

struct PendingBark {
    text: String,
    style: SpeechBubbleStyle,
}

struct AiOutcome {
    next_state: AiState,
    next_memory: AiMemory,
    target: TargetChange,
    move_to: Option<TilePosition>,
    idle_pause: bool,
    bark: Option<PendingBark>,
}

enum TargetChange {
    Keep,
    Set(Entity),
    Clear,
}

fn step_ai(mut input: StepAiInput<'_>) -> AiOutcome {
    match input.current_state {
        AiState::Wander => tick_wander(&mut input),
        AiState::Alert {
            last_seen,
            expires_at_seconds,
        } => tick_alert(&mut input, last_seen, expires_at_seconds),
        AiState::Pursue { target } => tick_pursue_or_engage(&mut input, target, false),
        AiState::Engage { target } => tick_pursue_or_engage(&mut input, target, true),
    }
}

fn tick_wander(input: &mut StepAiInput<'_>) -> AiOutcome {
    // Try to acquire a target. On fresh aggro, execute the corresponding
    // pursue/engage action immediately rather than burning a tick to "wake
    // up" — players expect a chasing NPC to actually take its first step.
    if let Some(hostile) = input.hostile_behavior {
        if let Some((target_entity, _)) = nearest_visible_player(
            input.tile_position,
            input.space_id,
            hostile,
            input.players,
            input.blockers,
            hostile.detect_distance_tiles,
        ) {
            let mut outcome = tick_pursue_or_engage(input, target_entity, false);
            // We just transitioned from Wander, so there's no prior
            // CombatTarget — ensure we mark the component regardless of what
            // the pursue/engage helper decided about target re-affirmation.
            outcome.target = TargetChange::Set(target_entity);
            outcome.bark = pick_bark(input, BarkKind::Aggro);
            return outcome;
        }
    }

    // Low-probability ambient mutter while wandering. Gated by a per-NPC
    // cooldown so an unlucky streak doesn't produce a chatty NPC.
    let mutter = if next_random_f32(input.random_state) < MUTTER_PROBABILITY {
        pick_bark(input, BarkKind::Mutter)
    } else {
        None
    };

    // No target — wander with momentum + idle pauses.
    let roll = next_random_f32(input.random_state);
    if roll < input.behavior.idle_pause_chance {
        return AiOutcome {
            next_state: AiState::Wander,
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: TargetChange::Keep,
            move_to: None,
            idle_pause: true,
            bark: mutter,
        };
    }

    // If outside bounds, walk back inside (greedy 8-way like before).
    if !input
        .behavior
        .bounds
        .contains(input.tile_position.x, input.tile_position.y)
    {
        let return_target = TilePosition::new(
            input
                .tile_position
                .x
                .clamp(input.behavior.bounds.min_x, input.behavior.bounds.max_x),
            input
                .tile_position
                .y
                .clamp(input.behavior.bounds.min_y, input.behavior.bounds.max_y),
            input.tile_position.z,
        );
        let step = choose_seek_step(
            input.entity,
            input.space_id,
            input.tile_position,
            return_target,
            input.blockers,
            input.npc_tiles,
            Some(input.player_tiles),
            None,
        );
        return AiOutcome {
            next_state: AiState::Wander,
            next_memory: AiMemory {
                last_step: step
                    .map(|p| IVec2::new(p.x - input.tile_position.x, p.y - input.tile_position.y)),
                ..input.memory
            },
            target: TargetChange::Keep,
            move_to: step,
            idle_pause: false,
            bark: mutter,
        };
    }

    // Pick a cardinal direction weighted by momentum bias.
    let direction = pick_wander_direction(
        input.random_state,
        input.behavior.momentum_bias,
        input.memory.last_step,
    );

    // Try the picked direction, then fall back to the others in weight order.
    let ordered = order_cardinals_by_preference(direction);
    for delta in ordered {
        let target_position = TilePosition::new(
            input.tile_position.x + delta.x,
            input.tile_position.y + delta.y,
            input.tile_position.z,
        );
        if !input
            .behavior
            .bounds
            .contains(target_position.x, target_position.y)
        {
            continue;
        }
        if is_blocked_position(
            input.entity,
            input.space_id,
            target_position,
            input.blockers,
            input.npc_tiles,
            Some(input.player_tiles),
            None,
        ) {
            continue;
        }
        return AiOutcome {
            next_state: AiState::Wander,
            next_memory: AiMemory {
                last_step: Some(delta),
                ..input.memory
            },
            target: TargetChange::Keep,
            move_to: Some(target_position),
            idle_pause: false,
            bark: mutter,
        };
    }

    // Fully boxed in — stand still, clear momentum so we re-roll next tick.
    AiOutcome {
        next_state: AiState::Wander,
        next_memory: AiMemory {
            last_step: None,
            ..input.memory
        },
        target: TargetChange::Keep,
        move_to: None,
        idle_pause: false,
        bark: mutter,
    }
}

fn tick_alert(
    input: &mut StepAiInput<'_>,
    last_seen: TilePosition,
    expires_at_seconds: f32,
) -> AiOutcome {
    // While alert, re-detect at the *engage* radius (hysteresis). Lets a
    // briefly-hidden player snap back into pursuit before the alert decays.
    // Act on the new state immediately, same as Wander → Pursue.
    if let Some(hostile) = input.hostile_behavior {
        if let Some((target_entity, _)) = nearest_visible_player(
            input.tile_position,
            input.space_id,
            hostile,
            input.players,
            input.blockers,
            hostile.disengage_distance_tiles,
        ) {
            let mut outcome = tick_pursue_or_engage(input, target_entity, false);
            outcome.target = TargetChange::Set(target_entity);
            return outcome;
        }
    }

    // Expired — drop back to Wander.
    if input.elapsed >= expires_at_seconds {
        return AiOutcome {
            next_state: AiState::Wander,
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: TargetChange::Keep,
            move_to: None,
            idle_pause: false,
            bark: None,
        };
    }

    // Walk toward last-seen tile via A*; fall back to greedy if blocked off.
    let next = astar_next_step(
        input.entity,
        input.space_id,
        input.tile_position,
        last_seen,
        input.blockers,
        input.npc_tiles,
        input.player_tiles,
        None,
    )
    .or_else(|| {
        choose_seek_step(
            input.entity,
            input.space_id,
            input.tile_position,
            last_seen,
            input.blockers,
            input.npc_tiles,
            Some(input.player_tiles),
            None,
        )
    });

    // If we've arrived at last_seen and still nothing visible, let the alert
    // timer run out naturally on subsequent ticks — keep the state intact.
    AiOutcome {
        next_state: AiState::Alert {
            last_seen,
            expires_at_seconds,
        },
        next_memory: AiMemory {
            last_step: None,
            ..input.memory
        },
        target: TargetChange::Keep,
        move_to: next,
        idle_pause: false,
        bark: None,
    }
}

fn tick_pursue_or_engage(input: &mut StepAiInput<'_>, target: Entity, engaged: bool) -> AiOutcome {
    // Validate target still exists and is in the same space.
    let Some((_, target_space, target_pos)) =
        input.players.iter().copied().find(|(e, _, _)| *e == target)
    else {
        return AiOutcome {
            next_state: AiState::Wander,
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: TargetChange::Clear,
            move_to: None,
            idle_pause: false,
            bark: None,
        };
    };
    if target_space != input.space_id {
        return AiOutcome {
            next_state: AiState::Wander,
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: TargetChange::Clear,
            move_to: None,
            idle_pause: false,
            bark: None,
        };
    }

    let Some(hostile) = input.hostile_behavior else {
        return AiOutcome {
            next_state: AiState::Wander,
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: TargetChange::Clear,
            move_to: None,
            idle_pause: false,
            bark: None,
        };
    };

    let distance = chebyshev_distance(input.tile_position, target_pos);

    // Leash: too far → drop to Alert with last-seen.
    if distance > hostile.disengage_distance_tiles {
        return AiOutcome {
            next_state: AiState::Alert {
                last_seen: target_pos,
                expires_at_seconds: input.elapsed + hostile.alert_duration_seconds,
            },
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: TargetChange::Clear,
            move_to: None,
            idle_pause: false,
            bark: None,
        };
    }

    // Line of sight maintenance.
    if hostile.requires_line_of_sight
        && !has_line_of_sight(
            input.tile_position,
            target_pos,
            input.space_id,
            input.blockers,
        )
    {
        return AiOutcome {
            next_state: AiState::Alert {
                last_seen: target_pos,
                expires_at_seconds: input.elapsed + hostile.alert_duration_seconds,
            },
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: TargetChange::Clear,
            move_to: None,
            idle_pause: false,
            bark: None,
        };
    }

    let attack_range = attack_range_for(input.attack_profile);
    let now_engaged = distance <= attack_range;
    let next_target = if engaged != now_engaged {
        TargetChange::Set(target) // Re-affirm; cheap, keeps CombatTarget present.
    } else {
        TargetChange::Keep
    };

    if !now_engaged {
        // Pursue: A* toward target, target tile treated as walkable for the
        // pathfinder so it doesn't dead-end against the player's own tile.
        let move_to = astar_next_step(
            input.entity,
            input.space_id,
            input.tile_position,
            target_pos,
            input.blockers,
            input.npc_tiles,
            input.player_tiles,
            Some(target_pos),
        )
        .or_else(|| {
            choose_seek_step(
                input.entity,
                input.space_id,
                input.tile_position,
                target_pos,
                input.blockers,
                input.npc_tiles,
                Some(input.player_tiles),
                Some(target_pos),
            )
        });
        return AiOutcome {
            next_state: AiState::Pursue { target },
            next_memory: AiMemory {
                last_step: None,
                ..input.memory
            },
            target: next_target,
            move_to,
            idle_pause: false,
            bark: None,
        };
    }

    // Engaged: melee holds; ranged kites.
    let move_to = match input.attack_profile.map(|p| p.kind) {
        Some(AttackKind::Ranged { range_tiles }) => kite_step(
            input.entity,
            input.space_id,
            input.tile_position,
            target_pos,
            range_tiles,
            hostile.disengage_distance_tiles,
            input.blockers,
            input.npc_tiles,
            input.player_tiles,
        ),
        _ => None, // Melee: stand adjacent.
    };

    AiOutcome {
        next_state: AiState::Engage { target },
        next_memory: AiMemory {
            last_step: None,
            ..input.memory
        },
        target: next_target,
        move_to,
        idle_pause: false,
        bark: None,
    }
}

fn nearest_visible_player(
    tile_position: TilePosition,
    space_id: SpaceId,
    hostile: &HostileBehavior,
    players: &[(Entity, SpaceId, TilePosition)],
    blockers: &BlockerIndex,
    radius: i32,
) -> Option<(Entity, TilePosition)> {
    players
        .iter()
        .copied()
        .filter(|(_, player_space_id, _)| *player_space_id == space_id)
        .filter(|(_, _, position)| chebyshev_distance(tile_position, *position) <= radius)
        .filter(|(_, _, position)| {
            !hostile.requires_line_of_sight
                || has_line_of_sight(tile_position, *position, space_id, blockers)
        })
        .min_by_key(|(_, _, position)| chebyshev_distance(tile_position, *position))
        .map(|(entity, _, position)| (entity, position))
}

fn attack_range_for(profile: Option<&AttackProfile>) -> i32 {
    match profile.map(|p| p.kind) {
        Some(AttackKind::Ranged { range_tiles }) => range_tiles.max(1),
        _ => 1,
    }
}

#[derive(Clone, Copy)]
enum BarkKind {
    Aggro,
    Mutter,
}

/// Pull a random utterance from the NPC's `Barks` pool of the requested
/// kind, honoring the per-NPC cooldown. Returns `None` if the pool is empty
/// or the cooldown hasn't elapsed.
fn pick_bark(input: &mut StepAiInput<'_>, kind: BarkKind) -> Option<PendingBark> {
    let barks = input.barks?;
    let pool = match kind {
        BarkKind::Aggro => &barks.aggro,
        BarkKind::Mutter => &barks.mutter,
    };
    if pool.is_empty() {
        return None;
    }
    if input.elapsed - input.memory.last_bark_seconds < BUBBLE_COOLDOWN_SECONDS {
        return None;
    }
    let pick = (next_random_f32(input.random_state) * pool.len() as f32) as usize;
    let pick = pick.min(pool.len() - 1);
    Some(PendingBark {
        text: pool[pick].clone(),
        style: match kind {
            BarkKind::Aggro => SpeechBubbleStyle::Bark,
            BarkKind::Mutter => SpeechBubbleStyle::Mutter,
        },
    })
}

// ---------------------------------------------------------------------------
// Movement helpers
// ---------------------------------------------------------------------------

fn kite_step(
    entity: Entity,
    space_id: SpaceId,
    tile_position: TilePosition,
    target_pos: TilePosition,
    range_tiles: i32,
    disengage_distance_tiles: i32,
    blockers: &BlockerIndex,
    npc_tiles: &NpcTileIndex,
    player_tiles: &PlayerTileSet,
) -> Option<TilePosition> {
    let preferred_cap = (disengage_distance_tiles - 1).max(0);
    let preferred = (range_tiles - 1).max(2).min(preferred_cap);
    let tolerance: i32 = 1;
    let distance = chebyshev_distance(tile_position, target_pos);

    if distance > preferred + tolerance {
        return astar_next_step(
            entity,
            space_id,
            tile_position,
            target_pos,
            blockers,
            npc_tiles,
            player_tiles,
            Some(target_pos),
        )
        .or_else(|| {
            choose_seek_step(
                entity,
                space_id,
                tile_position,
                target_pos,
                blockers,
                npc_tiles,
                Some(player_tiles),
                Some(target_pos),
            )
        });
    }

    if distance < preferred - tolerance {
        // Retreat: mirror our position around the target and seek that tile.
        let away_goal = TilePosition::new(
            2 * tile_position.x - target_pos.x,
            2 * tile_position.y - target_pos.y,
            tile_position.z,
        );
        let retreat = choose_seek_step(
            entity,
            space_id,
            tile_position,
            away_goal,
            blockers,
            npc_tiles,
            Some(player_tiles),
            None,
        );
        if retreat.is_some() {
            return retreat;
        }

        // Cornered — try to strafe: any 8-neighbor that maintains or grows
        // distance to the target. Beats the old "stand still and die".
        return strafe_step(
            entity,
            space_id,
            tile_position,
            target_pos,
            blockers,
            npc_tiles,
            player_tiles,
        );
    }

    None
}

fn strafe_step(
    entity: Entity,
    space_id: SpaceId,
    tile_position: TilePosition,
    target_pos: TilePosition,
    blockers: &BlockerIndex,
    npc_tiles: &NpcTileIndex,
    player_tiles: &PlayerTileSet,
) -> Option<TilePosition> {
    let current_distance = chebyshev_distance(tile_position, target_pos);
    let mut best: Option<(TilePosition, i32)> = None;
    for y in -1..=1 {
        for x in -1..=1 {
            if x == 0 && y == 0 {
                continue;
            }
            let candidate =
                TilePosition::new(tile_position.x + x, tile_position.y + y, tile_position.z);
            if is_blocked_position(
                entity,
                space_id,
                candidate,
                blockers,
                npc_tiles,
                Some(player_tiles),
                None,
            ) {
                continue;
            }
            let new_distance = chebyshev_distance(candidate, target_pos);
            if new_distance < current_distance {
                continue; // Don't strafe closer.
            }
            if best.is_none_or(|(_, d)| new_distance > d) {
                best = Some((candidate, new_distance));
            }
        }
    }
    best.map(|(pos, _)| pos)
}

/// Greedy 8-way seek (kept as a fast fallback when A* runs out of budget).
fn choose_seek_step(
    entity: Entity,
    space_id: SpaceId,
    tile_position: TilePosition,
    seek_target: TilePosition,
    blockers: &BlockerIndex,
    npc_tiles: &NpcTileIndex,
    player_tiles: Option<&PlayerTileSet>,
    goal_override: Option<TilePosition>,
) -> Option<TilePosition> {
    let mut candidate_offsets = Vec::with_capacity(8);
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
            npc_tiles,
            player_tiles,
            goal_override,
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
    npc_tiles: &NpcTileIndex,
    player_tiles: Option<&PlayerTileSet>,
    goal_override: Option<TilePosition>,
) -> bool {
    if goal_override == Some(target_position) {
        return false;
    }
    if blockers.contains(&(space_id, target_position)) {
        return true;
    }
    if let Some(set) = player_tiles {
        if set.contains(&(space_id, target_position)) {
            return true;
        }
    }
    npc_tiles
        .get(&(space_id, target_position))
        .is_some_and(|other| *other != entity)
}

// ---------------------------------------------------------------------------
// A* (8-direction, Chebyshev heuristic)
// ---------------------------------------------------------------------------

#[derive(Eq, PartialEq)]
struct AstarNode {
    f: i32,
    counter: u32,
    pos: TilePosition,
}

impl Ord for AstarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower f wins; counter is a stable tiebreaker so equal-f nodes
        // pop in insertion order rather than producing a panicking
        // partial_cmp on TilePosition.
        self.f.cmp(&other.f).then(self.counter.cmp(&other.counter))
    }
}

impl PartialOrd for AstarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn astar_next_step(
    entity: Entity,
    space_id: SpaceId,
    start: TilePosition,
    goal: TilePosition,
    blockers: &BlockerIndex,
    npc_tiles: &NpcTileIndex,
    player_tiles: &PlayerTileSet,
    goal_override: Option<TilePosition>,
) -> Option<TilePosition> {
    if start == goal {
        return None;
    }
    if start.z != goal.z {
        return None;
    }

    let mut open: BinaryHeap<Reverse<AstarNode>> = BinaryHeap::new();
    let mut g_score: HashMap<TilePosition, i32> = HashMap::new();
    let mut came_from: HashMap<TilePosition, TilePosition> = HashMap::new();

    g_score.insert(start, 0);
    let mut counter: u32 = 0;
    open.push(Reverse(AstarNode {
        f: chebyshev_distance(start, goal),
        counter,
        pos: start,
    }));

    let mut expansions = 0usize;
    while let Some(Reverse(AstarNode { pos: current, .. })) = open.pop() {
        if current == goal {
            // Reconstruct path: walk came_from from goal until a node whose
            // parent is start; return that node.
            let mut node = current;
            while let Some(&parent) = came_from.get(&node) {
                if parent == start {
                    return Some(node);
                }
                node = parent;
            }
            // current == start case is handled by the early return above.
            return None;
        }

        expansions += 1;
        if expansions > ASTAR_EXPANSION_CAP {
            return None;
        }

        let current_g = *g_score.get(&current).unwrap_or(&i32::MAX);

        // Push neighbors in goal-direction-preferred order so the priority
        // queue's insertion-order tiebreaker resolves equal-f ties toward
        // the goal. Row-major iteration biased ties toward (-1,-1), which
        // made goblins zigzag through south-west when pursuing a player to
        // the north-west.
        let gdx = (goal.x - current.x).signum();
        let gdy = (goal.y - current.y).signum();
        let mut deltas: [(i32, i32); 8] = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        deltas.sort_by_key(|&(ddx, ddy)| neighbor_alignment_penalty(ddx, ddy, gdx, gdy));

        for (dx, dy) in deltas {
            let neighbor = TilePosition::new(current.x + dx, current.y + dy, current.z);
            if is_blocked_position(
                entity,
                space_id,
                neighbor,
                blockers,
                npc_tiles,
                Some(player_tiles),
                goal_override,
            ) {
                continue;
            }
            let tentative_g = current_g + 1; // Chebyshev: uniform cost.
            let existing = g_score.get(&neighbor).copied().unwrap_or(i32::MAX);
            if tentative_g < existing {
                came_from.insert(neighbor, current);
                g_score.insert(neighbor, tentative_g);
                let f = tentative_g + chebyshev_distance(neighbor, goal);
                counter = counter.wrapping_add(1);
                open.push(Reverse(AstarNode {
                    f,
                    counter,
                    pos: neighbor,
                }));
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Line of sight (Bresenham across blocker tiles)
// ---------------------------------------------------------------------------

fn has_line_of_sight(
    from: TilePosition,
    to: TilePosition,
    space_id: SpaceId,
    blockers: &BlockerIndex,
) -> bool {
    if from.z != to.z {
        return false;
    }
    if from == to {
        return true;
    }

    let mut x = from.x;
    let mut y = from.y;
    let dx = (to.x - x).abs();
    let dy = (to.y - y).abs();
    let sx = if from.x < to.x { 1 } else { -1 };
    let sy = if from.y < to.y { 1 } else { -1 };
    let mut err = dx - dy;

    loop {
        if x == to.x && y == to.y {
            return true;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
        if x == to.x && y == to.y {
            return true;
        }
        // Don't treat the source or destination as blocking themselves.
        let here = TilePosition::new(x, y, from.z);
        if blockers.contains(&(space_id, here)) {
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// Wander direction sampling
// ---------------------------------------------------------------------------

const CARDINALS: [IVec2; 4] = [
    IVec2::new(0, 1),
    IVec2::new(1, 0),
    IVec2::new(0, -1),
    IVec2::new(-1, 0),
];

fn pick_wander_direction(
    random_state: &mut RoamingRandomState,
    momentum_bias: f32,
    last_step: Option<IVec2>,
) -> IVec2 {
    let last = last_step.and_then(|delta| {
        // Only cardinal lasts are meaningful for momentum; ignore diagonals
        // that might have been left over from a return-to-bounds 8-way step.
        if (delta.x == 0) ^ (delta.y == 0) {
            Some(delta)
        } else {
            None
        }
    });

    let Some(last_step) = last else {
        // No momentum hint — uniform random over cardinals.
        let idx = next_random_index(random_state, CARDINALS.len());
        return CARDINALS[idx];
    };

    let mut weights = [0.0f32; 4];
    let off = 1.0 - momentum_bias.clamp(0.0, 1.0);
    let perpendicular_each = off * 0.4;
    let reverse = off * 0.2;
    for (i, dir) in CARDINALS.iter().enumerate() {
        weights[i] = if *dir == last_step {
            momentum_bias.clamp(0.0, 1.0)
        } else if *dir == -last_step {
            reverse
        } else {
            perpendicular_each
        };
    }

    let roll = next_random_f32(random_state);
    let mut acc = 0.0;
    for (i, w) in weights.iter().enumerate() {
        acc += *w;
        if roll <= acc {
            return CARDINALS[i];
        }
    }
    CARDINALS[3]
}

/// Reorder cardinals so that the preferred direction comes first, then
/// perpendiculars (random tiebreaker), then reverse last. Used when the
/// picked direction is blocked.
fn order_cardinals_by_preference(preferred: IVec2) -> [IVec2; 4] {
    let reverse = -preferred;
    let mut out = [preferred, IVec2::ZERO, IVec2::ZERO, reverse];
    let mut idx = 1;
    for dir in CARDINALS {
        if dir != preferred && dir != reverse {
            out[idx] = dir;
            idx += 1;
        }
    }
    out
}

fn sample_step_interval(behavior: &RoamingBehavior, random_state: &mut RoamingRandomState) -> f32 {
    let base = behavior.step_interval_seconds.max(0.05);
    let jitter_range = behavior.step_interval_jitter_seconds.max(0.0);
    if jitter_range <= 0.0 {
        return base;
    }
    base + next_random_f32(random_state) * jitter_range
}

/// Lower penalty = preferred neighbor. Returns 0 for a step that matches the
/// goal direction on every active axis, with rising penalties for perpendicular
/// and anti-aligned moves. When the goal sits on the same row or column, the
/// off-axis cost just rewards staying on that line.
fn neighbor_alignment_penalty(ddx: i32, ddy: i32, gdx: i32, gdy: i32) -> i32 {
    let ax = if gdx == 0 {
        ddx.abs()
    } else if ddx == gdx {
        0
    } else if ddx == 0 {
        1
    } else {
        2
    };
    let ay = if gdy == 0 {
        ddy.abs()
    } else if ddy == gdy {
        0
    } else if ddy == 0 {
        1
    } else {
        2
    };
    ax + ay
}

fn chebyshev_distance(a: TilePosition, b: TilePosition) -> i32 {
    if a.z != b.z {
        return i32::MAX;
    }
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

fn next_random_index(random_state: &mut RoamingRandomState, modulo: usize) -> usize {
    advance_rng(random_state);
    ((random_state.seed >> 32) as usize) % modulo
}

fn next_random_f32(random_state: &mut RoamingRandomState) -> f32 {
    advance_rng(random_state);
    // High 24 bits → uniform [0, 1).
    let bits = (random_state.seed >> 40) as u32 & 0x00FF_FFFF;
    bits as f32 / 16_777_216.0
}

fn advance_rng(random_state: &mut RoamingRandomState) {
    random_state.seed = random_state
        .seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::combat::components::{AttackProfile, CombatTarget};
    use crate::npc::components::{
        AiMemory, AiState, HostileBehavior, Npc, RoamBounds, RoamingBehavior, RoamingRandomState,
        RoamingStepTimer,
    };
    use crate::player::components::{
        ChatLog, Inventory, Player, PlayerId, PlayerIdentity, VitalStats,
    };
    use crate::world::components::{Collider, SpaceResident};

    const TEST_SPACE: crate::world::components::SpaceId = crate::world::components::SpaceId(0);

    fn default_roaming(bounds: RoamBounds, step: f32) -> RoamingBehavior {
        RoamingBehavior {
            bounds,
            step_interval_seconds: step,
            step_interval_jitter_seconds: 0.0,
            idle_pause_chance: 0.0,
            momentum_bias: 0.6,
        }
    }

    fn default_hostile(detect: i32, disengage: i32) -> HostileBehavior {
        HostileBehavior {
            detect_distance_tiles: detect,
            disengage_distance_tiles: disengage,
            alert_duration_seconds: 4.0,
            requires_line_of_sight: false, // most tests don't care about LoS
        }
    }

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
                default_roaming(
                    RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    0.1,
                ),
                default_hostile(20, 20),
                AttackProfile::ranged(range),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
                AiState::default(),
                AiMemory::default(),
            ))
            .id()
    }

    fn spawn_melee(app: &mut App, position: TilePosition) -> Entity {
        app.world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                position,
                default_roaming(
                    RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    0.1,
                ),
                default_hostile(20, 20),
                AttackProfile::melee(),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
                AiState::default(),
                AiMemory::default(),
            ))
            .id()
    }

    #[test]
    fn hostile_npc_targets_the_nearest_player() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 5));
        let near_player = spawn_player(&mut app, 2, TilePosition::ground(2, 2));
        let npc = spawn_melee(&mut app, TilePosition::ground(1, 1));

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
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

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
        // With range=6, preferred = (6-1).max(2) = 5. Tolerance 1.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 10));
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
        // preferred = 5; dead-band = [4, 6].
        for player_y in [9, 10, 11] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

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
        // preferred = 5, tolerance 1 → archer chases at distance > 6.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 12));
        let archer = spawn_archer(&mut app, TilePosition::ground(5, 5), 6);

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        let archer_position = *app.world().get::<TilePosition>(archer).unwrap();
        assert_eq!(
            chebyshev_distance(archer_position, TilePosition::ground(5, 12)),
            6,
            "archer should close one tile; ended at {archer_position:?}"
        );
    }

    #[test]
    fn archer_cornered_stands_still_or_strafes() {
        // Player adjacent, all retreat tiles blocked. With strafe-fallback,
        // the archer may strafe to a tile that maintains current distance.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

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

        // No retreat tile, no strafe tile → stand still.
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
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 8));
        let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

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
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        // Player on floor 1 (raw z=2 in half-block units), NPC on ground floor.
        spawn_player(&mut app, 1, TilePosition::new(5, 6, 2));
        let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "NPC should not target a player on a different floor"
        );
    }

    #[test]
    fn idle_pause_skips_step() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

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
                    step_interval_jitter_seconds: 0.0,
                    idle_pause_chance: 1.0, // always pause
                    momentum_bias: 0.6,
                },
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
                AiState::default(),
                AiMemory::default(),
            ))
            .id();

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert_eq!(
            *app.world().get::<TilePosition>(npc).unwrap(),
            TilePosition::ground(5, 5),
            "NPC with idle_pause_chance=1.0 should never move"
        );
    }

    #[test]
    fn wander_momentum_biases_continue_direction() {
        // With momentum_bias=1.0 and last_step=(0,1), the NPC must continue
        // moving in the same direction.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

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
                    step_interval_jitter_seconds: 0.0,
                    idle_pause_chance: 0.0,
                    momentum_bias: 1.0,
                },
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
                AiState::default(),
                AiMemory {
                    last_step: Some(IVec2::new(0, 1)),
                    ..Default::default()
                },
            ))
            .id();

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert_eq!(
            *app.world().get::<TilePosition>(npc).unwrap(),
            TilePosition::ground(5, 6),
            "NPC with momentum_bias=1.0 and last_step=(0,1) should continue up"
        );
    }

    #[test]
    fn los_blocks_aggro_through_wall() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 8));
        let npc = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(5, 5),
                default_roaming(
                    RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    0.1,
                ),
                HostileBehavior {
                    detect_distance_tiles: 20,
                    disengage_distance_tiles: 20,
                    alert_duration_seconds: 4.0,
                    requires_line_of_sight: true,
                },
                AttackProfile::melee(),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
                AiState::default(),
                AiMemory::default(),
            ))
            .id();

        // Wall directly between NPC (5,5) and player (5,8).
        for y in [6, 7] {
            app.world_mut().spawn((
                Collider,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(5, y),
            ));
        }

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "NPC with LoS required should not aggro through walls"
        );
        assert!(
            matches!(*app.world().get::<AiState>(npc).unwrap(), AiState::Wander),
            "state should remain Wander"
        );
    }

    #[test]
    fn los_allows_aggro_with_clear_line() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 8));
        let npc = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(5, 5),
                default_roaming(
                    RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    0.1,
                ),
                HostileBehavior {
                    detect_distance_tiles: 20,
                    disengage_distance_tiles: 20,
                    alert_duration_seconds: 4.0,
                    requires_line_of_sight: true,
                },
                AttackProfile::melee(),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
                AiState::default(),
                AiMemory::default(),
            ))
            .id();

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert!(
            app.world().get::<CombatTarget>(npc).is_some(),
            "NPC with clear LoS should aggro"
        );
    }

    #[test]
    fn target_loyalty_holds_initial_player() {
        // Two players equidistant initially. NPC picks one. Then we move the
        // other player closer; loyalty should keep the original target.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        let first = spawn_player(&mut app, 1, TilePosition::ground(7, 5));
        let _second = spawn_player(&mut app, 2, TilePosition::ground(5, 7));
        let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

        app.add_systems(Update, update_roaming_npcs);
        app.update();
        let initial_target = app.world().get::<CombatTarget>(npc).unwrap().entity;
        assert!(initial_target == first || initial_target == _second);

        // Reset timer so we tick again.
        app.world_mut()
            .get_mut::<RoamingStepTimer>(npc)
            .unwrap()
            .remaining_seconds = 0.0;
        // Move the *other* player to be much closer than the original target.
        let other = if initial_target == first {
            _second
        } else {
            first
        };
        let mut other_pos = app.world_mut().get_mut::<TilePosition>(other).unwrap();
        *other_pos = TilePosition::ground(5, 6);
        // Also reset the NPC's timer.
        app.world_mut()
            .get_mut::<RoamingStepTimer>(npc)
            .unwrap()
            .remaining_seconds = 0.0;

        app.update();

        let still = app.world().get::<CombatTarget>(npc).unwrap().entity;
        assert_eq!(
            still, initial_target,
            "NPC should stay locked on the initial target"
        );
    }

    #[test]
    fn astar_routes_around_wall() {
        // Wall plus the player on the other side: greedy would corner, A*
        // must find a path around.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(8, 5));
        let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

        // Vertical wall at x=6, blocking the direct path. Player must be
        // approached via y=3 or y=7.
        for y in 4..=6 {
            app.world_mut().spawn((
                Collider,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(6, y),
            ));
        }

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        // After one tick, NPC should have moved diagonally up or down, not
        // sat at (5,5) and not into the wall at (6,5).
        let npc_position = *app.world().get::<TilePosition>(npc).unwrap();
        assert_ne!(
            npc_position,
            TilePosition::ground(5, 5),
            "NPC should have moved instead of stalling against wall"
        );
        assert_ne!(npc_position, TilePosition::ground(6, 5));
        assert_ne!(npc_position, TilePosition::ground(6, 4));
        assert_ne!(npc_position, TilePosition::ground(6, 6));
    }

    #[test]
    fn melee_npc_does_not_zigzag_when_player_is_top_left() {
        // Regression: A* used row-major neighbor expansion (SW first), so the
        // priority queue's insertion-order tiebreaker steered the first step
        // south-west when the player sat to the north-west of the goblin. The
        // goblin would visibly take a bottom-left step before swinging back
        // up. Cover both the "mostly-west, slightly north" case (the one that
        // tripped the bug in play) and the directly-west case (a tighter
        // three-way tie among SW, W, NW).
        let start = TilePosition::ground(5, 5);
        for player_position in [
            TilePosition::ground(2, 6),
            TilePosition::ground(2, 5),
            TilePosition::ground(1, 6),
        ] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.init_resource::<crate::world::step_triggers::PendingStepEvents>();
            spawn_player(&mut app, 1, player_position);
            let npc = spawn_melee(&mut app, start);

            app.add_systems(Update, update_roaming_npcs);
            app.update();

            let npc_position = *app.world().get::<TilePosition>(npc).unwrap();
            assert!(
                npc_position.y >= start.y,
                "goblin must not step south when player at {player_position:?} is to the \
                 north-west of {start:?}; ended at {npc_position:?}",
            );
            let before = chebyshev_distance(start, player_position);
            let after = chebyshev_distance(npc_position, player_position);
            assert!(
                after < before,
                "goblin must close one step toward {player_position:?} (was {before}, now \
                 {after}); ended at {npc_position:?}",
            );
        }
    }

    #[test]
    fn melee_npc_walks_straight_when_player_is_cardinal() {
        // Directly-north / directly-east players should produce a cardinal
        // first step, not a diagonal. The old row-major expansion would push
        // the goblin onto a diagonal in this case too — visually fine, but
        // covered here so the alignment-aware tiebreaker doesn't regress.
        for (player_position, expected) in [
            (TilePosition::ground(5, 8), TilePosition::ground(5, 6)),
            (TilePosition::ground(8, 5), TilePosition::ground(6, 5)),
            (TilePosition::ground(5, 2), TilePosition::ground(5, 4)),
            (TilePosition::ground(2, 5), TilePosition::ground(4, 5)),
        ] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.init_resource::<crate::world::step_triggers::PendingStepEvents>();
            spawn_player(&mut app, 1, player_position);
            let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

            app.add_systems(Update, update_roaming_npcs);
            app.update();

            let npc_position = *app.world().get::<TilePosition>(npc).unwrap();
            assert_eq!(
                npc_position, expected,
                "goblin chasing cardinal-direction player at {player_position:?} should \
                 take the straight cardinal step",
            );
        }
    }

    #[test]
    fn alert_walks_to_last_seen_then_returns_to_wander() {
        // Place NPC in Alert state directly and verify it walks toward
        // last_seen, then drops back to Wander on expiry.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        // No player nearby — only the alert memory.
        let npc = app
            .world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                TilePosition::ground(5, 5),
                default_roaming(
                    RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 20,
                        max_y: 20,
                    },
                    0.1,
                ),
                default_hostile(20, 20),
                AttackProfile::melee(),
                RoamingStepTimer {
                    remaining_seconds: 0.0,
                },
                RoamingRandomState { seed: 1 },
                AiState::Alert {
                    last_seen: TilePosition::ground(8, 5),
                    expires_at_seconds: 1.0, // small future window
                },
                AiMemory::default(),
            ))
            .id();

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        let npc_position = *app.world().get::<TilePosition>(npc).unwrap();
        assert_eq!(
            chebyshev_distance(npc_position, TilePosition::ground(8, 5)),
            2,
            "NPC should have walked one tile toward last_seen; ended at {npc_position:?}"
        );

        // Force the alert to expire by setting expires_at into the past.
        *app.world_mut().get_mut::<AiState>(npc).unwrap() = AiState::Alert {
            last_seen: TilePosition::ground(8, 5),
            expires_at_seconds: 0.0,
        };
        app.world_mut()
            .get_mut::<RoamingStepTimer>(npc)
            .unwrap()
            .remaining_seconds = 0.0;
        app.update();

        assert!(
            matches!(*app.world().get::<AiState>(npc).unwrap(), AiState::Wander),
            "NPC should return to Wander after alert expires"
        );
    }
}
