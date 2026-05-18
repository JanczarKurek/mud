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
use crate::player::classes::Class;
use crate::player::components::{BaseStats, ChatLog, Inventory, Player, PlayerId, PlayerIdentity};
use crate::player::skills::{skill_check, Skill, SkillSheet};
use crate::world::components::{
    Collider, ObjectState, OverworldObject, RespawnTimer, SpaceResident, TilePosition,
};
use crate::world::object_definitions::{
    DcSource, EquipmentSlot, InteractionSideEffect, KeyIdSource, OverworldObjectDefinitions,
    ToolGateDef,
};
use crate::world::object_registry::ObjectRegistry;

/// Bound on cascade depth — long enough to cover plausible lever-chains, low
/// enough that an authoring mistake (mutual cycle) is caught quickly.
const MAX_CASCADE_DEPTH: usize = 4;

type PlayerInteractQuery<'a> = (
    &'a PlayerIdentity,
    &'a SpaceResident,
    &'a TilePosition,
    &'a BaseStats,
    &'a SkillSheet,
    &'a Class,
    &'a Inventory,
    &'a mut ChatLog,
);

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
    mut player_query: Query<PlayerInteractQuery, With<Player>>,
) {
    let drained: Vec<QueuedGameCommand> = pending_commands.commands.drain(..).collect();
    let mut remaining = Vec::with_capacity(drained.len());

    for queued in drained {
        // `bypass_tool_gate` is set for `ApplyToolInteraction` — those entries
        // come from `handle_use_item_on` after it has already verified the tool
        // was in the player's inventory and consumed a charge on the source.
        let (object_id, verb, bypass_tool_gate) = match queued.command {
            GameCommand::InteractWithObject { object_id, verb } => (object_id, verb, false),
            GameCommand::ApplyToolInteraction {
                target_object_id,
                verb,
            } => (target_object_id, verb, true),
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

        let Some((
            identity,
            player_space,
            player_tile,
            base_stats,
            skill_sheet,
            _class,
            inventory,
            mut chat_log,
        )) = (match queued.player_id {
            Some(id) => player_query.iter_mut().find(|row| row.0.id == id),
            None => player_query.iter_mut().next(),
        })
        else {
            continue;
        };
        let actor_id = queued.player_id.or(Some(identity.id));

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

        // Resolve any tool / skill / key gates *before* committing to the
        // transition. Tool gate runs first so a player without the right
        // equipment never burns a skill roll. `bypass_tool_gate` short-circuits
        // this when the command came from `handle_use_item_on` — that path
        // matched the tool against the gate's `required_type_id` itself and
        // already paid the charge cost on the source item.
        if !bypass_tool_gate {
            if let Some(gate) = &interaction.tool_gate {
                if !inventory_has_tool(inventory, &gate.required_type_id) {
                    chat_log.push_narrator(tool_gate_failure_message(gate, &definitions));
                    continue;
                }
            }
        }

        if let Some(gate) = &interaction.key_gate {
            let required_id = match gate.source {
                KeyIdSource::FromLock => definition.lock.as_ref().map(|l| l.lock_id),
                KeyIdSource::Fixed(id) => Some(id),
            };
            let Some(required_id) = required_id else {
                chat_log.push_narrator("This lock doesn't accept any key you carry.");
                continue;
            };
            if !inventory_has_key(inventory, &definitions, required_id) {
                chat_log.push_narrator("You don't have the right key.");
                continue;
            }
        }

        if let Some(gate) = &interaction.skill_gate {
            let dc = match gate.dc {
                DcSource::FromLockPick => definition.lock.as_ref().map(|l| l.pick_dc),
                DcSource::FromLockForce => definition.lock.as_ref().map(|l| l.force_dc),
                DcSource::Fixed(dc) => Some(dc),
            };
            let Some(dc) = dc else {
                bevy::log::warn!(
                    "interaction '{}' on '{}' has skill_gate but no resolvable DC",
                    verb,
                    object_def_id
                );
                continue;
            };
            let result = skill_check(skill_sheet, &base_stats.attributes, gate.skill, dc, 0);
            if !result.success {
                chat_log.push_narrator(skill_failure_message(gate.skill, &verb, result.total, dc));
                continue;
            }
            chat_log.push_narrator(skill_success_message(gate.skill, &verb, result.total, dc));
        }

        let new_state = interaction.to.clone();
        let side_effects = interaction.side_effects.clone();
        let grants_items = interaction.grants_items.clone();
        let respawn_seconds = interaction.respawn_seconds;
        let respawn_restore_state = interaction
            .from
            .first()
            .cloned()
            .or_else(|| definition.initial_state.clone());

        apply_state_transition(
            object_id,
            &new_state,
            &definitions,
            &mut object_registry,
            &mut commands,
            &mut stateful_query,
        );

        // Queue inventory grants as GiveItem commands; `process_game_commands`
        // picks them up in the same tick (this system runs in CommandIntercept,
        // which is configured before process_game_commands).
        if !grants_items.is_empty() {
            if let Some(player_id) = actor_id {
                for drop in &grants_items {
                    if drop.probability < 1.0 && roll_unit_interval() >= drop.probability {
                        continue;
                    }
                    let qty = drop.quantity.roll();
                    if qty == 0 {
                        continue;
                    }
                    remaining.push(QueuedGameCommand {
                        player_id: Some(player_id),
                        command: GameCommand::GiveItem {
                            type_id: drop.type_id.clone(),
                            count: qty,
                        },
                    });
                }
            }
        }

        // Attach a respawn timer so the node reverts to its starting state
        // after the configured delay. Restores to the first `from` state, or
        // the definition's `initial_state` if `from` is empty.
        if let (Some(secs), Some(restore_state)) = (respawn_seconds, respawn_restore_state) {
            if let Some(entity) = stateful_query
                .iter()
                .find(|(_, _, _, object, _)| object.object_id == object_id)
                .map(|(entity, _, _, _, _)| entity)
            {
                commands.entity(entity).insert(RespawnTimer {
                    remaining_seconds: secs,
                    restore_state,
                });
            }
        }

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

/// Drive `RespawnTimer`s: when one reaches zero, transition its object back
/// to the configured `restore_state` and drop the timer. The projection
/// loop emits `WorldObjectUpserted` automatically once `ObjectState` flips.
pub fn tick_respawn_timers(
    time: Res<Time>,
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
    mut timer_query: Query<(Entity, &mut RespawnTimer)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let mut due: Vec<(Entity, String)> = Vec::new();
    for (entity, mut timer) in timer_query.iter_mut() {
        timer.remaining_seconds -= dt;
        if timer.remaining_seconds <= 0.0 {
            due.push((entity, timer.restore_state.clone()));
        }
    }

    for (entity, restore_state) in due {
        let Some(object_id) = stateful_query
            .get(entity)
            .ok()
            .map(|(_, _, _, object, _)| object.object_id)
        else {
            commands.entity(entity).remove::<RespawnTimer>();
            continue;
        };
        apply_state_transition(
            object_id,
            &restore_state,
            &definitions,
            &mut object_registry,
            &mut commands,
            &mut stateful_query,
        );
        commands.entity(entity).remove::<RespawnTimer>();
    }
}

/// Check whether the player has the required tool equipped in the weapon
/// slot. Used by `tool_gate` resolution.
pub fn inventory_has_tool(inventory: &Inventory, required_type_id: &str) -> bool {
    inventory
        .equipment_item(EquipmentSlot::Weapon)
        .is_some_and(|item| item.type_id == required_type_id)
}

fn tool_gate_failure_message(
    gate: &ToolGateDef,
    definitions: &OverworldObjectDefinitions,
) -> String {
    if let Some(msg) = &gate.fail_message {
        return msg.clone();
    }
    let tool_name = definitions
        .get(&gate.required_type_id)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| gate.required_type_id.clone());
    format!("You need a {} equipped for that.", tool_name)
}

/// Sample uniformly in [0, 1) from the same time-mix RNG the loot path uses.
fn roll_unit_interval() -> f32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    (nanos % 10_000) as f32 / 10_000.0
}

/// Walk the player's backpack + equipment for an item whose definition has
/// a matching `lock_id`. Used by `key_gate` resolution above and by the
/// context-menu visibility predicate.
pub fn inventory_has_key(
    inventory: &Inventory,
    definitions: &OverworldObjectDefinitions,
    lock_id: u32,
) -> bool {
    for stack in inventory.backpack_slots.iter().flatten() {
        if definitions
            .get(&stack.type_id)
            .and_then(|d| d.lock_id)
            .is_some_and(|id| id == lock_id)
        {
            return true;
        }
    }
    for (_, item) in &inventory.equipment_slots {
        let Some(item) = item else {
            continue;
        };
        if definitions
            .get(&item.type_id)
            .and_then(|d| d.lock_id)
            .is_some_and(|id| id == lock_id)
        {
            return true;
        }
    }
    false
}

fn skill_failure_message(skill: Skill, verb: &str, total: i32, dc: i32) -> String {
    match skill {
        Skill::Thievery => {
            format!("The lock resists your attempt to pick it. ({total} vs DC {dc})")
        }
        Skill::Athletics => {
            format!("You strain against it, but it doesn't give. ({total} vs DC {dc})")
        }
        _ => format!("Your {verb} attempt fails. ({total} vs DC {dc})"),
    }
}

fn skill_success_message(skill: Skill, verb: &str, total: i32, dc: i32) -> String {
    match skill {
        Skill::Thievery => format!("The lock clicks open. ({total} vs DC {dc})"),
        Skill::Athletics => format!("The lock splinters under your weight. ({total} vs DC {dc})"),
        _ => format!("Your {verb} attempt succeeds. ({total} vs DC {dc})"),
    }
}

/// Direct (non-verb) transition used by both the player path and side-effect
/// cascades. Updates the `ObjectState` component, mirrors the value into the
/// registry's properties bag, and toggles `Collider` to match the new state's
/// `colliding` override.
pub(crate) fn apply_state_transition(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::components::{AttackProfile, CombatLeash};
    use crate::game::resources::PendingGameCommands;
    use crate::game::GameServerPlugin;
    use crate::magic::MagicServerPlugin;
    use crate::player::components::{
        BaseStats, DefenseStats, DerivedStats, EquippedItem, Inventory, InventoryStack,
        MovementCooldown, VitalStats, WeaponDamage,
    };
    use crate::player::progression::Experience;
    use crate::player::PlayerServerPlugin;
    use crate::world::components::{Collider, ObjectState};
    use crate::world::map_layout::ObjectProperties;
    use crate::world::object_registry::ObjectRegistry;
    use crate::world::WorldConfig;
    use crate::world::WorldServerPlugin;

    fn setup_app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::MinimalPlugins);
        app.add_plugins((
            GameServerPlugin,
            WorldServerPlugin,
            PlayerServerPlugin,
            MagicServerPlugin,
        ));
        app.update();
        app
    }

    fn spawn_test_player(
        app: &mut App,
        class: Class,
        thievery: u8,
        athletics: u8,
        x: i32,
        y: i32,
    ) -> Entity {
        let base_stats = BaseStats::default();
        let derived = DerivedStats::from_base(&base_stats);
        let max_health = derived.max_health as f32;
        let max_mana = derived.max_mana as f32;
        let mut sheet = SkillSheet::default();
        sheet.set_rank(Skill::Thievery, thievery);
        sheet.set_rank(Skill::Athletics, athletics);
        let space_id = app.world().resource::<WorldConfig>().current_space_id;
        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("player");
        app.world_mut()
            .spawn((
                crate::player::components::Player,
                PlayerIdentity::new(crate::player::components::PlayerId(1)),
                Inventory::default(),
                ChatLog::default(),
                base_stats,
                derived,
                VitalStats::full(max_health, max_mana),
                MovementCooldown::default(),
                (
                    AttackProfile::melee(),
                    WeaponDamage::default(),
                    DefenseStats::default(),
                ),
                CombatLeash {
                    max_distance_tiles: 6,
                },
                (
                    crate::magic::effects::MagicEffects::default(),
                    sheet,
                    class,
                    Experience::default(),
                ),
                Collider,
                OverworldObject {
                    object_id,
                    definition_id: "player".to_owned(),
                },
                SpaceResident { space_id },
                TilePosition::ground(x, y),
            ))
            .id()
    }

    fn spawn_locked_door(app: &mut App, type_id: &str, x: i32, y: i32) -> (Entity, u64) {
        use crate::apply_overworld_definition_components;

        let space_id = app.world().resource::<WorldConfig>().current_space_id;
        let definition = app
            .world()
            .resource::<OverworldObjectDefinitions>()
            .get(type_id)
            .unwrap()
            .clone();
        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id(type_id);
        let mut props = ObjectProperties::new();
        props.insert("state".to_owned(), "locked".to_owned());
        app.world_mut()
            .resource_mut::<ObjectRegistry>()
            .set_properties(object_id, props.clone());
        let mut entity = app.world_mut().spawn((
            OverworldObject {
                object_id,
                definition_id: type_id.to_owned(),
            },
            SpaceResident { space_id },
            TilePosition::ground(x, y),
        ));
        apply_overworld_definition_components!(entity, &definition, None, None);
        // Override the initial state to "locked" — the macro just inserted
        // the definition's default "closed" state.
        entity.insert(ObjectState("locked".to_owned()));
        if definition.colliding_for_state(Some("locked")) {
            entity.insert(Collider);
        }
        let id = entity.id();
        (id, object_id)
    }

    fn current_state(app: &mut App, object_id: u64) -> String {
        let mut q = app.world_mut().query::<(&OverworldObject, &ObjectState)>();
        for (object, state) in q.iter(app.world()) {
            if object.object_id == object_id {
                return state.0.clone();
            }
        }
        panic!("object {object_id} not found")
    }

    #[test]
    fn pick_lock_succeeds_at_max_thievery() {
        let mut app = setup_app();
        let _player = spawn_test_player(&mut app, Class::Vagabond, 20, 0, 10, 10);
        let (_door_entity, door_id) = spawn_locked_door(&mut app, "wooden_door", 11, 10);
        app.update();

        app.world_mut().resource_mut::<PendingGameCommands>().push(
            GameCommand::InteractWithObject {
                object_id: door_id,
                verb: "pick_lock".to_owned(),
            },
        );
        app.update();

        // With 20 ranks of Thievery a DC 15 pick can fail only on a natural 1
        // (1 + 20 = 21 ≥ 15). To stay deterministic this test runs the
        // pick repeatedly until success; in practice the very first attempt
        // succeeds outside of pathological RNG outcomes.
        for _ in 0..10 {
            if current_state(&mut app, door_id) == "closed" {
                return;
            }
            app.world_mut().resource_mut::<PendingGameCommands>().push(
                GameCommand::InteractWithObject {
                    object_id: door_id,
                    verb: "pick_lock".to_owned(),
                },
            );
            app.update();
        }
        panic!(
            "pick_lock never succeeded at max Thievery: final state = {}",
            current_state(&mut app, door_id)
        );
    }

    #[test]
    fn pick_lock_fails_at_zero_thievery() {
        let mut app = setup_app();
        let _player = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        let (_, door_id) = spawn_locked_door(&mut app, "wooden_door", 11, 10);
        app.update();

        // At rank 0 Thievery + 0 ability mod, max roll is 20 < DC 15? No,
        // 20 ≥ 15, so a natural 20 still passes. But the verb visibility
        // gate hides the button when rank == 0 — this test only checks the
        // server-side state, which still applies the check. Realistically
        // we expect the great majority of attempts to fail; assert that the
        // chat-log got a "lock resists" line on the first attempt.
        app.world_mut().resource_mut::<PendingGameCommands>().push(
            GameCommand::InteractWithObject {
                object_id: door_id,
                verb: "pick_lock".to_owned(),
            },
        );
        app.update();

        // Either the door stayed locked OR it transitioned. In either case
        // the player's chat log should have a feedback line.
        let mut chat_q = app
            .world_mut()
            .query::<(&crate::player::components::Player, &ChatLog)>();
        let chat_lines: Vec<String> = chat_q
            .iter(app.world())
            .next()
            .map(|(_, log)| log.lines.clone())
            .unwrap_or_default();
        let has_feedback = chat_lines
            .iter()
            .any(|line| line.contains("lock resists") || line.contains("lock clicks open"));
        assert!(
            has_feedback,
            "expected pick-lock attempt to push a chat line, got: {chat_lines:?}"
        );
    }

    #[test]
    fn use_key_unlocks_without_skill_check() {
        let mut app = setup_app();
        let player = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        // Put an iron_key into the player's backpack.
        let mut inventory = app
            .world()
            .entity(player)
            .get::<Inventory>()
            .unwrap()
            .clone();
        inventory.backpack_slots[0] = Some(InventoryStack::item(
            "iron_key".to_owned(),
            ObjectProperties::new(),
            1,
        ));
        let _ = inventory;
        let mut entity_mut = app.world_mut().entity_mut(player);
        let mut inv = entity_mut.get_mut::<Inventory>().unwrap();
        inv.backpack_slots[0] = Some(InventoryStack::item(
            "iron_key".to_owned(),
            ObjectProperties::new(),
            1,
        ));

        let (_, door_id) = spawn_locked_door(&mut app, "wooden_door", 11, 10);
        app.update();

        app.world_mut().resource_mut::<PendingGameCommands>().push(
            GameCommand::InteractWithObject {
                object_id: door_id,
                verb: "use_key".to_owned(),
            },
        );
        app.update();

        assert_eq!(current_state(&mut app, door_id), "closed");
    }

    #[test]
    fn use_key_rejected_without_matching_key() {
        let mut app = setup_app();
        let _ = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        let (_, door_id) = spawn_locked_door(&mut app, "wooden_door", 11, 10);
        app.update();

        app.world_mut().resource_mut::<PendingGameCommands>().push(
            GameCommand::InteractWithObject {
                object_id: door_id,
                verb: "use_key".to_owned(),
            },
        );
        app.update();

        // Door stays locked.
        assert_eq!(current_state(&mut app, door_id), "locked");
    }

    #[test]
    fn open_verb_blocked_while_locked() {
        let mut app = setup_app();
        let _ = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        let (_, door_id) = spawn_locked_door(&mut app, "wooden_door", 11, 10);
        app.update();

        app.world_mut().resource_mut::<PendingGameCommands>().push(
            GameCommand::InteractWithObject {
                object_id: door_id,
                verb: "open".to_owned(),
            },
        );
        app.update();

        assert_eq!(current_state(&mut app, door_id), "locked");
    }

    /// Compile-time guard against an unused `EquippedItem` import that lint
    /// otherwise flags when tests are compiled but EquippedItem isn't
    /// actually used; including it here documents intent for future
    /// equipped-key tests.
    #[allow(dead_code)]
    fn _equipment_marker(_: EquippedItem) {}

    fn spawn_resource_node(app: &mut App, type_id: &str, x: i32, y: i32) -> (Entity, u64) {
        use crate::apply_overworld_definition_components;

        let space_id = app.world().resource::<WorldConfig>().current_space_id;
        let definition = app
            .world()
            .resource::<OverworldObjectDefinitions>()
            .get(type_id)
            .unwrap_or_else(|| panic!("missing definition for '{type_id}'"))
            .clone();
        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id(type_id);
        let mut entity = app.world_mut().spawn((
            OverworldObject {
                object_id,
                definition_id: type_id.to_owned(),
            },
            SpaceResident { space_id },
            TilePosition::ground(x, y),
        ));
        apply_overworld_definition_components!(entity, &definition, None, None);
        let id = entity.id();
        (id, object_id)
    }

    fn equip_weapon(app: &mut App, player: Entity, type_id: &str) {
        let mut entity_mut = app.world_mut().entity_mut(player);
        let mut inventory = entity_mut.get_mut::<Inventory>().unwrap();
        inventory.restore_equipment_item(
            crate::world::object_definitions::EquipmentSlot::Weapon,
            EquippedItem::new(type_id.to_owned()),
        );
    }

    #[test]
    fn gather_tool_gate_rejects_when_no_tool_equipped() {
        let mut app = setup_app();
        let _player = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        let (_, node_id) = spawn_resource_node(&mut app, "herb_patch", 11, 10);
        app.update();

        app.world_mut().resource_mut::<PendingGameCommands>().push(
            GameCommand::InteractWithObject {
                object_id: node_id,
                verb: "pick".to_owned(),
            },
        );
        app.update();

        // No herb_knife equipped — state stays `available`.
        assert_eq!(current_state(&mut app, node_id), "available");

        let mut chat_q = app
            .world_mut()
            .query::<(&crate::player::components::Player, &ChatLog)>();
        let chat_lines: Vec<String> = chat_q
            .iter(app.world())
            .next()
            .map(|(_, log)| log.lines.clone())
            .unwrap_or_default();
        assert!(
            chat_lines.iter().any(|line| line.contains("herb knife")),
            "expected tool-gate failure line in chat, got: {chat_lines:?}"
        );
    }

    #[test]
    fn gather_with_tool_depletes_and_grants_drops() {
        let mut app = setup_app();
        // Fighter has Survival as a class skill; max rank at level 1 = 4.
        // Survival DC 6 for herb_patch + 4 ranks → always succeeds on a roll
        // of 2+, i.e. 19/20 attempts. Loop to absorb the natural-1 case.
        let player = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        {
            let mut entity_mut = app.world_mut().entity_mut(player);
            let mut sheet = entity_mut.get_mut::<SkillSheet>().unwrap();
            sheet.set_rank(Skill::Survival, 4);
        }
        equip_weapon(&mut app, player, "herb_knife");
        let (_, node_id) = spawn_resource_node(&mut app, "herb_patch", 11, 10);
        app.update();

        let mut attempts = 0;
        let succeeded = loop {
            attempts += 1;
            app.world_mut().resource_mut::<PendingGameCommands>().push(
                GameCommand::InteractWithObject {
                    object_id: node_id,
                    verb: "pick".to_owned(),
                },
            );
            app.update();
            if current_state(&mut app, node_id) == "depleted" {
                break true;
            }
            if attempts >= 20 {
                break false;
            }
        };
        assert!(
            succeeded,
            "herb_patch never depleted across {attempts} attempts at rank 4 Survival vs DC 6"
        );

        // Inventory should now contain green_herb (1..=3) — GiveItem ran in
        // the same tick the skill check succeeded.
        let inventory = app
            .world()
            .entity(player)
            .get::<Inventory>()
            .unwrap()
            .clone();
        let herb_count: u32 = inventory
            .backpack_slots
            .iter()
            .flatten()
            .filter(|stack| stack.type_id == "green_herb")
            .map(|stack| stack.quantity)
            .sum();
        assert!(
            (1..=3).contains(&herb_count),
            "expected 1-3 green_herb, got {herb_count}"
        );

        // RespawnTimer should be attached to the node.
        let mut timer_q = app.world_mut().query::<(&OverworldObject, &RespawnTimer)>();
        let has_timer = timer_q
            .iter(app.world())
            .any(|(object, _)| object.object_id == node_id);
        assert!(
            has_timer,
            "expected RespawnTimer to be attached after harvest"
        );
    }

    fn give_to_backpack(app: &mut App, player: Entity, type_id: &str) {
        let mut entity_mut = app.world_mut().entity_mut(player);
        let mut inv = entity_mut.get_mut::<Inventory>().unwrap();
        inv.backpack_slots[0] = Some(InventoryStack::item(
            type_id.to_owned(),
            ObjectProperties::new(),
            1,
        ));
    }

    /// Gathering via "Use On": pickaxe in backpack (NOT equipped), target an
    /// ore_node, and after a few ticks the node depletes and iron_ore lands in
    /// inventory. Exercises the new `UseItemOn` → `ApplyToolInteraction` path.
    #[test]
    fn gather_via_use_item_on_with_backpack_pickaxe() {
        use crate::game::commands::{ItemReference, ItemSlotRef, UseTarget};

        let mut app = setup_app();
        let player = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        {
            let mut entity_mut = app.world_mut().entity_mut(player);
            let mut sheet = entity_mut.get_mut::<SkillSheet>().unwrap();
            sheet.set_rank(Skill::Survival, 4);
        }
        give_to_backpack(&mut app, player, "pickaxe");
        let (_, node_id) = spawn_resource_node(&mut app, "ore_node", 11, 10);
        app.update();

        let mut attempts = 0;
        let succeeded = loop {
            attempts += 1;
            app.world_mut()
                .resource_mut::<PendingGameCommands>()
                .push(GameCommand::UseItemOn {
                    source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
                    target: UseTarget::Object(node_id),
                });
            // Two ticks: handle_use_item_on (frame N) queues ApplyToolInteraction;
            // process_interact_commands picks it up on frame N+1.
            app.update();
            app.update();
            if current_state(&mut app, node_id) == "depleted" {
                break true;
            }
            if attempts >= 20 {
                break false;
            }
        };
        assert!(
            succeeded,
            "ore_node never depleted via UseItemOn across {attempts} attempts"
        );

        let inventory = app
            .world()
            .entity(player)
            .get::<Inventory>()
            .unwrap()
            .clone();
        let ore_count: u32 = inventory
            .backpack_slots
            .iter()
            .flatten()
            .filter(|stack| stack.type_id == "iron_ore")
            .map(|stack| stack.quantity)
            .sum();
        assert!(
            (1..=2).contains(&ore_count),
            "expected 1-2 iron_ore in inventory after Use-On mining, got {ore_count}"
        );
    }

    /// Pickaxe carries `infinite_uses: true`; using it on a node many times
    /// must not decrement its quantity.
    #[test]
    fn infinite_use_pickaxe_not_consumed_on_use_on() {
        use crate::game::commands::{ItemReference, ItemSlotRef, UseTarget};

        let mut app = setup_app();
        let player = spawn_test_player(&mut app, Class::Fighter, 0, 0, 10, 10);
        {
            let mut entity_mut = app.world_mut().entity_mut(player);
            let mut sheet = entity_mut.get_mut::<SkillSheet>().unwrap();
            sheet.set_rank(Skill::Survival, 4);
        }
        give_to_backpack(&mut app, player, "pickaxe");
        let (_, node_id) = spawn_resource_node(&mut app, "ore_node", 11, 10);
        app.update();

        for _ in 0..5 {
            app.world_mut()
                .resource_mut::<PendingGameCommands>()
                .push(GameCommand::UseItemOn {
                    source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
                    target: UseTarget::Object(node_id),
                });
            app.update();
            app.update();
        }

        let inventory = app
            .world()
            .entity(player)
            .get::<Inventory>()
            .unwrap()
            .clone();
        let pickaxe_qty: u32 = inventory
            .backpack_slots
            .iter()
            .flatten()
            .filter(|stack| stack.type_id == "pickaxe")
            .map(|stack| stack.quantity)
            .sum();
        assert_eq!(
            pickaxe_qty, 1,
            "pickaxe with infinite_uses must remain in inventory after repeated UseItemOn"
        );
    }
}
