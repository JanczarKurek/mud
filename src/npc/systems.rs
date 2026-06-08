use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::prelude::*;

use crate::combat::components::{AttackKind, AttackProfile, CombatTarget};
use crate::combat::systems::is_target_in_range;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents, SpeechBubbleStyle};
use crate::game::shop::Shopkeeper;
use crate::magic::effects::MagicEffects;
use crate::npc::components::{
    AiMemory, AiState, Barks, HostileBehavior, LastDamagedAt, Npc, RoamingBehavior,
    RoamingRandomState, RoamingStepTimer,
};
use crate::player::classes::ability_mod;
use crate::player::components::{DerivedStats, Player};
use crate::world::components::OverworldObject;
use crate::world::components::{
    floor_index, tile_distance_3d, Collider, Facing, SpaceId, SpaceResident, TilePosition,
};
use crate::world::direction::Direction;
use crate::world::spatial::{self, has_line_of_sight, BlockerIndex};

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

/// How recently the NPC must have taken damage from its current target for the
/// "can't reach + hurt" flee trigger to fire. Keeps NPCs from fleeing every
/// time a player merely stands somewhere they can't path to.
const FLEE_RECENT_DAMAGE_WINDOW_SECS: f32 = 4.0;

/// Total time the Flee stance lasts before reverting to Wander. Refreshes
/// every tick the NPC re-spots the attacker.
const FLEE_DURATION_SECS: f32 = 6.0;

/// Grace window an NPC keeps pursuing a target that just changed floors: it
/// heads to where it last saw them (via Alert) and climbs after them if the
/// stairs are close enough to reach in time, otherwise the Alert decays to
/// Wander. Longer than the default alert so a nearby staircase is actually
/// climbable before giving up. The window + walk speed is what makes
/// "follow only if the stairs are near" emergent. Tune by playtest.
const CROSS_FLOOR_FOLLOW_SECS: f32 = 8.0;

/// Grace window an Engage/Pursue keeps its CombatTarget after a *soft* contact
/// loss (line of sight breaks, or the target brushes just past the leash
/// radius). Refreshed every healthy tick, so it measures time since the last
/// solid contact, not since aggro. While inside it the NPC holds the target and
/// keeps pressing toward its live position via A*, only dropping to Alert once
/// the window lapses with contact still broken.
///
/// Several seconds by design: a hostile monster should *commit* to a chase and
/// keep coming for a few seconds after losing sight (around a pillar, through a
/// doorway, behind a brief occlusion) rather than instantly forgetting you.
/// This must also comfortably exceed a single AI step interval (~1s for most
/// NPCs) — otherwise the window expires before the NPC's next tick and the
/// hysteresis never actually fires. Tune by playtest.
const CONTACT_GRACE_SECS: f32 = 3.0;

/// Spatial index of static blocker tiles, rebuilt at the top of
/// `update_roaming_npcs`. Replaces a per-NPC × per-candidate-tile linear scan
/// of every collider in the world (~thousands), which produced a 20+ ms spike
/// every step interval when all NPCs synchronized on the same frame. The
/// type is defined in `crate::world::spatial` and shared with combat.
type NpcTileIndex = HashMap<(SpaceId, TilePosition), Entity>;
type PlayerTileSet = HashSet<(SpaceId, TilePosition)>;

pub fn update_roaming_npcs(
    time: Res<Time>,
    blocker_query: Query<
        (&SpaceResident, &TilePosition, Option<&OverworldObject>),
        (With<Collider>, Without<Npc>),
    >,
    definitions: Option<Res<crate::world::object_definitions::OverworldObjectDefinitions>>,
    floor_maps: Option<Res<crate::world::floor_map::FloorMaps>>,
    floor_defs: Option<Res<crate::world::floor_definitions::FloorTilesetDefinitions>>,
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
    last_damaged_query: Query<&LastDamagedAt, With<Npc>>,
    derived_stats_query: Query<&DerivedStats, With<Npc>>,
    mut pending_steps: ResMut<crate::world::step_triggers::PendingStepEvents>,
    mut ui_events: Option<ResMut<PendingGameUiEvents>>,
    mut commands: Commands,
) {
    let _t = crate::diagnostics::SystemTimer::new("npc:update_roaming_npcs", 1.0);
    let elapsed = time.elapsed_secs();

    let players: Vec<(Entity, SpaceId, TilePosition)> = player_query
        .iter()
        .map(|(entity, resident, tile_position)| (entity, resident.space_id, *tile_position))
        .collect();

    // Movement vs. line-of-sight indices — see `crate::world::spatial` for
    // the semantics. Movement is what we pass to `resolve_npc_step` so cascade
    // descent finds an upper-floor surface to land on; LoS is what we pass to
    // `has_line_of_sight` so vision rays stop at painted ceilings.
    let (blockers, los_blockers) = {
        let _t = crate::diagnostics::SystemTimer::new("npc:build_indices", 1.0);
        spatial::build_indices(
            blocker_query.iter(),
            definitions.as_deref(),
            floor_maps.as_deref(),
            floor_defs.as_deref(),
        )
    };

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
        let last_damaged_at = last_damaged_query.get(entity).ok().map(|t| t.0);
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

        let mut outcome = step_ai(StepAiInput {
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
            los_blockers: &los_blockers,
            npc_tiles: &npc_tiles,
            player_tiles: &player_tiles,
            random_state: &mut random_state,
            elapsed,
            barks,
            last_damaged_at,
        });

        // Athletics gate on any step that climbs more than the free auto-step
        // (dz>1). NPCs without a `DerivedStats` use STR 10 → +0 mod, matching
        // the player default. A failed roll forfeits the step for this tick;
        // the NPC may retry next tick with a different d20 roll.
        if let Some(landed) = outcome.move_to {
            let dz = landed.z - tile_position.z;
            if dz > crate::game::traversal::CLIMB_FREE_DZ {
                let strength = derived_stats_query
                    .get(entity)
                    .map(|d| d.attributes.strength)
                    .unwrap_or(10);
                let dc = crate::game::traversal::climb_dc(dz);
                let roll = (next_random_index(&mut random_state, 20) as i32) + 1;
                if roll + ability_mod(strength) < dc {
                    outcome.move_to = None;
                }
            }
        }

        // Per-tick AI trace. Run with `RUST_LOG=mud2::npc::systems=debug`
        // (or `RUST_LOG=debug` in the embedded client) to see the per-NPC
        // state-machine decisions: previous → next state, target change,
        // resolved move tile, and the NPC's identity. One line per NPC per
        // step interval — verbose but invaluable when an enemy gets stuck.
        let prev_state = *ai_state;
        let npc_label = overworld_object
            .map(|object| format!("{}#{}", object.definition_id, object.object_id))
            .unwrap_or_else(|| format!("npc{:?}", entity));
        debug!(
            target: "npc_ai",
            "{npc_label}@{x},{y},{z}: {prev:?} → {next:?} target={target:?} move={move_to:?}",
            x = tile_position.x,
            y = tile_position.y,
            z = tile_position.z,
            prev = prev_state,
            next = outcome.next_state,
            target = outcome.target,
            move_to = outcome.move_to,
        );

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
    los_blockers: &'a BlockerIndex,
    npc_tiles: &'a NpcTileIndex,
    player_tiles: &'a PlayerTileSet,
    random_state: &'a mut RoamingRandomState,
    elapsed: f32,
    barks: Option<&'a Barks>,
    /// Elapsed-seconds timestamp of the last damage we took; `None` if we've
    /// never been hit. Used to gate the Flee transition so an NPC only flees
    /// from an unreachable target that has recently hurt them.
    last_damaged_at: Option<f32>,
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

#[derive(Debug)]
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
        AiState::Flee {
            from,
            expires_at_seconds,
        } => tick_flee(&mut input, from, expires_at_seconds),
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
            input.los_blockers,
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
    //
    // Use the LoS index (not the movement index): visibility here must agree
    // with `tick_wander`'s acquisition (`:nearest_visible_player` with
    // `los_blockers`) and the `lost_los` gate in `tick_pursue_or_engage`.
    // Detecting with the movement index while the pursue gate checks the LoS
    // index lets an NPC "see" a target it then immediately declares out of
    // contact, freezing it in a detect→abort loop.
    if let Some(hostile) = input.hostile_behavior {
        if let Some((target_entity, _)) = nearest_visible_player(
            input.tile_position,
            input.space_id,
            hostile,
            input.players,
            input.los_blockers,
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
    // Single source of truth for "can I hit them": the exact predicate combat
    // uses to resolve an attack. Deriving engagement from anything else risks
    // the AI abandoning a target it is physically able to strike — which is how
    // the z=1↔z=2 stair oscillation arose (a `floor_index` cross-floor check
    // fired on a target one half-block away that was squarely in melee reach).
    let now_engaged = is_target_in_range(
        attack_kind_of(input.attack_profile),
        &input.tile_position,
        &target_pos,
    );
    let next_target = if engaged != now_engaged {
        TargetChange::Set(target) // Re-affirm; cheap, keeps CombatTarget present.
    } else {
        TargetChange::Keep
    };

    // In attack range → hold (melee) or kite (ranged) and let combat resolve the
    // hit. Decided *before* the cross-floor / leash / LoS drops below: if we can
    // strike them this tick we never let those gates yank the CombatTarget out
    // from under an active engagement. Refreshes the contact-grace window.
    if now_engaged {
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

        return AiOutcome {
            next_state: AiState::Engage { target },
            next_memory: AiMemory {
                last_step: None,
                contact_grace_until: input.elapsed + CONTACT_GRACE_SECS,
                ..input.memory
            },
            target: next_target,
            move_to,
            idle_pause: false,
            bark: None,
        };
    }

    // Not in range. A target more than one auto-climb step up/down is on a real
    // floor we can't reach without stairs: lose direct contact (drop the
    // CombatTarget → red dot off) but fall into Alert aimed at their last tile so
    // we head for the stairwell and climb after them. `tick_alert` re-detects
    // once we reach their floor and decays to Wander if the stairs are too far. A
    // single half-block (z=1↔z=2) is climbable, so it is *not* cross-floor — fall
    // through and pursue normally; `resolve_npc_step` auto-climbs it.
    let dz = (input.tile_position.z - target_pos.z).abs();
    if dz > crate::game::traversal::CLIMB_FREE_DZ {
        return AiOutcome {
            next_state: AiState::Alert {
                last_seen: target_pos,
                expires_at_seconds: input.elapsed + CROSS_FLOOR_FOLLOW_SECS,
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

    // Soft contact loss (leash exceeded or line of sight broken). A grace window
    // — refreshed on every healthy tick — lets a one-tick LoS flicker or a brush
    // past the leash ride through without strobing the CombatTarget: while inside
    // it we hold the target and keep closing on their last position, and only
    // once it lapses do we drop to Alert.
    let lost_leash = distance > hostile.disengage_distance_tiles;
    let lost_los = hostile.requires_line_of_sight
        && !has_line_of_sight(
            input.tile_position,
            target_pos,
            input.space_id,
            input.los_blockers,
        );
    if lost_leash || lost_los {
        if input.elapsed >= input.memory.contact_grace_until {
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
        // Within grace: keep the target, keep pressing toward their last tile.
        // Don't refresh the window — let it lapse if contact stays broken.
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
            target: TargetChange::Keep,
            move_to,
            idle_pause: false,
            bark: None,
        };
    }

    // Healthy pursuit: A* toward target, target tile treated as walkable for the
    // pathfinder so it doesn't dead-end against the player's own tile.
    let astar = astar_next_step(
        input.entity,
        input.space_id,
        input.tile_position,
        target_pos,
        input.blockers,
        input.npc_tiles,
        input.player_tiles,
        Some(target_pos),
    );

    // Flee trigger: A* couldn't find a path *and* the target has hurt us
    // recently. Catches "player camped on a ledge we can't climb while
    // shooting down" — without this, the NPC would stand still eating
    // arrows. Greedy seek isn't enough to disprove reachability (it might
    // make local progress while still being permanently stuck), so we
    // gate strictly on A*.
    if astar.is_none()
        && input
            .last_damaged_at
            .is_some_and(|t| input.elapsed - t <= FLEE_RECENT_DAMAGE_WINDOW_SECS)
    {
        return AiOutcome {
            next_state: AiState::Flee {
                from: target,
                expires_at_seconds: input.elapsed + FLEE_DURATION_SECS,
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

    let move_to = astar.or_else(|| {
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
    AiOutcome {
        next_state: AiState::Pursue { target },
        next_memory: AiMemory {
            last_step: None,
            contact_grace_until: input.elapsed + CONTACT_GRACE_SECS,
            ..input.memory
        },
        target: next_target,
        move_to,
        idle_pause: false,
        bark: None,
    }
}

/// Run a flee tick: pick the best (no-LoS, distant) neighbor away from
/// `from`. Expires to Wander when `expires_at_seconds` has elapsed or the
/// attacker leaves the space. Refreshes the timer while LoS to `from` is
/// still possible — staying visible means the NPC is still being chased and
/// shouldn't stop fleeing yet.
fn tick_flee(input: &mut StepAiInput<'_>, from: Entity, expires_at_seconds: f32) -> AiOutcome {
    let attacker = input.players.iter().copied().find(|(e, _, _)| *e == from);
    let Some((_, attacker_space, attacker_pos)) = attacker else {
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
    let attacker_lost = attacker_space != input.space_id;
    if attacker_lost || input.elapsed >= expires_at_seconds {
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

    // Refresh the timer while the attacker can still see us — they're
    // still chasing, so the flee shouldn't time out.
    let still_in_sight = has_line_of_sight(
        input.tile_position,
        attacker_pos,
        input.space_id,
        input.los_blockers,
    );
    let next_expires_at = if still_in_sight {
        input.elapsed + FLEE_DURATION_SECS
    } else {
        expires_at_seconds
    };

    // Pick the neighbor that maximizes (no-LoS, distance-from-attacker).
    let mut best: Option<(TilePosition, (i32, i32))> = None;
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let Some(candidate) =
                resolve_npc_step(input.space_id, input.tile_position, dx, dy, input.blockers)
            else {
                continue;
            };
            if is_blocked_position(
                input.entity,
                input.space_id,
                candidate,
                input.blockers,
                input.npc_tiles,
                Some(input.player_tiles),
                None,
            ) {
                continue;
            }
            let los =
                has_line_of_sight(candidate, attacker_pos, input.space_id, input.los_blockers);
            let los_score = if los { 0 } else { 1 };
            let dist_score = chebyshev_distance(candidate, attacker_pos);
            let score = (los_score, dist_score);
            if best.is_none_or(|(_, existing)| score > existing) {
                best = Some((candidate, score));
            }
        }
    }

    AiOutcome {
        next_state: AiState::Flee {
            from,
            expires_at_seconds: next_expires_at,
        },
        next_memory: AiMemory {
            last_step: None,
            ..input.memory
        },
        target: TargetChange::Clear,
        move_to: best.map(|(pos, _)| pos),
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
        // No sensing across a *full* floor by default: an NPC detects players on
        // its own floor, plus anything within one auto-climb half-block (so a
        // player on the stair step right beside it — z=1↔z=2, which straddles a
        // `floor_index` boundary yet is in melee reach — is seen, matching the
        // combat reach rule). The half-block allowance keeps the stairwell
        // line-of-sight leak closed: a player a full floor up (dz≥2) through an
        // open stair hole still fails this gate. A future "sense other floors"
        // skill would widen the band further.
        .filter(|(_, _, position)| {
            floor_index(position.z) == floor_index(tile_position.z)
                || (position.z - tile_position.z).abs() <= crate::game::traversal::CLIMB_FREE_DZ
        })
        .filter(|(_, _, position)| chebyshev_distance(tile_position, *position) <= radius)
        .filter(|(_, _, position)| {
            !hostile.requires_line_of_sight
                || has_line_of_sight(tile_position, *position, space_id, blockers)
        })
        .min_by_key(|(_, _, position)| chebyshev_distance(tile_position, *position))
        .map(|(entity, _, position)| (entity, position))
}

/// The NPC's attack kind, defaulting an absent profile to `Melee`. Feeds the
/// shared `is_target_in_range` reach test so an unarmed NPC engages at melee
/// reach rather than not at all.
fn attack_kind_of(profile: Option<&AttackProfile>) -> AttackKind {
    profile.map(|p| p.kind).unwrap_or(AttackKind::Melee)
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
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let Some(candidate) = resolve_npc_step(space_id, tile_position, dx, dy, blockers)
            else {
                continue;
            };
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
/// Uses the same Z-aware step resolver as A*, so cross-floor seeks behave
/// consistently between the two paths.
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
    let mut candidates: Vec<TilePosition> = Vec::with_capacity(8);
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if let Some(landed) = resolve_npc_step(space_id, tile_position, dx, dy, blockers) {
                candidates.push(landed);
            }
        }
    }

    candidates.sort_by_key(|candidate| {
        (
            chebyshev_distance(*candidate, seek_target),
            // Slightly penalize diagonal-XY steps when distances tie, same
            // tiebreak as the previous IVec2-based version.
            i32::from(candidate.x - tile_position.x != 0 && candidate.y - tile_position.y != 0),
        )
    });

    for target_position in candidates {
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

/// Resolve a single NPC step in direction `(dx, dy)` from `from`, accounting
/// for Z-traversal. Returns the destination tile or `None` when no valid
/// landing exists.
///
/// Uses the BlockerIndex-only world model (Colliders are both walls and
/// standable surfaces — a wall's top is its highest Collider z plus one).
/// `update_roaming_npcs` inflates each Collider over its definition's full
/// `block_size`, so a 2-half-block wall blocks both z and z+1 and the NPC
/// can't laterally clip through the upper half. No floor-map or walkable-
/// decal awareness; an NPC walking sideways on an upper-floor surface relies
/// on the absence of a Collider, matching the pre-Z-aware behavior.
///
/// Resolution order:
/// 1. **Flat** at `from.z` — if the lateral tile is not blocked, step
///    laterally. Identical to the pre-Z-aware behavior.
/// 2. **Climb up** by `dz ∈ [1, CLIMB_MAX_DZ]` — when flat is blocked, scan
///    upward for the first unblocked tile that has a supporting Collider
///    just below it (chest top, wall top, etc.). Includes the dz=1 free
///    auto-step.
/// 3. **Cascade descent** — when the lateral tile is unblocked at `from.z`,
///    walk straight down to the highest support strictly below `cz` (or the
///    ground at z=0). Mirrors the player's `resolve_step_with_climb` descent
///    so an NPC that steps off a wall top into empty air falls to the
///    floor instead of hovering.
fn resolve_npc_step(
    space_id: SpaceId,
    from: TilePosition,
    dx: i32,
    dy: i32,
    blockers: &BlockerIndex,
) -> Option<TilePosition> {
    let x = from.x + dx;
    let y = from.y + dy;
    let cz = from.z;

    let flat = TilePosition::new(x, y, cz);
    let flat_blocked = blockers.contains(&(space_id, flat));

    if !flat_blocked {
        // Cascade down to the highest support strictly below `cz` (or ground
        // at z=0). This mirrors the player's `resolve_step_with_climb`
        // descent branch: an NPC that walks off a wall top into empty air
        // falls all the way to the floor instead of hovering at z=cz.
        if cz > 0 {
            let mut landing_z = 0;
            for z in (0..cz).rev() {
                if blockers.contains(&(space_id, TilePosition::new(x, y, z))) {
                    landing_z = z + 1;
                    break;
                }
            }
            if landing_z < cz {
                return Some(TilePosition::new(x, y, landing_z));
            }
        }
        return Some(flat);
    }

    // Flat blocked. Auto-climb caps at CLIMB_FREE_DZ to match the player's
    // no-SHIFT behavior: half-block steps (chests, stair_n_low, stone_step)
    // resolve, full-block walls don't. Without this cap an NPC would freely
    // scale a 2-half-block wall — visually wrong, and the destination on
    // top of the wall has no real support so the NPC ends up hovering.
    for nz in (cz + 1)..=(cz + crate::game::traversal::CLIMB_FREE_DZ) {
        let up = TilePosition::new(x, y, nz);
        if blockers.contains(&(space_id, up)) {
            continue;
        }
        if blockers.contains(&(space_id, TilePosition::new(x, y, nz - 1))) {
            return Some(up);
        }
    }

    None
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
    let _t = crate::diagnostics::SystemTimer::new("npc:astar", 1.0);
    if start == goal {
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
            let Some(neighbor) = resolve_npc_step(space_id, current, dx, dy, blockers) else {
                continue;
            };
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
            // Step cost: 1 + dz_up so A* prefers flat routes when both
            // exist. Descents (dz<0) are free — gravity does the work.
            let dz_up = (neighbor.z - current.z).max(0);
            let tentative_g = current_g + 1 + dz_up;
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

// Line-of-sight lives in `crate::world::spatial::has_line_of_sight`; we
// re-export it at the top of the file so call sites can stay terse.

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
    tile_distance_3d(a, b)
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

    /// Melee NPC that requires line of sight — for the contact-grace tests.
    fn spawn_melee_los(app: &mut App, position: TilePosition) -> Entity {
        let mut hostile = default_hostile(20, 20);
        hostile.requires_line_of_sight = true;
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
                hostile,
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

    /// Regression: an NPC and player standing on the same occluding upper floor
    /// (both at z=2) must still see each other beyond melee range. The floor's
    /// line-of-sight slab used to sit on the walking surface (z=2), blocking
    /// every non-adjacent same-floor ray, so an LoS-gated NPC froze — attacking
    /// only when the player was directly adjacent. With the slab lowered to the
    /// between-floor half-block (z=1), the NPC acquires and pursues.
    #[test]
    fn los_npc_pursues_across_occluding_upper_floor() {
        use crate::world::floor_definitions::{FloorTilesetDefinition, FloorTilesetDefinitions};
        use crate::world::floor_map::{FloorMap, FloorMaps};
        use std::collections::HashMap;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        // Paint floor index 1 with a walkable, occluding floor across the area.
        let mut maps = FloorMaps::default();
        maps.insert(
            TEST_SPACE,
            1,
            FloorMap::new_filled(20, 20, Some("wooden_floor".to_string())),
        );
        let mut floor_by_id = HashMap::new();
        floor_by_id.insert(
            "wooden_floor".to_string(),
            FloorTilesetDefinition {
                id: "wooden_floor".to_string(),
                name: "Wooden Floor".to_string(),
                priority: 100,
                tile_size_px: 16,
                atlas_path: None,
                debug_color: [0, 0, 0],
                occludes_floor_above: true,
                walkable_surface: true,
                variants: HashMap::new(),
                ripple: None,
            },
        );
        app.insert_resource(maps);
        app.insert_resource(FloorTilesetDefinitions::for_test(
            floor_by_id,
            HashMap::new(),
        ));

        // Player and NPC on the second floor (z=2), four tiles apart along x.
        spawn_player(&mut app, 1, TilePosition::new(9, 5, 2));
        let npc = spawn_melee_los(&mut app, TilePosition::new(5, 5, 2));

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        // The NPC must have acquired the player it can see across the floor and
        // stepped toward them (east, staying on the upper floor) — not frozen.
        assert!(
            app.world().get::<CombatTarget>(npc).is_some(),
            "LoS NPC should acquire a player it can see across the same floor"
        );
        let pos = *app.world().get::<TilePosition>(npc).unwrap();
        assert_eq!(pos.z, 2, "NPC should stay on the upper floor (z=2)");
        assert!(
            pos.x > 5,
            "NPC should step toward the player (east), got {pos:?}"
        );
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
        // Player adjacent, all retreat tiles blocked. With Z-aware
        // pathfinding the archer can climb onto a 1-block obstacle, so we
        // stack the cornering colliders 5 half-blocks tall — beyond the
        // CLIMB_MAX_DZ ceiling — to actually trap it. The intent is unchanged:
        // an NPC with no escape route stands still.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 6));
        let archer = spawn_archer(&mut app, TilePosition::ground(5, 5), 6);

        for (x, y) in [(4, 4), (5, 4), (6, 4), (4, 5), (6, 5), (4, 6), (6, 6)] {
            for z in 0..=crate::game::traversal::CLIMB_MAX_DZ {
                app.world_mut().spawn((
                    Collider,
                    SpaceResident {
                        space_id: TEST_SPACE,
                    },
                    TilePosition::new(x, y, z),
                ));
            }
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
    fn npc_does_not_target_player_a_floor_up() {
        // No cross-floor sensing by default: a player a full floor above (z=2,
        // floor 1) with no stairs is out of reach and should not be detected —
        // even with a clear vertical line. (This test used to assert the
        // opposite; that "targets you through the floor" behavior is the bug
        // we're removing. Half-block steps within a floor still target — see
        // `npc_targets_player_on_a_half_block_step`.)
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::new(5, 6, 2));
        let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "NPC should not target a player a full floor above it"
        );
    }

    #[test]
    fn npc_targets_player_on_a_half_block_step() {
        // Regression guard for the floor gate: a player perched on a half-block
        // step / chest (z=1) is still on floor 0 (`floor_index(1) == 0`), so the
        // NPC at z=0 detects and targets them. The gate keys on `floor_index`,
        // not raw z, precisely so Tibia-style auto-step combat keeps working.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        let player = spawn_player(&mut app, 1, TilePosition::new(5, 6, 1));
        let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert_eq!(
            app.world().get::<CombatTarget>(npc).map(|t| t.entity),
            Some(player),
            "NPC should target a player on a half-block step (same floor)"
        );
    }

    #[test]
    fn npc_drops_target_to_alert_when_player_changes_floor() {
        // A chased player who escapes up a floor isn't dropped instantly: the
        // NPC loses direct contact (CombatTarget cleared → red dot off) but
        // falls into Alert aimed at their last tile, so it heads for the stairs
        // and follows if they're near.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        let player = spawn_player(&mut app, 1, TilePosition::ground(5, 6));
        let npc = spawn_melee(&mut app, TilePosition::ground(5, 5));

        app.add_systems(Update, update_roaming_npcs);
        app.update();
        assert_eq!(
            app.world().get::<CombatTarget>(npc).map(|t| t.entity),
            Some(player),
            "NPC should engage the adjacent same-floor player first"
        );

        // Player escapes one floor up; force another AI tick.
        *app.world_mut().get_mut::<TilePosition>(player).unwrap() = TilePosition::new(5, 6, 2);
        app.world_mut()
            .get_mut::<RoamingStepTimer>(npc)
            .unwrap()
            .remaining_seconds = 0.0;
        app.update();

        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "crossing a floor should clear the CombatTarget (red dot off)"
        );
        assert!(
            matches!(app.world().get::<AiState>(npc), Some(AiState::Alert { .. })),
            "crossing a floor should drop to Alert (follow), not instantly to Wander"
        );
    }

    #[test]
    fn nearest_visible_player_ignores_other_floors() {
        // Unit-level proof of the detection gate: same XY, only z differs.
        let hostile = default_hostile(20, 20);
        let blockers: BlockerIndex = BlockerIndex::default();
        let npc = TilePosition::ground(5, 5);

        let upstairs = vec![(Entity::PLACEHOLDER, TEST_SPACE, TilePosition::new(5, 6, 2))];
        assert!(
            nearest_visible_player(npc, TEST_SPACE, &hostile, &upstairs, &blockers, 20).is_none(),
            "a player a floor up must be invisible to detection"
        );

        let same_floor = vec![(Entity::PLACEHOLDER, TEST_SPACE, TilePosition::new(5, 6, 1))];
        assert!(
            nearest_visible_player(npc, TEST_SPACE, &hostile, &same_floor, &blockers, 20).is_some(),
            "a player on a half-block step (same floor) stays visible"
        );
    }

    #[test]
    fn nearest_visible_player_sees_one_half_block_across_floor_boundary() {
        // The widened detection band: an NPC on the lower step (z=1, floor 0)
        // now senses a player on the upper step (z=2, floor 1) right beside it —
        // one auto-climb half-block away, matching melee reach. A genuine full
        // floor up (dz=2) still fails the gate, so the stairwell LoS leak stays
        // closed.
        let hostile = default_hostile(20, 20);
        let blockers: BlockerIndex = BlockerIndex::default();
        let npc = TilePosition::new(5, 5, 1);

        let half_block_up = vec![(Entity::PLACEHOLDER, TEST_SPACE, TilePosition::new(5, 6, 2))];
        assert!(
            nearest_visible_player(npc, TEST_SPACE, &hostile, &half_block_up, &blockers, 20)
                .is_some(),
            "a player one half-block up (z=1→z=2) must be visible (melee reach)"
        );

        let full_floor_up = vec![(Entity::PLACEHOLDER, TEST_SPACE, TilePosition::new(5, 6, 3))];
        assert!(
            nearest_visible_player(npc, TEST_SPACE, &hostile, &full_floor_up, &blockers, 20)
                .is_none(),
            "a player a full floor up (z=1→z=3, dz=2) stays invisible"
        );
    }

    #[test]
    fn npc_holds_engagement_on_stair_step_no_oscillation() {
        // The reported bug: the player camps the upper stair step (z=2, floor 1)
        // and the NPC settles on the step below (z=1, floor 0). The pair is one
        // half-block apart — squarely in melee reach (dz=1) yet straddling the
        // `floor_index` boundary. The NPC must hold a stable Engage and never
        // flap to Alert / drop the CombatTarget, tick after tick.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        let player = spawn_player(&mut app, 1, TilePosition::new(5, 6, 2));
        let npc = spawn_melee(&mut app, TilePosition::new(5, 5, 1));

        app.add_systems(Update, update_roaming_npcs);

        for tick in 0..6 {
            app.world_mut()
                .get_mut::<RoamingStepTimer>(npc)
                .unwrap()
                .remaining_seconds = 0.0;
            app.update();

            assert!(
                matches!(
                    app.world().get::<AiState>(npc),
                    Some(AiState::Engage { .. })
                ),
                "tick {tick}: NPC should stay Engaged with the player one step \
                 above, not flap to {:?}",
                app.world().get::<AiState>(npc),
            );
            assert_eq!(
                app.world().get::<CombatTarget>(npc).map(|t| t.entity),
                Some(player),
                "tick {tick}: CombatTarget must stay locked (no red-dot strobe)"
            );
            assert_eq!(
                *app.world().get::<TilePosition>(npc).unwrap(),
                TilePosition::new(5, 5, 1),
                "tick {tick}: melee NPC holds its step, doesn't climb onto the player"
            );
        }
    }

    #[test]
    fn half_block_up_target_is_pursued_not_dropped() {
        // A target one half-block up but out of melee reach (xy=2, dz=1) is NOT
        // "cross-floor": a single auto-climb step closes it. The NPC stays in
        // Pursue with the target locked, rather than dropping to the cross-floor
        // Alert that the old `floor_index` gate triggered here.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        let player = spawn_player(&mut app, 1, TilePosition::new(5, 7, 2));
        let npc = spawn_melee(&mut app, TilePosition::new(5, 5, 1));

        app.add_systems(Update, update_roaming_npcs);
        app.update();

        assert!(
            matches!(
                app.world().get::<AiState>(npc),
                Some(AiState::Pursue { .. })
            ),
            "a half-block-up target should be pursued, got {:?}",
            app.world().get::<AiState>(npc),
        );
        assert_eq!(
            app.world().get::<CombatTarget>(npc).map(|t| t.entity),
            Some(player),
            "pursuit should keep the CombatTarget on a half-block-up target"
        );
    }

    #[test]
    fn los_flicker_within_grace_keeps_target() {
        // With LoS required, a single tick of broken sight must NOT immediately
        // drop the target: the contact-grace window holds the CombatTarget and
        // keeps the NPC in Pursue. (Pre-hysteresis this strobed straight to
        // Alert the instant the ray was occluded.)
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        let player = spawn_player(&mut app, 1, TilePosition::ground(5, 9));
        let npc = spawn_melee_los(&mut app, TilePosition::ground(5, 5));

        app.add_systems(Update, update_roaming_npcs);
        app.update(); // clear LoS → acquire + Pursue, grace armed.
        assert_eq!(
            app.world().get::<CombatTarget>(npc).map(|t| t.entity),
            Some(player),
            "NPC should acquire the visible player on the first tick"
        );

        // Hold the grace window wide open, break LoS with a wall, tick again.
        app.world_mut()
            .get_mut::<AiMemory>(npc)
            .unwrap()
            .contact_grace_until = 1.0e9;
        app.world_mut().spawn((
            Collider,
            SpaceResident {
                space_id: TEST_SPACE,
            },
            TilePosition::ground(5, 7),
        ));
        app.world_mut()
            .get_mut::<RoamingStepTimer>(npc)
            .unwrap()
            .remaining_seconds = 0.0;
        app.update();

        assert!(
            matches!(
                app.world().get::<AiState>(npc),
                Some(AiState::Pursue { .. })
            ),
            "within grace, a broken LoS should keep pursuing, got {:?}",
            app.world().get::<AiState>(npc),
        );
        assert_eq!(
            app.world().get::<CombatTarget>(npc).map(|t| t.entity),
            Some(player),
            "within grace, the CombatTarget must persist through a LoS flicker"
        );
    }

    #[test]
    fn los_loss_past_grace_drops_to_alert() {
        // Once the grace window lapses with sight still broken, the NPC concedes
        // contact: CombatTarget cleared and state drops to Alert (it heads for
        // the last-seen tile rather than chasing blind).
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<crate::world::step_triggers::PendingStepEvents>();

        spawn_player(&mut app, 1, TilePosition::ground(5, 9));
        let npc = spawn_melee_los(&mut app, TilePosition::ground(5, 5));

        app.add_systems(Update, update_roaming_npcs);
        app.update(); // acquire + Pursue.

        // Expire the grace window (deadline in the past) and break LoS.
        app.world_mut()
            .get_mut::<AiMemory>(npc)
            .unwrap()
            .contact_grace_until = 0.0;
        app.world_mut().spawn((
            Collider,
            SpaceResident {
                space_id: TEST_SPACE,
            },
            TilePosition::ground(5, 7),
        ));
        app.world_mut()
            .get_mut::<RoamingStepTimer>(npc)
            .unwrap()
            .remaining_seconds = 0.0;
        app.update();

        assert!(
            matches!(app.world().get::<AiState>(npc), Some(AiState::Alert { .. })),
            "past grace, broken LoS should drop to Alert, got {:?}",
            app.world().get::<AiState>(npc),
        );
        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "past grace, the CombatTarget should be cleared"
        );
    }

    #[test]
    fn astar_climbs_stairs_to_reach_player_on_upper_floor() {
        // A staircase east of the NPC leads to a player on floor 1. Modeled in
        // the blocker grid the way `update_roaming_npcs` inflates real objects:
        //   (6,5,0)         stair_*_low  (block_size 1 → blocks z=0)
        //   (7,5,0),(7,5,1) stair_*_high (block_size 2 → blocks z=0..1)
        //   (8,5,1)         upper-floor support (player stands on it at z=2)
        // A* should take the first climbing step onto the low stair rather than
        // dead-ending under the player — this is what lets the cross-floor Alert
        // follow a target up the stairs.
        let mut blockers: BlockerIndex = BlockerIndex::default();
        blockers.insert((TEST_SPACE, TilePosition::new(6, 5, 0)));
        blockers.insert((TEST_SPACE, TilePosition::new(7, 5, 0)));
        blockers.insert((TEST_SPACE, TilePosition::new(7, 5, 1)));
        blockers.insert((TEST_SPACE, TilePosition::new(8, 5, 1)));
        let npc_tiles: NpcTileIndex = HashMap::new();
        let player_tiles: PlayerTileSet = HashSet::new();

        let goal = TilePosition::new(8, 5, 2);
        let next = astar_next_step(
            Entity::PLACEHOLDER,
            TEST_SPACE,
            TilePosition::ground(5, 5),
            goal,
            &blockers,
            &npc_tiles,
            &player_tiles,
            Some(goal),
        )
        .expect("A* should find a stair route to the upper floor");
        assert_eq!(
            next,
            TilePosition::new(6, 5, 1),
            "first step should climb onto the low stair"
        );
    }

    #[test]
    fn resolve_npc_step_climbs_onto_a_chest() {
        // A single-tile chest (Collider at z=0) stands east of the NPC.
        // resolve_npc_step should treat the chest top (z=1) as a valid
        // landing — that's the dz=1 auto-step the player gets for free.
        let mut blockers: BlockerIndex = BlockerIndex::default();
        blockers.insert((TEST_SPACE, TilePosition::ground(6, 5)));

        let landed =
            resolve_npc_step(TEST_SPACE, TilePosition::ground(5, 5), 1, 0, &blockers).unwrap();
        assert_eq!(landed, TilePosition::new(6, 5, 1));
    }

    #[test]
    fn resolve_npc_step_refuses_unsupported_climb() {
        // (6, 5, 1) is unblocked but has no supporting collider at z=0 —
        // there's nothing to stand on. Flat (6, 5, 0) is what we land on.
        let mut blockers: BlockerIndex = BlockerIndex::default();
        // No colliders at all → flat z=0 is the answer.
        let landed =
            resolve_npc_step(TEST_SPACE, TilePosition::ground(5, 5), 1, 0, &blockers).unwrap();
        assert_eq!(landed, TilePosition::ground(6, 5));

        // Now add a Collider only at z=1 (floating wall fragment) but
        // leave z=0 open. NPC steps onto flat z=0 as before — the floating
        // collider doesn't change the landing.
        blockers.insert((TEST_SPACE, TilePosition::new(6, 5, 1)));
        let landed =
            resolve_npc_step(TEST_SPACE, TilePosition::ground(5, 5), 1, 0, &blockers).unwrap();
        assert_eq!(landed, TilePosition::ground(6, 5));
    }

    #[test]
    fn cross_floor_line_of_sight_traces_through_voxels() {
        // Source (0,0,0) → target (3,0,2). With no blockers, LoS holds.
        let blockers: BlockerIndex = BlockerIndex::default();
        assert!(has_line_of_sight(
            TilePosition::ground(0, 0),
            TilePosition::new(3, 0, 2),
            TEST_SPACE,
            &blockers,
        ));

        // A wall slab at the interpolated midpoint blocks the line. With
        // dx=3, dz=2 the steps land at (1,0,1), (2,0,1), so a blocker at
        // (2,0,1) sits squarely on the arc.
        let mut wall: BlockerIndex = BlockerIndex::default();
        wall.insert((TEST_SPACE, TilePosition::new(2, 0, 1)));
        assert!(!has_line_of_sight(
            TilePosition::ground(0, 0),
            TilePosition::new(3, 0, 2),
            TEST_SPACE,
            &wall,
        ));
    }

    #[test]
    fn astar_climbs_a_chest_to_reach_player() {
        // Player on top of a chest 1 tile east; NPC starts west of the chest.
        // The chest is a Collider at (6, 5, 0); the player perches at (6, 5, 1).
        // A* should choose to climb the chest (dz=1, free auto-step) rather
        // than rejecting the path because it crosses Z.
        let mut blockers: BlockerIndex = BlockerIndex::default();
        blockers.insert((TEST_SPACE, TilePosition::ground(6, 5)));
        let npc_tiles: NpcTileIndex = HashMap::new();
        let player_tiles: PlayerTileSet = HashSet::new();

        let next = astar_next_step(
            Entity::PLACEHOLDER,
            TEST_SPACE,
            TilePosition::ground(5, 5),
            TilePosition::new(6, 5, 1),
            &blockers,
            &npc_tiles,
            &player_tiles,
            Some(TilePosition::new(6, 5, 1)),
        )
        .expect("A* should find a path that climbs onto the chest");
        // First (and only) step lands on the chest top — adjacency-1.
        assert_eq!(next, TilePosition::new(6, 5, 1));
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
