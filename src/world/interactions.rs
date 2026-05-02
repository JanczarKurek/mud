//! Player-driven state transitions on stateful world objects (doors, levers,
//! torches, …). Drained from `PendingGameCommands` in `CommandIntercept`,
//! before `process_game_commands` runs.
//!
//! Transitions mutate the authoritative `ObjectState` component, mirror the
//! new state into `ObjectRegistry::properties[id]["state"]` (so persistence
//! captures it for free), and insert/remove `Collider` markers when the state
//! changes the colliding flag. Side-effects (lever → door) cascade in the
//! same frame, capped by `MAX_CASCADE_DEPTH`.

use bevy::prelude::*;

use crate::game::commands::GameCommand;
use crate::game::helpers::is_near_player;
use crate::game::resources::{
    ContainerViewers, GameUiEvent, PendingGameCommands, PendingGameUiEvents, QueuedGameCommand,
};
use crate::player::components::{Player, PlayerId, PlayerIdentity};
use crate::world::components::{
    Collider, ObjectState, OverworldObject, SpaceResident, TilePosition,
};
use crate::world::object_definitions::{InteractionSideEffect, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;

/// Bound on cascade depth — long enough to cover plausible lever-chains, low
/// enough that an authoring mistake (mutual cycle) is caught quickly.
const MAX_CASCADE_DEPTH: usize = 4;

/// Server-side handler for `GameCommand::InteractWithObject`. Drains matching
/// commands from `PendingGameCommands`, applies transitions, and runs any
/// declared side-effects to depth `MAX_CASCADE_DEPTH`.
#[allow(clippy::too_many_arguments)]
pub fn process_interact_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut commands: Commands,
    mut stateful_query: Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &mut ObjectState,
        ),
        Without<Player>,
    >,
    player_query: Query<(&PlayerIdentity, &SpaceResident, &TilePosition), With<Player>>,
) {
    let drained: Vec<QueuedGameCommand> = pending_commands.commands.drain(..).collect();
    let mut remaining = Vec::with_capacity(drained.len());

    for queued in drained {
        let (object_id, verb) = match queued.command {
            GameCommand::InteractWithObject { object_id, verb } => (object_id, verb),
            GameCommand::AdminSetObjectState { object_id, state } => {
                apply_state_transition(
                    object_id,
                    &state,
                    &definitions,
                    &mut object_registry,
                    &mut commands,
                    &mut stateful_query,
                );
                continue;
            }
            other => {
                remaining.push(QueuedGameCommand {
                    player_id: queued.player_id,
                    command: other,
                });
                continue;
            }
        };

        let Some((_, player_space, player_tile)) = (match queued.player_id {
            Some(id) => player_query
                .iter()
                .find(|(identity, _, _)| identity.id == id),
            None => player_query.iter().next(),
        }) else {
            continue;
        };
        let actor_id = queued.player_id;

        // Locate the target stateful object on the same floor + Chebyshev-1.
        let Some((object_def_id, current_state)) = stateful_query
            .iter()
            .find(|(_, resident, tile, object, _)| {
                resident.space_id == player_space.space_id
                    && object.object_id == object_id
                    && is_near_player(player_tile, tile)
            })
            .map(|(_, _, _, object, state)| (object.definition_id.clone(), state.0.clone()))
        else {
            bevy::log::debug!(
                "InteractWithObject {object_id} verb='{verb}' ignored: not stateful, not nearby, or different space"
            );
            continue;
        };

        let Some(definition) = definitions.get(&object_def_id) else {
            bevy::log::debug!(
                "InteractWithObject {object_id}: missing definition '{object_def_id}'"
            );
            continue;
        };
        let Some(interaction) = definition.interaction_for(&verb, Some(&current_state)) else {
            bevy::log::debug!(
                "InteractWithObject {object_id} verb='{verb}' ignored: no matching interaction for state '{current_state}'"
            );
            continue;
        };

        let new_state = interaction.to.clone();
        let side_effects = interaction.side_effects.clone();

        apply_state_transition(
            object_id,
            &new_state,
            &definitions,
            &mut object_registry,
            &mut commands,
            &mut stateful_query,
        );

        run_side_effects(
            object_id,
            actor_id,
            &side_effects,
            &mut ui_events,
            &definitions,
            &mut object_registry,
            &mut commands,
            &mut stateful_query,
            0,
        );
    }

    pending_commands.commands = remaining;
}

/// Direct (non-verb) transition used by both the player path and side-effect
/// cascades. Updates the `ObjectState` component, mirrors the value into the
/// registry's properties bag, and toggles `Collider` to match the new state's
/// `colliding` override.
fn apply_state_transition(
    object_id: u64,
    new_state: &str,
    definitions: &OverworldObjectDefinitions,
    object_registry: &mut ObjectRegistry,
    commands: &mut Commands,
    stateful_query: &mut Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &mut ObjectState,
        ),
        Without<Player>,
    >,
) {
    let Some((entity, _, _, object, mut object_state)) = stateful_query
        .iter_mut()
        .find(|(_, _, _, object, _)| object.object_id == object_id)
    else {
        return;
    };

    if object_state.0 == new_state {
        return;
    }
    object_state.0 = new_state.to_owned();

    if let Some(properties) = object_registry.properties_mut(object_id) {
        properties.insert("state".to_owned(), new_state.to_owned());
    }

    let Some(definition) = definitions.get(&object.definition_id) else {
        return;
    };
    if definition.colliding_for_state(Some(new_state)) {
        commands.entity(entity).insert(Collider);
    } else {
        commands.entity(entity).remove::<Collider>();
    }
}

#[allow(clippy::too_many_arguments)]
fn run_side_effects(
    source_object_id: u64,
    actor: Option<PlayerId>,
    side_effects: &[InteractionSideEffect],
    ui_events: &mut PendingGameUiEvents,
    definitions: &OverworldObjectDefinitions,
    object_registry: &mut ObjectRegistry,
    commands: &mut Commands,
    stateful_query: &mut Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &mut ObjectState,
        ),
        Without<Player>,
    >,
    depth: usize,
) {
    if depth > MAX_CASCADE_DEPTH {
        bevy::log::warn!(
            "stateful-object cascade exceeded depth {} starting at object {}",
            MAX_CASCADE_DEPTH,
            source_object_id
        );
        return;
    }

    for side_effect in side_effects {
        match side_effect {
            InteractionSideEffect::SetTargetState { target, to } => {
                let Some(target_id) = resolve_target_id(target, source_object_id, object_registry)
                else {
                    continue;
                };
                // Capture the next-step's side-effects before mutating, so the
                // recursive call sees the post-transition state in the query.
                let cascade_side_effects = definitions
                    .get(&type_id_for(target_id, stateful_query))
                    .and_then(|def| {
                        def.interactions
                            .iter()
                            .find(|i| i.to == *to)
                            .map(|i| i.side_effects.clone())
                    })
                    .unwrap_or_default();
                apply_state_transition(
                    target_id,
                    to,
                    definitions,
                    object_registry,
                    commands,
                    stateful_query,
                );
                if !cascade_side_effects.is_empty() {
                    run_side_effects(
                        target_id,
                        actor,
                        &cascade_side_effects,
                        ui_events,
                        definitions,
                        object_registry,
                        commands,
                        stateful_query,
                        depth + 1,
                    );
                }
            }
            InteractionSideEffect::OpenContainerPanel => {
                if let Some(player_id) = actor {
                    ui_events.push(
                        player_id,
                        GameUiEvent::OpenContainer {
                            object_id: source_object_id,
                        },
                    );
                }
            }
        }
    }
}

/// Resolve `target` (e.g. `"{properties.target}"`) against the *source*
/// object's properties. The resulting string must parse as a runtime u64
/// (the map-load `wires_to` pass rewrites authored ids to runtime ids). When
/// resolution fails we log and skip — this is a recoverable runtime error,
/// not a panic, because the source object may have been edited at runtime.
fn resolve_target_id(
    target_template: &str,
    source_object_id: u64,
    object_registry: &ObjectRegistry,
) -> Option<u64> {
    let resolved = if let Some(stripped) = target_template
        .strip_prefix("{properties.")
        .and_then(|s| s.strip_suffix('}'))
    {
        let properties = object_registry.properties(source_object_id)?;
        properties.get(stripped)?.clone()
    } else {
        target_template.to_owned()
    };

    match resolved.parse::<u64>() {
        Ok(id) => Some(id),
        Err(_) => {
            bevy::log::warn!(
                "stateful-object side-effect: target '{}' for source {} resolved to non-numeric '{}'",
                target_template,
                source_object_id,
                resolved
            );
            None
        }
    }
}

/// Drive the visual state ("open"/"closed") of every container-type object
/// (definition has both `states.open` and `states.closed` plus a
/// `container_capacity`) from `ContainerViewers`. When at least one player is
/// looking at the panel the chest reads "open"; when the last viewer closes
/// it flips back to "closed".
pub fn sync_container_visual_state(
    container_viewers: Res<ContainerViewers>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut commands: Commands,
    mut stateful_query: Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &mut ObjectState,
        ),
        Without<Player>,
    >,
) {
    // Collect target transitions first — we need an immutable read of the
    // query before we can apply state changes back through it.
    let pending: Vec<(u64, &'static str)> = stateful_query
        .iter()
        .filter_map(|(_, _, _, object, state)| {
            let def = definitions.get(&object.definition_id)?;
            if def.container_capacity.is_none()
                || !def.states.contains_key("open")
                || !def.states.contains_key("closed")
            {
                return None;
            }
            let want = if container_viewers.has_viewers(object.object_id) {
                "open"
            } else {
                "closed"
            };
            (state.0 != want).then_some((object.object_id, want))
        })
        .collect();

    for (object_id, new_state) in pending {
        apply_state_transition(
            object_id,
            new_state,
            &definitions,
            &mut object_registry,
            &mut commands,
            &mut stateful_query,
        );
    }
}

fn type_id_for(
    object_id: u64,
    stateful_query: &Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &mut ObjectState,
        ),
        Without<Player>,
    >,
) -> String {
    stateful_query
        .iter()
        .find(|(_, _, _, object, _)| object.object_id == object_id)
        .map(|(_, _, _, object, _)| object.definition_id.clone())
        .unwrap_or_default()
}
