use bevy::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::combat::components::CombatTarget;
use crate::game::commands::{
    GameCommand, InspectTarget, ItemDestination, ItemReference, ItemSlotRef, MoveDelta, UseTarget,
};
use crate::game::helpers::{colliders_in_space, is_near_player, player_space_id};
use crate::game::resources::{
    ChatLogState, ContainerViewers, GameUiEvent, InventoryState, PendingGameCommands,
    PendingGameUiEvents,
};
use crate::magic::resources::{SpellDefinition, SpellDefinitions};
use crate::npc::components::Npc;
use crate::player::components::{
    DerivedStats, EquippedItem, InventoryStack, MovementCooldown, Player, PlayerIdentity,
    VitalStats,
};
use crate::world::components::{
    Collider, Container, Facing, Movable, OverworldObject, Quantity, Rotatable, SpaceResident,
    TilePosition,
};
use crate::world::direction::Direction;
use crate::world::floor_map::FloorMaps;
use crate::world::loot::spawn_corpse_for_npc;
use crate::world::map_layout::{ObjectProperties, SpaceDefinitions};
use crate::world::object_definitions::{
    EquipmentSlot, OverworldObjectDefinition, OverworldObjectDefinitions,
};
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::setup::{resolve_portal_destination_space, spawn_overworld_object};
use crate::world::WorldConfig;
use bevy::ecs::system::SystemParam;

/// Bundle of side-output channels needed by `process_game_commands`. Bevy's
/// `IntoSystem` impl caps individual function-parameter count, so we pack
/// these together to leave headroom for the existing query mix.
#[derive(SystemParam)]
pub struct CommandOutputs<'w> {
    pub ui_events: ResMut<'w, PendingGameUiEvents>,
    pub container_viewers: ResMut<'w, ContainerViewers>,
}

/// Bundle of resources needed together when a command may cause space
/// instantiation (portals). Kept as one `SystemParam` so `process_game_commands`
/// stays under Bevy's system parameter-count limit.
#[derive(SystemParam)]
pub struct SpaceAuthority<'w> {
    pub space_manager: ResMut<'w, SpaceManager>,
    pub floor_maps: ResMut<'w, FloorMaps>,
}

pub fn tick_player_movement_cooldowns(
    time: Res<Time>,
    mut player_query: Query<&mut MovementCooldown, With<Player>>,
) {
    for mut movement_cooldown in &mut player_query {
        movement_cooldown.remaining_seconds =
            (movement_cooldown.remaining_seconds - time.delta_secs()).max(0.0);
    }
}

/// Drains `GameCommand::RotateObject` from `PendingGameCommands` and applies the
/// rotation to the target object's `Facing`. Scheduled in `CommandIntercept`
/// (before `process_game_commands`) so the main processor's param list does not
/// need to grow. Validation: target must have `Rotatable` + sit within
/// Chebyshev-1 of the acting player (same adjacency rule as move-item).
pub fn process_rotate_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut rotatable_query: Query<
        (&SpaceResident, &TilePosition, &OverworldObject, &mut Facing),
        (With<Rotatable>, Without<Player>),
    >,
    player_query: Query<(&PlayerIdentity, &SpaceResident, &TilePosition), With<Player>>,
) {
    let original_len = pending_commands.commands.len();
    let mut remaining = Vec::with_capacity(original_len);
    let drained: Vec<_> = pending_commands.commands.drain(..).collect();

    for queued in drained {
        let (object_id, rotation) = match queued.command {
            GameCommand::RotateObject {
                object_id,
                rotation,
            } => (object_id, rotation),
            other => {
                remaining.push(crate::game::resources::QueuedGameCommand {
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

        let Some((_, _, _, mut facing)) =
            rotatable_query
                .iter_mut()
                .find(|(resident, tile_position, object, _)| {
                    resident.space_id == player_space.space_id
                        && object.object_id == object_id
                        && is_near_player(player_tile, tile_position)
                })
        else {
            bevy::log::debug!(
                "RotateObject {object_id} ignored: not rotatable, not nearby, or different space"
            );
            continue;
        };

        facing.0 = rotation.apply(facing.0);
    }

    pending_commands.commands = remaining;
}

/// Drains `GameCommand::EditorSetFloorTile` from `PendingGameCommands` and
/// mutates `FloorMaps` directly. Scheduled in `CommandIntercept` so the main
/// `process_game_commands` system parameter list does not have to grow.
pub fn process_floor_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut floor_maps: ResMut<FloorMaps>,
) {
    let original_len = pending_commands.commands.len();
    let mut remaining = Vec::with_capacity(original_len);
    let drained: Vec<_> = pending_commands.commands.drain(..).collect();

    for queued in drained {
        match queued.command {
            GameCommand::EditorSetFloorTile {
                space_id,
                z,
                x,
                y,
                floor_type,
            } => {
                if let Some(map) = floor_maps.get_mut(space_id, z) {
                    if !map.set(x, y, floor_type) {
                        bevy::log::warn!(
                            "EditorSetFloorTile: ({},{}) out of bounds for space {} z={}",
                            x,
                            y,
                            space_id.0,
                            z
                        );
                    }
                } else {
                    bevy::log::warn!(
                        "EditorSetFloorTile: no floor map for space {} z={}",
                        space_id.0,
                        z
                    );
                }
            }
            other => remaining.push(crate::game::resources::QueuedGameCommand {
                player_id: queued.player_id,
                command: other,
            }),
        }
    }

    pending_commands.commands = remaining;
}

pub fn process_game_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut command_outputs: CommandOutputs,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    authored_spaces: Res<SpaceDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut space_authority: SpaceAuthority,
    world_config: Res<WorldConfig>,
    object_query: Query<(Entity, &SpaceResident, &TilePosition, &OverworldObject), Without<Player>>,
    movable_query: Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    mut container_query: Query<&mut Container>,
    mut player_queries: ParamSet<(
        Query<(&SpaceResident, &TilePosition), With<Collider>>,
        Query<
            (
                Entity,
                &PlayerIdentity,
                &SpaceResident,
                &TilePosition,
                &OverworldObject,
            ),
            With<Player>,
        >,
        Query<
            (
                Entity,
                &PlayerIdentity,
                &mut InventoryState,
                &mut ChatLogState,
                &mut SpaceResident,
                &mut TilePosition,
                &mut MovementCooldown,
                &mut VitalStats,
                Option<&CombatTarget>,
            ),
            With<Player>,
        >,
    )>,
    mut npc_vitals_query: Query<(&mut VitalStats, &OverworldObject), (With<Npc>, Without<Player>)>,
    quantity_query: Query<&Quantity>,
    player_derived_stats_query: Query<&DerivedStats, With<Player>>,
    mut commands: Commands,
) {
    let queued_commands = std::mem::take(&mut pending_commands.commands);

    for queued_command in queued_commands {
        let Some(player_entity) =
            resolve_player_entity(queued_command.player_id, &player_queries.p1())
        else {
            continue;
        };

        match queued_command.command {
            GameCommand::MovePlayer { delta } => {
                let Some(source_space_id) = player_space_id(player_entity, &player_queries.p1())
                else {
                    continue;
                };
                let collider_positions = colliders_in_space(source_space_id, &player_queries.p0());
                handle_move_player(
                    player_entity,
                    delta,
                    &collider_positions,
                    &object_query,
                    &mut player_queries.p2(),
                    &authored_spaces,
                    &definitions,
                    &mut space_authority.space_manager,
                    &mut space_authority.floor_maps,
                    &mut commands,
                );
            }
            GameCommand::SetCombatTarget { target_object_id } => {
                let Some(source_space_id) = player_space_id(player_entity, &player_queries.p1())
                else {
                    continue;
                };
                let target_entity = target_object_id.and_then(|object_id| {
                    find_combat_target_entity(
                        object_id,
                        source_space_id,
                        &object_query,
                        &player_queries.p1(),
                    )
                });
                handle_set_combat_target(
                    player_entity,
                    target_object_id,
                    target_entity,
                    &mut player_queries.p2(),
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut commands,
                );
            }
            GameCommand::OpenContainer { object_id } => {
                handle_open_container(
                    player_entity,
                    object_id,
                    &object_query,
                    &mut container_query,
                    &mut player_queries.p2(),
                    &mut command_outputs.ui_events,
                    &mut command_outputs.container_viewers,
                );
            }
            GameCommand::CloseContainer { object_id } => {
                if let Ok((_, identity, _, _, _, _, _, _, _)) =
                    player_queries.p2().get(player_entity)
                {
                    command_outputs
                        .container_viewers
                        .remove(object_id, identity.id);
                }
            }
            GameCommand::Inspect { target } => {
                handle_inspect(
                    player_entity,
                    target,
                    &mut container_query,
                    &object_query,
                    &quantity_query,
                    &mut player_queries.p2(),
                    &player_derived_stats_query,
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                );
            }
            GameCommand::UseItem { source } => {
                handle_use_item(
                    player_entity,
                    source,
                    &mut container_query,
                    &object_query,
                    &mut player_queries.p2(),
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut commands,
                );
            }
            GameCommand::UseItemOn { source, target } => {
                handle_use_item_on(
                    player_entity,
                    source,
                    target,
                    &mut container_query,
                    &mut player_queries.p2(),
                    &object_query,
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut commands,
                );
            }
            GameCommand::CastSpellAt {
                source,
                spell_id,
                target_object_id,
            } => {
                handle_cast_spell_at(
                    player_entity,
                    source,
                    &spell_id,
                    target_object_id,
                    &mut container_query,
                    &object_query,
                    &mut player_queries.p2(),
                    &mut npc_vitals_query,
                    &mut object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut commands,
                );
            }
            GameCommand::MoveItem {
                source,
                destination,
            } => {
                let Some(source_space_id) = player_space_id(player_entity, &player_queries.p1())
                else {
                    continue;
                };
                let collider_positions = colliders_in_space(source_space_id, &player_queries.p0());
                handle_move_item(
                    player_entity,
                    source,
                    destination,
                    &mut container_query,
                    &mut player_queries.p2(),
                    &collider_positions,
                    &movable_query,
                    &object_query,
                    &quantity_query,
                    &mut object_registry,
                    &definitions,
                    &world_config,
                    &mut commands,
                );
            }
            GameCommand::TakeFromStack {
                source,
                amount,
                destination,
            } => {
                let Some(source_space_id) = player_space_id(player_entity, &player_queries.p1())
                else {
                    continue;
                };
                let collider_positions = colliders_in_space(source_space_id, &player_queries.p0());
                handle_take_from_stack(
                    player_entity,
                    source,
                    amount,
                    destination,
                    &mut container_query,
                    &mut player_queries.p2(),
                    &collider_positions,
                    &movable_query,
                    &object_query,
                    &quantity_query,
                    &mut object_registry,
                    &definitions,
                    &world_config,
                    &mut commands,
                );
            }
            GameCommand::AdminSpawn {
                type_id,
                tile_position,
            } => {
                let Some(source_space_id) = player_space_id(player_entity, &player_queries.p1())
                else {
                    continue;
                };
                let collider_positions = colliders_in_space(source_space_id, &player_queries.p0());
                handle_admin_spawn(
                    player_entity,
                    &type_id,
                    tile_position,
                    &definitions,
                    &world_config,
                    &collider_positions,
                    &mut object_registry,
                    &mut commands,
                    &mut player_queries.p2(),
                );
            }
            GameCommand::GiveItem { type_id, count } => {
                handle_give_item(
                    player_entity,
                    &type_id,
                    count,
                    &definitions,
                    &mut player_queries.p2(),
                );
            }
            GameCommand::TakeItem { type_id, count } => {
                handle_take_item(player_entity, &type_id, count, &mut player_queries.p2());
            }
            GameCommand::EditorSetFloorTile { .. } => {
                // Drained by `process_floor_commands` in `CommandIntercept` before this system runs.
                bevy::log::warn!(
                    "process_game_commands saw EditorSetFloorTile — check system ordering"
                );
            }
            GameCommand::TalkToNpc { .. }
            | GameCommand::DialogAdvance { .. }
            | GameCommand::DialogChoose { .. }
            | GameCommand::DialogEnd { .. } => {
                // Dialog commands are drained by `process_dialog_commands`
                // before this system runs. If one slips through here it
                // means the scheduler ran us out of order.
                bevy::log::warn!(
                    "process_game_commands saw a dialog command — check system ordering"
                );
            }
            GameCommand::RotateObject { .. } => {
                // Rotate commands are drained by `process_rotate_commands`
                // in `CommandIntercept` before this system runs.
                bevy::log::warn!(
                    "process_game_commands saw a rotate command — check system ordering"
                );
            }
            GameCommand::InteractWithObject { .. } => {
                // Drained by `process_interact_commands` in `CommandIntercept`.
                bevy::log::warn!(
                    "process_game_commands saw an interact command — check system ordering"
                );
            }
        }
    }
}

fn handle_give_item(
    player_entity: Entity,
    type_id: &str,
    count: u32,
    definitions: &OverworldObjectDefinitions,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
) {
    if count == 0 {
        return;
    }
    let Some(definition) = definitions.get(type_id) else {
        bevy::log::warn!("GiveItem: unknown type_id '{type_id}'");
        return;
    };
    let Ok((_, _, mut inventory, mut chat_log, _, _, _, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };

    let max_stack = definition.max_stack_size.max(1);
    let mut remaining = count;

    if max_stack > 1 {
        for slot in inventory.backpack_slots.iter_mut() {
            if remaining == 0 {
                break;
            }
            let Some(stack) = slot else { continue };
            if stack.type_id != type_id {
                continue;
            }
            let available = max_stack.saturating_sub(stack.quantity);
            if available == 0 {
                continue;
            }
            let grant = remaining.min(available);
            stack.quantity += grant;
            remaining -= grant;
        }
    }

    while remaining > 0 {
        let Some(empty_index) = inventory
            .backpack_slots
            .iter()
            .position(|slot| slot.is_none())
        else {
            chat_log.push_narrator(format!("You cannot carry any more {}.", definition.name));
            break;
        };
        let grant = if max_stack > 1 {
            remaining.min(max_stack)
        } else {
            1
        };
        inventory.backpack_slots[empty_index] = Some(InventoryStack {
            type_id: type_id.to_owned(),
            properties: ObjectProperties::new(),
            quantity: grant,
        });
        remaining -= grant;
    }

    chat_log.push_narrator(format!(
        "You receive {} {}.",
        count - remaining,
        definition.name
    ));
}

fn handle_take_item(
    player_entity: Entity,
    type_id: &str,
    count: u32,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
) {
    if count == 0 {
        return;
    }
    let Ok((_, _, mut inventory, _, _, _, _, _, _)) = player_query.get_mut(player_entity) else {
        return;
    };

    let mut remaining = count;
    for slot in inventory.backpack_slots.iter_mut() {
        if remaining == 0 {
            break;
        }
        let Some(stack) = slot else { continue };
        if stack.type_id != type_id {
            continue;
        }
        if stack.quantity <= remaining {
            remaining -= stack.quantity;
            *slot = None;
        } else {
            stack.quantity -= remaining;
            remaining = 0;
        }
    }
}

fn handle_move_player(
    player_entity: Entity,
    delta: MoveDelta,
    collider_positions: &[TilePosition],
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    authored_spaces: &SpaceDefinitions,
    definitions: &OverworldObjectDefinitions,
    space_manager: &mut SpaceManager,
    floor_maps: &mut FloorMaps,
    commands: &mut Commands,
) {
    let Ok((
        _,
        _,
        _,
        mut chat_log_state,
        mut space_resident,
        mut tile_position,
        mut movement_cooldown,
        _,
        _,
    )) = player_query.get_mut(player_entity)
    else {
        return;
    };

    if movement_cooldown.remaining_seconds > 0.0 {
        return;
    }

    let Some(runtime_space) = space_manager.get(space_resident.space_id).cloned() else {
        return;
    };

    let target_position = TilePosition::new(
        (tile_position.x + delta.x).clamp(0, runtime_space.width - 1),
        (tile_position.y + delta.y).clamp(0, runtime_space.height - 1),
        tile_position.z,
    );

    if !is_walkable_tile(
        target_position,
        space_resident.space_id,
        collider_positions,
        object_query,
        definitions,
    ) {
        return;
    }

    *tile_position = target_position;
    movement_cooldown.remaining_seconds = movement_cooldown.step_interval_seconds;

    if let Some(direction) = Direction::from_delta(delta.x, delta.y) {
        commands.entity(player_entity).insert(Facing(direction));
    }

    // Stair transition: if the tile the player just stepped onto carries a
    // `floor_transition`, bump z by its delta (unless the destination is blocked).
    let stairs_delta = object_query
        .iter()
        .filter(|(_, resident, tile, _)| {
            resident.space_id == space_resident.space_id && **tile == target_position
        })
        .find_map(|(_, _, _, object)| {
            definitions
                .get(&object.definition_id)
                .and_then(|def| def.floor_transition.as_ref())
                .map(|transition| transition.delta)
        });
    if let Some(delta_z) = stairs_delta {
        let stair_destination = TilePosition::new(
            target_position.x,
            target_position.y,
            target_position.z + delta_z,
        );
        if is_walkable_tile(
            stair_destination,
            space_resident.space_id,
            collider_positions,
            object_query,
            definitions,
        ) {
            *tile_position = stair_destination;
        } else {
            chat_log_state.push_narrator("The way is blocked.");
        }
    }

    let Some(space_definition) = authored_spaces.get(&runtime_space.authored_id) else {
        return;
    };
    let Some(portal) = space_definition.portal_at(target_position) else {
        return;
    };
    let Some(destination_space_id) = resolve_portal_destination_space(
        commands,
        authored_spaces,
        definitions,
        space_manager,
        floor_maps,
        space_resident.space_id,
        portal,
    ) else {
        return;
    };

    space_resident.space_id = destination_space_id;
    *tile_position = portal.destination_tile.to_tile_position();
}

fn handle_set_combat_target(
    player_entity: Entity,
    target_object_id: Option<u64>,
    target_entity: Option<Entity>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    commands: &mut Commands,
) {
    let Ok((_, _, _, mut chat_log_state, _, _, _, _, _)) = player_query.get_mut(player_entity)
    else {
        return;
    };

    match target_object_id {
        Some(object_id) => {
            let Some(target_entity) = target_entity else {
                return;
            };
            commands.entity(player_entity).insert(CombatTarget {
                entity: target_entity,
            });

            if let Some(target_name) =
                object_registry.display_name(object_id, definitions, spell_definitions)
            {
                chat_log_state.push_narrator(format!("Targeting {target_name}."));
            }
        }
        None => {
            commands.entity(player_entity).remove::<CombatTarget>();
        }
    }
}

fn handle_open_container(
    player_entity: Entity,
    object_id: u64,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    container_query: &mut Query<&mut Container>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    ui_events: &mut PendingGameUiEvents,
    container_viewers: &mut ContainerViewers,
) {
    let Ok((
        _,
        player_identity,
        _,
        mut chat_log_state,
        player_space_resident,
        player_position,
        _,
        _,
        _,
    )) = player_query.get_mut(player_entity)
    else {
        return;
    };
    let Some((entity, tile_position)) =
        find_object_entity(object_id, player_space_resident.space_id, object_query)
    else {
        return;
    };

    if container_query.get_mut(entity).is_err() || !is_near_player(&player_position, &tile_position)
    {
        chat_log_state.push_narrator("That container is out of reach.");
        return;
    }

    container_viewers.insert(object_id, player_identity.id);
    ui_events.push(player_identity.id, GameUiEvent::OpenContainer { object_id });
}

fn handle_inspect(
    player_entity: Entity,
    target: InspectTarget,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    quantity_query: &Query<&Quantity>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    player_derived_stats_query: &Query<&DerivedStats, With<Player>>,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
) {
    // Resolve (type_id, properties, count) and, for world targets, the object's tile.
    let result: Option<(String, ObjectProperties, u32, Option<TilePosition>)> = {
        let Ok((_, _, inventory_state, _, _, _, _, _, _)) = player_query.get(player_entity) else {
            return;
        };
        match target {
            InspectTarget::Object(id) => {
                let entry = object_query
                    .iter()
                    .find(|(_, _, _, obj)| obj.object_id == id)
                    .map(|(e, _, tile, obj)| (e, *tile, obj.definition_id.clone()));
                let count = entry
                    .as_ref()
                    .and_then(|(e, _, _)| quantity_query.get(*e).ok())
                    .map(|q| q.0)
                    .unwrap_or(1);
                entry.map(|(_, tile, def_id)| {
                    let properties = object_registry.properties(id).cloned().unwrap_or_default();
                    (def_id, properties, count, Some(tile))
                })
            }
            InspectTarget::SlotItem(slot_ref) => match slot_ref {
                ItemSlotRef::Backpack(idx) => inventory_state
                    .backpack_slots
                    .get(idx)
                    .cloned()
                    .flatten()
                    .map(|s| (s.type_id, s.properties, s.quantity, None)),
                ItemSlotRef::Equipment(slot) => inventory_state
                    .equipment_item(slot)
                    .map(|item| (item.type_id.clone(), item.properties.clone(), 1u32, None)),
                ItemSlotRef::Container { .. } => None, // resolved below with container_query
            },
        }
    };

    let result = result.or_else(|| {
        if let InspectTarget::SlotItem(ItemSlotRef::Container {
            object_id,
            slot_index,
        }) = target
        {
            let entity = find_container_entity(object_id, object_query)?;
            let container = container_query.get_mut(entity).ok()?;
            container
                .slots
                .get(slot_index)
                .cloned()
                .flatten()
                .map(|s| (s.type_id, s.properties, s.quantity, None))
        } else {
            None
        }
    });

    let Some((type_id, properties, count, world_tile)) = result else {
        return;
    };

    // For world-object inspects, gate on perception-driven distance.
    let too_far = if let Some(target_tile) = world_tile {
        let Ok((_, _, _, _, _, player_position, _, _, _)) = player_query.get(player_entity) else {
            return;
        };
        let player_position = *player_position;
        let focus = player_derived_stats_query
            .get(player_entity)
            .map(|stats| stats.attributes.focus)
            .unwrap_or(0);
        let base = definitions
            .get(&type_id)
            .and_then(|def| def.inspect_range)
            .unwrap_or(DEFAULT_INSPECT_RANGE);
        let effective_range = (base + focus / FOCUS_TILES_PER_POINT).max(1);
        chebyshev_distance_tiles(player_position, target_tile) > effective_range
    } else {
        false
    };

    if too_far {
        if let Ok((_, _, _, mut chat_log, _, _, _, _, _)) = player_query.get_mut(player_entity) {
            chat_log.push_narrator("You stand too far to see it clearly.");
        }
        return;
    }

    let description =
        object_description_for_type(&type_id, &properties, count, definitions, spell_definitions);

    if let (Some(desc), Ok((_, _, _, mut chat_log, _, _, _, _, _))) =
        (description, player_query.get_mut(player_entity))
    {
        chat_log.push_narrator(desc);
    }
}

const DEFAULT_INSPECT_RANGE: i32 = 3;
const FOCUS_TILES_PER_POINT: i32 = 5;

fn handle_use_item(
    player_entity: Entity,
    source: ItemReference,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    commands: &mut Commands,
) {
    let Ok((
        _,
        _,
        mut inventory_state,
        mut chat_log_state,
        _,
        player_position,
        _,
        mut vital_stats,
        _,
    )) = player_query.get_mut(player_entity)
    else {
        return;
    };
    let player_position = *player_position;

    if let ItemReference::WorldObject(world_object_id) = source {
        let Some((_, world_tile)) = object_query
            .iter()
            .find(|(_, _, _, obj)| obj.object_id == world_object_id)
            .map(|(e, _, tile, _)| (e, *tile))
        else {
            return;
        };
        if !is_near_player(&player_position, &world_tile) {
            chat_log_state.push_narrator("That item is out of reach.");
            return;
        }
    }

    let view = item_reference_view(
        source,
        &inventory_state,
        container_query,
        object_query,
        object_registry,
    );
    let Some(view) = view else {
        return;
    };

    let Some(definition) = definitions.get(&view.type_id) else {
        return;
    };

    if let Some(spell_id) = ObjectRegistry::resolved_spell_id_for_type(
        &view.type_id,
        Some(&view.properties),
        definitions,
        spell_definitions,
    ) {
        let Some(spell) = spell_definitions.get(&spell_id) else {
            chat_log_state.push_narrator("That spell is unknown.");
            return;
        };
        if spell.targeting == crate::magic::resources::SpellTargeting::Targeted {
            return;
        }
        if vital_stats.mana < spell.mana_cost {
            chat_log_state.push_narrator(format!("Not enough mana to cast {}.", spell.name));
            return;
        }
        vital_stats.mana = (vital_stats.mana - spell.mana_cost).max(0.0);
        apply_spell_effects(spell, &mut vital_stats);
        consume_item_reference(
            source,
            &mut inventory_state,
            container_query,
            object_query,
            commands,
        );
        chat_log_state.push_line(format!("[Player]: \"{}\"", spell.incantation));
        chat_log_state.push_narrator(format!("Cast {}.", spell.name));
        return;
    }

    if !definition.is_usable() {
        return;
    }

    let source_name = ObjectRegistry::display_name_for_type(
        &view.type_id,
        Some(&view.properties),
        definitions,
        spell_definitions,
    )
    .unwrap_or_else(|| definition.name.clone());

    vital_stats.health = (vital_stats.health + definition.use_effects.restore_health)
        .clamp(0.0, vital_stats.max_health);
    vital_stats.mana =
        (vital_stats.mana + definition.use_effects.restore_mana).clamp(0.0, vital_stats.max_mana);
    consume_item_reference(
        source,
        &mut inventory_state,
        container_query,
        object_query,
        commands,
    );
    chat_log_state.push_narrator(use_text(definition, &source_name));
}

fn handle_use_item_on(
    player_entity: Entity,
    source: ItemReference,
    target: UseTarget,
    container_query: &mut Query<&mut Container>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    commands: &mut Commands,
) {
    match target {
        UseTarget::Player => handle_use_item(
            player_entity,
            source,
            container_query,
            object_query,
            player_query,
            object_registry,
            definitions,
            spell_definitions,
            commands,
        ),
        UseTarget::Object(target_object_id) => {
            let Ok((
                _,
                _,
                inventory_state,
                mut chat_log_state,
                player_space_resident,
                player_position,
                _,
                _,
                _,
            )) = player_query.get_mut(player_entity)
            else {
                return;
            };
            let Some(source_view) = item_reference_view(
                source,
                &inventory_state,
                container_query,
                object_query,
                object_registry,
            ) else {
                return;
            };
            if let ItemReference::WorldObject(world_source_id) = source {
                let Some((_, source_tile)) = object_query
                    .iter()
                    .find(|(_, _, _, obj)| obj.object_id == world_source_id)
                    .map(|(e, _, tile, _)| (e, *tile))
                else {
                    return;
                };
                if !is_near_player(&player_position, &source_tile) {
                    chat_log_state.push_narrator("That item is out of reach.");
                    return;
                }
            }
            let Some(source_definition) = definitions.get(&source_view.type_id) else {
                return;
            };
            let Some((_, target_position)) = find_object_entity(
                target_object_id,
                player_space_resident.space_id,
                object_query,
            ) else {
                return;
            };
            if !is_near_player(&player_position, &target_position) {
                chat_log_state.push_narrator("That target is out of reach.");
                return;
            }
            let source_name = ObjectRegistry::display_name_for_type(
                &source_view.type_id,
                Some(&source_view.properties),
                definitions,
                spell_definitions,
            )
            .unwrap_or_else(|| source_definition.name.clone());
            let target_name = object_registry
                .display_name(target_object_id, definitions, spell_definitions)
                .unwrap_or_else(|| target_object_id.to_string());
            chat_log_state.push_narrator(use_on_text(
                source_definition,
                &source_name,
                &target_name,
            ));
        }
    }
}

fn handle_cast_spell_at(
    player_entity: Entity,
    source: ItemReference,
    spell_id: &str,
    target_object_id: u64,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    npc_vitals_query: &mut Query<(&mut VitalStats, &OverworldObject), (With<Npc>, Without<Player>)>,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    commands: &mut Commands,
) {
    let Some(spell) = spell_definitions.get(spell_id) else {
        return;
    };
    let Ok((
        _,
        _,
        mut inventory_state,
        mut chat_log_state,
        player_space_resident,
        player_position,
        _,
        mut player_vitals,
        _,
    )) = player_query.get_mut(player_entity)
    else {
        return;
    };

    let Some((target_entity, target_position)) = find_object_entity(
        target_object_id,
        player_space_resident.space_id,
        object_query,
    ) else {
        return;
    };

    if chebyshev_distance_tiles(*player_position, target_position) > spell.range_tiles.max(1) {
        let target_name = object_registry
            .display_name(target_object_id, definitions, spell_definitions)
            .unwrap_or_else(|| target_object_id.to_string());
        chat_log_state.push_narrator(format!(
            "{} is out of range for {}.",
            target_name, spell.name
        ));
        return;
    }

    if player_vitals.mana < spell.mana_cost {
        chat_log_state.push_narrator(format!("Not enough mana to cast {}.", spell.name));
        return;
    }
    player_vitals.mana = (player_vitals.mana - spell.mana_cost).max(0.0);

    let Ok((mut target_vitals, target_object)) = npc_vitals_query.get_mut(target_entity) else {
        return;
    };
    let target_name = object_registry
        .display_name(target_object.object_id, definitions, spell_definitions)
        .unwrap_or_else(|| target_object.definition_id.clone());

    apply_spell_effects(spell, &mut target_vitals);
    consume_item_reference(
        source,
        &mut inventory_state,
        container_query,
        object_query,
        commands,
    );
    chat_log_state.push_line(format!("[Player]: \"{}\"", spell.incantation));
    chat_log_state.push_narrator(format!("Cast {} on {}.", spell.name, target_name));

    if target_vitals.health <= 0.0 {
        if let Some(loot_table) = definitions
            .get(&target_object.definition_id)
            .and_then(|def| def.loot_table.as_ref())
        {
            spawn_corpse_for_npc(
                commands,
                definitions,
                object_registry,
                loot_table,
                player_space_resident.space_id,
                target_position,
            );
        }
        commands.entity(target_entity).despawn();
        chat_log_state.push_line(format!("[{target_name} dies]"));
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_move_item(
    player_entity: Entity,
    source: ItemReference,
    destination: ItemDestination,
    container_query: &mut Query<&mut Container>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    collider_positions: &[TilePosition],
    movable_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    quantity_query: &Query<&Quantity>,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    commands: &mut Commands,
) {
    let Ok((
        _,
        _,
        mut inventory_state,
        mut chat_log_state,
        space_resident,
        player_position,
        _,
        _,
        _,
    )) = player_query.get_mut(player_entity)
    else {
        return;
    };

    match (source, destination) {
        (ItemReference::WorldObject(object_id), ItemDestination::Slot(slot_ref)) => {
            let Some((entity, tile_position, definition_id)) = find_movable_entity_with_definition(
                object_id,
                space_resident.space_id,
                movable_query,
            ) else {
                return;
            };
            if !is_near_player(&player_position, &tile_position) {
                chat_log_state.push_narrator("That item is out of reach.");
                return;
            }
            let quantity = quantity_query.get(entity).map(|q| q.0).unwrap_or(1);
            let properties = object_registry
                .properties(object_id)
                .cloned()
                .unwrap_or_default();
            let stack = InventoryStack {
                type_id: definition_id,
                properties,
                quantity,
            };
            if !place_stack_in_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                stack,
                slot_ref,
                definitions,
            ) {
                return;
            }
            commands.entity(entity).despawn();
        }
        (ItemReference::WorldObject(object_id), ItemDestination::WorldTile(target_tile)) => {
            let Some((entity, origin)) =
                find_movable_entity(object_id, space_resident.space_id, movable_query)
            else {
                return;
            };
            // Attempt stack merge first (before the "occupied by movable" check that blocks it).
            let merged = is_near_player(&player_position, &target_tile)
                && merge_into_ground_stack(
                    entity,
                    object_id,
                    target_tile,
                    space_resident.space_id,
                    object_query,
                    quantity_query,
                    object_registry,
                    definitions,
                    commands,
                );
            if !merged
                && is_valid_world_drop(
                    target_tile,
                    Some(origin),
                    space_resident.space_id,
                    &player_position,
                    entity,
                    collider_positions,
                    movable_query,
                    world_config,
                )
            {
                commands.entity(entity).insert(target_tile);
            }
        }
        (ItemReference::Slot(slot_ref), ItemDestination::Slot(destination_ref)) => {
            if slot_ref == destination_ref {
                return;
            }
            let Some(stack) = take_item_from_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                slot_ref,
            ) else {
                return;
            };
            let stack_for_restore = stack.clone();
            if !place_stack_in_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                stack,
                destination_ref,
                definitions,
            ) {
                restore_stack_to_slot_ref(
                    &mut inventory_state,
                    container_query,
                    object_query,
                    slot_ref,
                    stack_for_restore,
                );
            }
        }
        (ItemReference::Slot(slot_ref), ItemDestination::WorldTile(target_tile)) => {
            let Some(stack) = take_item_from_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                slot_ref,
            ) else {
                return;
            };

            let type_id = stack.type_id.clone();
            let stack_qty = stack.quantity;

            // Try merging into an existing same-type ground stack at the exact target
            // tile first, bypassing the "occupied by movable" rejection.
            if is_near_player(&player_position, &target_tile)
                && add_to_ground_stack(
                    &type_id,
                    stack_qty,
                    target_tile,
                    space_resident.space_id,
                    object_query,
                    quantity_query,
                    object_registry,
                    definitions,
                    commands,
                )
            {
                return;
            }

            // No merge: find a valid drop tile and spawn a new world object.
            let Some(world_drop_tile) = find_nearest_valid_world_drop_tile(
                target_tile,
                None,
                space_resident.space_id,
                &player_position,
                Entity::PLACEHOLDER,
                collider_positions,
                movable_query,
                world_config,
            ) else {
                restore_stack_to_slot_ref(
                    &mut inventory_state,
                    container_query,
                    object_query,
                    slot_ref,
                    stack,
                );
                return;
            };

            if !add_to_ground_stack(
                &type_id,
                stack_qty,
                world_drop_tile,
                space_resident.space_id,
                object_query,
                quantity_query,
                object_registry,
                definitions,
                commands,
            ) {
                let new_id = object_registry
                    .allocate_runtime_id_with_properties(type_id.clone(), stack.properties.clone());
                spawn_overworld_object(
                    commands,
                    definitions,
                    new_id,
                    &type_id,
                    None,
                    space_resident.space_id,
                    world_drop_tile,
                    Some(stack_qty),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_take_from_stack(
    player_entity: Entity,
    source: ItemReference,
    amount: u32,
    destination: ItemDestination,
    container_query: &mut Query<&mut Container>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    collider_positions: &[TilePosition],
    movable_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    quantity_query: &Query<&Quantity>,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    commands: &mut Commands,
) {
    let Ok((
        _,
        _,
        mut inventory_state,
        mut chat_log_state,
        space_resident,
        player_position,
        _,
        _,
        _,
    )) = player_query.get_mut(player_entity)
    else {
        return;
    };

    if amount == 0 {
        return;
    }

    match source {
        // ── source is an inventory/container slot ──────────────────────────────
        ItemReference::Slot(src_slot_ref) => {
            let Some(src_stack) = stack_in_slot_ref(
                &inventory_state,
                container_query,
                object_query,
                src_slot_ref,
            ) else {
                return;
            };
            if amount > src_stack.quantity {
                return;
            }
            let src_type_id = src_stack.type_id.clone();
            let src_properties = src_stack.properties.clone();
            let max_stack = definitions
                .get(&src_type_id)
                .map(|d| d.max_stack_size)
                .unwrap_or(1);

            match destination {
                ItemDestination::Slot(dst_slot_ref) => {
                    let dst_stack = stack_in_slot_ref(
                        &inventory_state,
                        container_query,
                        object_query,
                        dst_slot_ref,
                    );
                    match dst_stack {
                        None => {
                            let new_stack = InventoryStack {
                                type_id: src_type_id.clone(),
                                properties: src_properties.clone(),
                                quantity: amount,
                            };
                            if place_stack_in_slot_ref(
                                &mut inventory_state,
                                container_query,
                                object_query,
                                new_stack,
                                dst_slot_ref,
                                definitions,
                            ) {
                                reduce_slot_quantity(
                                    &mut inventory_state,
                                    container_query,
                                    object_query,
                                    src_slot_ref,
                                    amount,
                                );
                            }
                        }
                        Some(dst_existing) => {
                            if dst_existing.type_id != src_type_id {
                                chat_log_state.push_narrator("Can't mix different item types.");
                                return;
                            }
                            let dst_qty = dst_existing.quantity;
                            if dst_qty + amount > max_stack {
                                chat_log_state.push_narrator("Not enough space in that slot.");
                                return;
                            }
                            add_to_slot_quantity(
                                &mut inventory_state,
                                container_query,
                                object_query,
                                dst_slot_ref,
                                amount,
                            );
                            reduce_slot_quantity(
                                &mut inventory_state,
                                container_query,
                                object_query,
                                src_slot_ref,
                                amount,
                            );
                        }
                    }
                }
                ItemDestination::WorldTile(target_tile) => {
                    let Some(world_drop_tile) = find_nearest_valid_world_drop_tile(
                        target_tile,
                        None,
                        space_resident.space_id,
                        &player_position,
                        Entity::PLACEHOLDER,
                        collider_positions,
                        movable_query,
                        world_config,
                    ) else {
                        return;
                    };
                    let new_id = object_registry.allocate_runtime_id_with_properties(
                        src_type_id.clone(),
                        src_properties.clone(),
                    );
                    spawn_overworld_object(
                        commands,
                        definitions,
                        new_id,
                        &src_type_id,
                        None,
                        space_resident.space_id,
                        world_drop_tile,
                        Some(amount),
                    );
                    reduce_slot_quantity(
                        &mut inventory_state,
                        container_query,
                        object_query,
                        src_slot_ref,
                        amount,
                    );
                }
            }
        }

        // ── source is a world object (ground stack) ────────────────────────────
        ItemReference::WorldObject(object_id) => {
            let Some((entity, tile_position, definition_id)) = find_movable_entity_with_definition(
                object_id,
                space_resident.space_id,
                movable_query,
            ) else {
                return;
            };
            if !is_near_player(&player_position, &tile_position) {
                return;
            }
            let world_qty = quantity_query.get(entity).map(|q| q.0).unwrap_or(1);
            let actual_amount = amount.min(world_qty);
            let properties = object_registry
                .properties(object_id)
                .cloned()
                .unwrap_or_default();

            let new_stack = InventoryStack {
                type_id: definition_id,
                properties,
                quantity: actual_amount,
            };
            let destination_slot = match destination {
                ItemDestination::Slot(s) => s,
                ItemDestination::WorldTile(_) => return,
            };
            if !place_stack_in_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                new_stack,
                destination_slot,
                definitions,
            ) {
                return;
            }

            // Update or despawn the world entity
            if actual_amount >= world_qty {
                commands.entity(entity).despawn();
            } else {
                let remaining = world_qty - actual_amount;
                if remaining > 1 {
                    commands.entity(entity).insert(Quantity(remaining));
                } else {
                    commands.entity(entity).remove::<Quantity>();
                }
            }
        }
    }
}

/// Tries to merge the dragged world object into an existing same-type ground stack at
/// `target_tile`. Returns `true` if the dragged entity was fully or partially consumed
/// (caller should NOT then move it to target_tile).
fn merge_into_ground_stack(
    dragged_entity: Entity,
    dragged_object_id: u64,
    target_tile: TilePosition,
    space_id: crate::world::components::SpaceId,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    quantity_query: &Query<&Quantity>,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    commands: &mut Commands,
) -> bool {
    let Some(type_id) = object_registry
        .type_id(dragged_object_id)
        .map(str::to_owned)
    else {
        return false;
    };
    let Some(def) = definitions.get(&type_id) else {
        return false;
    };
    if def.max_stack_size <= 1 {
        return false;
    }
    let dragged_qty = quantity_query.get(dragged_entity).map(|q| q.0).unwrap_or(1);

    for (other_entity, other_resident, other_tile, other_object) in object_query.iter() {
        if other_entity == dragged_entity
            || other_resident.space_id != space_id
            || *other_tile != target_tile
        {
            continue;
        }
        let Some(other_type) = object_registry.type_id(other_object.object_id) else {
            continue;
        };
        if other_type != type_id {
            continue;
        }
        let other_qty = quantity_query.get(other_entity).map(|q| q.0).unwrap_or(1);
        if other_qty >= def.max_stack_size {
            continue;
        }
        let addable = (def.max_stack_size - other_qty).min(dragged_qty);
        let new_other_qty = other_qty + addable;
        if new_other_qty > 1 {
            commands
                .entity(other_entity)
                .insert(Quantity(new_other_qty));
        } else {
            commands.entity(other_entity).remove::<Quantity>();
        }
        if addable >= dragged_qty {
            commands.entity(dragged_entity).despawn();
        } else {
            // Partial merge — leave remainder on the dragged entity (stays at origin tile)
            let remaining = dragged_qty - addable;
            if remaining > 1 {
                commands.entity(dragged_entity).insert(Quantity(remaining));
            } else {
                commands.entity(dragged_entity).remove::<Quantity>();
            }
        }
        return true;
    }
    false
}

/// Tries to add `qty` of `type_id` items to an existing ground stack at `tile`. Returns
/// `true` if the quantity was fully absorbed (caller should NOT spawn a new world object).
fn add_to_ground_stack(
    type_id: &str,
    qty: u32,
    tile: TilePosition,
    space_id: crate::world::components::SpaceId,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    quantity_query: &Query<&Quantity>,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    commands: &mut Commands,
) -> bool {
    let Some(def) = definitions.get(type_id) else {
        return false;
    };
    if def.max_stack_size <= 1 {
        return false;
    }

    for (other_entity, other_resident, other_tile, other_object) in object_query.iter() {
        if other_resident.space_id != space_id || *other_tile != tile {
            continue;
        }
        let Some(other_type) = object_registry.type_id(other_object.object_id) else {
            continue;
        };
        if other_type != type_id {
            continue;
        }
        let other_qty = quantity_query.get(other_entity).map(|q| q.0).unwrap_or(1);
        if other_qty >= def.max_stack_size {
            continue;
        }
        let addable = (def.max_stack_size - other_qty).min(qty);
        let new_qty = other_qty + addable;
        if new_qty > 1 {
            commands.entity(other_entity).insert(Quantity(new_qty));
        } else {
            commands.entity(other_entity).remove::<Quantity>();
        }
        return addable >= qty;
    }
    false
}

fn handle_admin_spawn(
    player_entity: Entity,
    type_id: &str,
    tile_position: TilePosition,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    collider_positions: &[TilePosition],
    object_registry: &mut ObjectRegistry,
    commands: &mut Commands,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
) {
    let Ok((_, _, _, mut chat_log_state, space_resident, _, _, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };
    if tile_position.x < 0
        || tile_position.y < 0
        || tile_position.x >= world_config.map_width
        || tile_position.y >= world_config.map_height
    {
        chat_log_state.push_narrator("Spawn rejected: target tile is outside the map.");
        return;
    }

    let Some(definition) = definitions.get(type_id) else {
        chat_log_state.push_narrator(format!("Spawn rejected: unknown object type {type_id}."));
        return;
    };

    if definition.colliding
        && collider_positions
            .iter()
            .any(|collider_position| *collider_position == tile_position)
    {
        chat_log_state.push_narrator("Spawn rejected: target tile is blocked.");
        return;
    }

    let object_id = object_registry.allocate_runtime_id(type_id.to_owned());
    spawn_overworld_object(
        commands,
        definitions,
        object_id,
        type_id,
        None,
        space_resident.space_id,
        tile_position,
        None,
    );
    chat_log_state.push_narrator(format!(
        "Spawned {} as id {} at ({}, {}).",
        type_id, object_id, tile_position.x, tile_position.y
    ));
}

fn resolve_player_entity(
    player_id: Option<crate::player::components::PlayerId>,
    player_lookup_query: &Query<
        (
            Entity,
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
        ),
        With<Player>,
    >,
) -> Option<Entity> {
    match player_id {
        Some(player_id) => player_lookup_query
            .iter()
            .find_map(|(entity, identity, ..)| (identity.id == player_id).then_some(entity)),
        None => player_lookup_query.iter().next().map(|(entity, ..)| entity),
    }
}

fn find_object_entity(
    object_id: u64,
    space_id: crate::world::components::SpaceId,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
) -> Option<(Entity, TilePosition)> {
    object_query
        .iter()
        .find_map(|(entity, resident, tile_position, object)| {
            (resident.space_id == space_id && object.object_id == object_id)
                .then_some((entity, *tile_position))
        })
}

fn find_combat_target_entity(
    object_id: u64,
    source_space_id: crate::world::components::SpaceId,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    player_lookup_query: &Query<
        (
            Entity,
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
        ),
        With<Player>,
    >,
) -> Option<Entity> {
    find_object_entity(object_id, source_space_id, object_query)
        .map(|(entity, _)| entity)
        .or_else(|| {
            player_lookup_query
                .iter()
                .find_map(|(entity, _, resident, _, object)| {
                    (resident.space_id == source_space_id && object.object_id == object_id)
                        .then_some(entity)
                })
        })
}

fn find_movable_entity(
    object_id: u64,
    space_id: crate::world::components::SpaceId,
    movable_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
) -> Option<(Entity, TilePosition)> {
    movable_query
        .iter()
        .find_map(|(entity, resident, tile_position, object)| {
            (resident.space_id == space_id && object.object_id == object_id)
                .then_some((entity, *tile_position))
        })
}

fn find_movable_entity_with_definition(
    object_id: u64,
    space_id: crate::world::components::SpaceId,
    movable_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
) -> Option<(Entity, TilePosition, String)> {
    movable_query
        .iter()
        .find_map(|(entity, resident, tile_position, object)| {
            (resident.space_id == space_id && object.object_id == object_id)
                .then(|| (entity, *tile_position, object.definition_id.clone()))
        })
}

/// Resolve any kind of item reference (world object id or slot ref) to a
/// non-runtime view: `(type_id, properties, quantity)`. Inventory and container
/// slots already carry that info; world objects look it up from the registry.
fn item_reference_view(
    item_reference: ItemReference,
    inventory_state: &InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    object_registry: &ObjectRegistry,
) -> Option<ItemView> {
    match item_reference {
        ItemReference::WorldObject(object_id) => {
            view_for_world_object(object_id, object_query, object_registry)
        }
        ItemReference::Slot(slot_ref) => {
            stack_in_slot_ref(inventory_state, container_query, object_query, slot_ref).map(
                |stack| ItemView {
                    type_id: stack.type_id,
                    properties: stack.properties,
                    quantity: stack.quantity,
                },
            )
        }
    }
}

#[derive(Clone, Debug)]
struct ItemView {
    type_id: String,
    properties: ObjectProperties,
    #[allow(dead_code)]
    quantity: u32,
}

fn view_for_world_object(
    object_id: u64,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    object_registry: &ObjectRegistry,
) -> Option<ItemView> {
    let definition_id = object_query.iter().find_map(|(_, _, _, object)| {
        (object.object_id == object_id).then(|| object.definition_id.clone())
    })?;
    let properties = object_registry
        .properties(object_id)
        .cloned()
        .unwrap_or_default();
    Some(ItemView {
        type_id: definition_id,
        properties,
        quantity: 1,
    })
}

fn stack_in_slot_ref(
    inventory_state: &InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    slot_ref: ItemSlotRef,
) -> Option<InventoryStack> {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => inventory_state
            .backpack_slots
            .get(slot_index)
            .cloned()
            .flatten(),
        ItemSlotRef::Equipment(slot) => inventory_state.equipment_item(slot).map(|item| {
            let quantity = if slot == EquipmentSlot::Ammo {
                inventory_state.ammo_quantity.max(1)
            } else {
                1
            };
            InventoryStack {
                type_id: item.type_id.clone(),
                properties: item.properties.clone(),
                quantity,
            }
        }),
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            let entity = find_container_entity(object_id, object_query)?;
            let container = container_query.get_mut(entity).ok()?;
            container.slots.get(slot_index).cloned().flatten()
        }
    }
}

fn take_item_from_slot_ref(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    slot_ref: ItemSlotRef,
) -> Option<InventoryStack> {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            inventory_state.backpack_slots.get_mut(slot_index)?.take()
        }
        ItemSlotRef::Equipment(slot) => {
            let quantity = if slot == EquipmentSlot::Ammo {
                let q = inventory_state.ammo_quantity.max(1);
                inventory_state.ammo_quantity = 0;
                q
            } else {
                1
            };
            inventory_state
                .take_equipment_item(slot)
                .map(|item| InventoryStack {
                    type_id: item.type_id,
                    properties: item.properties,
                    quantity,
                })
        }
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            let entity = find_container_entity(object_id, object_query)?;
            let mut container = container_query.get_mut(entity).ok()?;
            container.slots.get_mut(slot_index)?.take()
        }
    }
}

fn place_stack_in_option_slot(
    slot: &mut Option<InventoryStack>,
    stack: InventoryStack,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    match slot {
        None => {
            *slot = Some(stack);
            true
        }
        Some(existing) => {
            if stack.type_id != existing.type_id {
                return false;
            }
            let max_stack = definitions
                .get(&stack.type_id)
                .map(|d| d.max_stack_size)
                .unwrap_or(1);
            if existing.quantity >= max_stack {
                return false;
            }
            let addable = max_stack
                .saturating_sub(existing.quantity)
                .min(stack.quantity);
            if addable < stack.quantity {
                return false;
            }
            existing.quantity += addable;
            true
        }
    }
}

fn place_stack_in_slot_ref(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    stack: InventoryStack,
    slot_ref: ItemSlotRef,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    if !type_is_storable(&stack.type_id, definitions) {
        return false;
    }

    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) else {
                return false;
            };
            place_stack_in_option_slot(slot, stack, definitions)
        }
        ItemSlotRef::Equipment(slot) => {
            place_item_in_equipment_slot(inventory_state, definitions, slot, stack)
        }
        ItemSlotRef::Container {
            object_id: container_object_id,
            slot_index,
        } => {
            let Some(entity) = find_container_entity(container_object_id, object_query) else {
                return false;
            };
            let Ok(mut container) = container_query.get_mut(entity) else {
                return false;
            };
            let Some(slot) = container.slots.get_mut(slot_index) else {
                return false;
            };
            place_stack_in_option_slot(slot, stack, definitions)
        }
    }
}

fn restore_stack_to_slot_ref(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    slot_ref: ItemSlotRef,
    stack: InventoryStack,
) {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            if let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) {
                *slot = Some(stack);
            }
        }
        ItemSlotRef::Equipment(slot) => {
            inventory_state.restore_equipment_item(
                slot,
                EquippedItem {
                    type_id: stack.type_id,
                    properties: stack.properties,
                },
            );
            if slot == EquipmentSlot::Ammo {
                inventory_state.ammo_quantity = stack.quantity.max(1);
            }
        }
        ItemSlotRef::Container {
            object_id: container_object_id,
            slot_index,
        } => {
            if let Some(entity) = find_container_entity(container_object_id, object_query) {
                if let Ok(mut container) = container_query.get_mut(entity) {
                    if let Some(slot) = container.slots.get_mut(slot_index) {
                        *slot = Some(stack);
                    }
                }
            }
        }
    }
}

fn consume_one_from_slot_ref(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    slot_ref: ItemSlotRef,
) {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            if let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) {
                if let Some(stack) = slot {
                    if stack.quantity > 1 {
                        stack.quantity -= 1;
                    } else {
                        *slot = None;
                    }
                }
            }
        }
        ItemSlotRef::Equipment(slot) => {
            if slot == EquipmentSlot::Ammo && inventory_state.ammo_quantity > 1 {
                inventory_state.ammo_quantity -= 1;
            } else {
                inventory_state.take_equipment_item(slot);
                if slot == EquipmentSlot::Ammo {
                    inventory_state.ammo_quantity = 0;
                }
            }
        }
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            if let Some(entity) = find_container_entity(object_id, object_query) {
                if let Ok(mut container) = container_query.get_mut(entity) {
                    if let Some(slot) = container.slots.get_mut(slot_index) {
                        if let Some(stack) = slot {
                            if stack.quantity > 1 {
                                stack.quantity -= 1;
                            } else {
                                *slot = None;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn reduce_slot_quantity(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    slot_ref: ItemSlotRef,
    amount: u32,
) {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            if let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) {
                if let Some(stack) = slot {
                    if stack.quantity <= amount {
                        *slot = None;
                    } else {
                        stack.quantity -= amount;
                    }
                }
            }
        }
        ItemSlotRef::Equipment(_) => {}
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            if let Some(entity) = find_container_entity(object_id, object_query) {
                if let Ok(mut container) = container_query.get_mut(entity) {
                    if let Some(slot) = container.slots.get_mut(slot_index) {
                        if let Some(stack) = slot {
                            if stack.quantity <= amount {
                                *slot = None;
                            } else {
                                stack.quantity -= amount;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn add_to_slot_quantity(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    slot_ref: ItemSlotRef,
    amount: u32,
) {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            if let Some(Some(stack)) = inventory_state.backpack_slots.get_mut(slot_index) {
                stack.quantity += amount;
            }
        }
        ItemSlotRef::Equipment(_) => {}
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            if let Some(entity) = find_container_entity(object_id, object_query) {
                if let Ok(mut container) = container_query.get_mut(entity) {
                    if let Some(Some(stack)) = container.slots.get_mut(slot_index) {
                        stack.quantity += amount;
                    }
                }
            }
        }
    }
}

fn consume_item_reference(
    item_reference: ItemReference,
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    commands: &mut Commands,
) {
    match item_reference {
        ItemReference::WorldObject(object_id) => {
            if let Some((entity, _, _, _)) = object_query
                .iter()
                .find(|(_, _, _, object)| object.object_id == object_id)
            {
                commands.entity(entity).despawn();
            }
        }
        ItemReference::Slot(slot_ref) => {
            consume_one_from_slot_ref(inventory_state, container_query, object_query, slot_ref);
        }
    }
}

fn find_container_entity(
    object_id: u64,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
) -> Option<Entity> {
    object_query
        .iter()
        .find_map(|(entity, _, _, object)| (object.object_id == object_id).then_some(entity))
}

fn place_item_in_equipment_slot(
    inventory_state: &mut InventoryState,
    definitions: &OverworldObjectDefinitions,
    slot: EquipmentSlot,
    stack: InventoryStack,
) -> bool {
    let Some(definition) = definitions.get(&stack.type_id) else {
        return false;
    };
    if definition.equipment_slot != Some(slot) {
        return false;
    }

    let quantity = stack.quantity;
    let placed = inventory_state.place_equipment_item(
        slot,
        EquippedItem {
            type_id: stack.type_id,
            properties: stack.properties,
        },
    );
    if placed && slot == EquipmentSlot::Ammo {
        inventory_state.ammo_quantity = quantity.max(1);
    }
    placed
}

fn object_description_for_type(
    type_id: &str,
    properties: &ObjectProperties,
    count: u32,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
) -> Option<String> {
    let definition = definitions.get(type_id)?;
    let display_name = ObjectRegistry::display_name_for_type(
        type_id,
        Some(properties),
        definitions,
        spell_definitions,
    )
    .unwrap_or_else(|| definition.name.clone());
    let description_text = ObjectRegistry::description_with_count_for_type(
        type_id,
        Some(properties),
        count,
        definitions,
        spell_definitions,
    )
    .unwrap_or_else(|| definition.description_for_count(count).to_owned());
    let description = description_text.trim();
    if description.is_empty() {
        Some(format!("Just a {}.", display_name.to_lowercase()))
    } else {
        Some(description.to_owned())
    }
}

fn type_is_storable(type_id: &str, definitions: &OverworldObjectDefinitions) -> bool {
    definitions.get(type_id).is_some_and(|d| d.storable)
}

fn use_text(definition: &OverworldObjectDefinition, item_name: &str) -> String {
    if definition.use_texts.is_empty() {
        return format!("{item_name} used.");
    }

    random_text(&definition.use_texts).replace("{item}", item_name)
}

fn use_on_text(
    definition: &OverworldObjectDefinition,
    item_name: &str,
    target_name: &str,
) -> String {
    if definition.use_on_texts.is_empty() {
        return format!("Used {} on {}.", item_name, target_name);
    }

    random_text(&definition.use_on_texts)
        .replace("{target}", target_name)
        .replace("{item}", item_name)
}

fn random_text(texts: &[String]) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    texts[nanos % texts.len()].clone()
}

fn apply_spell_effects(spell: &SpellDefinition, vital_stats: &mut VitalStats) {
    vital_stats.health =
        (vital_stats.health - spell.effects.damage).clamp(0.0, vital_stats.max_health);
    vital_stats.health =
        (vital_stats.health + spell.effects.restore_health).clamp(0.0, vital_stats.max_health);
    vital_stats.mana =
        (vital_stats.mana + spell.effects.restore_mana).clamp(0.0, vital_stats.max_mana);
}

fn chebyshev_distance_tiles(a: TilePosition, b: TilePosition) -> i32 {
    if a.z != b.z {
        return i32::MAX;
    }
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

/// Central walkability check for player moves (and, later, teleport targets).
///
/// Ground floor is walkable anywhere a collider isn't present. Upper floors
/// (`z != 0`) require at least one object at the target tile whose definition
/// has `walkable_surface: true` — floor planks and stairs opt in. This makes
/// upper floors "built from positive-space tiles" rather than infinite planes,
/// so players can't walk past the edge of an authored building.
fn is_walkable_tile(
    target: TilePosition,
    space_id: crate::world::components::SpaceId,
    collider_positions: &[TilePosition],
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    if collider_positions
        .iter()
        .any(|collider_position| *collider_position == target)
    {
        return false;
    }

    if target.z == TilePosition::GROUND_FLOOR {
        return true;
    }

    object_query
        .iter()
        .filter(|(_, resident, tile, _)| resident.space_id == space_id && **tile == target)
        .any(|(_, _, _, object)| {
            definitions
                .get(&object.definition_id)
                .is_some_and(|def| def.render.walkable_surface)
        })
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::combat::components::{AttackProfile, CombatLeash};
    use crate::game::commands::{
        GameCommand, ItemDestination, ItemReference, ItemSlotRef, MoveDelta,
    };
    use crate::game::resources::ClientGameState;
    use crate::game::GameServerPlugin;
    use crate::magic::MagicPlugin;
    use crate::player::components::{
        BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, Player, PlayerId,
        PlayerIdentity, VitalStats, WeaponDamage,
    };
    use crate::player::PlayerServerPlugin;
    use crate::world::components::{Collider, OverworldObject};
    use crate::world::object_registry::ObjectRegistry;
    use crate::world::WorldServerPlugin;

    fn setup_server_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins((
            GameServerPlugin,
            WorldServerPlugin,
            PlayerServerPlugin,
            MagicPlugin,
        ));
        app.update();
        app
    }

    fn spawn_player(app: &mut App, player_id: u64, x: i32, y: i32) -> Entity {
        let base_stats = BaseStats::default();
        let derived_stats = DerivedStats::from_base(&base_stats);
        let max_health = derived_stats.max_health as f32;
        let max_mana = derived_stats.max_mana as f32;
        let current_space_id = app.world().resource::<WorldConfig>().current_space_id;
        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("player");
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity {
                    id: PlayerId(player_id),
                },
                Inventory::default(),
                ChatLog::default(),
                base_stats,
                derived_stats,
                VitalStats::full(max_health, max_mana),
                MovementCooldown::default(),
                (AttackProfile::melee(), WeaponDamage::default()),
                CombatLeash {
                    max_distance_tiles: 6,
                },
                Collider,
                OverworldObject {
                    object_id,
                    definition_id: "player".to_owned(),
                },
                SpaceResident {
                    space_id: current_space_id,
                },
                TilePosition::ground(x, y),
            ))
            .id()
    }

    fn spawn_world_object(
        app: &mut App,
        type_id: &str,
        object_id: u64,
        tile: TilePosition,
    ) -> Entity {
        use crate::apply_overworld_definition_components;

        let current_space_id = app.world().resource::<WorldConfig>().current_space_id;
        let definition = app
            .world()
            .resource::<crate::world::object_definitions::OverworldObjectDefinitions>()
            .get(type_id)
            .unwrap()
            .clone();
        let mut entity = app.world_mut().spawn((
            OverworldObject {
                object_id,
                definition_id: type_id.to_owned(),
            },
            SpaceResident {
                space_id: current_space_id,
            },
            tile,
        ));
        apply_overworld_definition_components!(entity, &definition, None, None);

        entity.id()
    }

    #[test]
    fn loaded_player_space_drives_same_frame_world_projection() {
        let mut app = setup_server_app();
        let alternate_space_id = app
            .world()
            .resource::<crate::world::resources::SpaceManager>()
            .spaces
            .keys()
            .copied()
            .find(|space_id| *space_id != app.world().resource::<WorldConfig>().current_space_id)
            .expect("expected a second persistent space for projection test");

        let player = spawn_player(&mut app, 1, 4, 4);
        app.world_mut().entity_mut(player).insert(SpaceResident {
            space_id: alternate_space_id,
        });

        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("apple");
        let world_object =
            spawn_world_object(&mut app, "apple", object_id, TilePosition::ground(5, 4));
        app.world_mut()
            .entity_mut(world_object)
            .insert(SpaceResident {
                space_id: alternate_space_id,
            });

        app.update();

        let client_state = app.world().resource::<ClientGameState>();
        assert_eq!(
            client_state
                .player_position
                .map(|position| position.space_id),
            Some(alternate_space_id)
        );
        assert_eq!(
            client_state
                .current_space
                .as_ref()
                .map(|space| space.space_id),
            Some(alternate_space_id)
        );
        assert!(client_state.world_objects.contains_key(&object_id));
    }

    #[test]
    fn routes_move_command_to_only_the_owning_player() {
        let mut app = setup_server_app();
        let player_one = spawn_player(&mut app, 1, 10, 10);
        let player_two = spawn_player(&mut app, 2, 12, 10);

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::MovePlayer {
                    delta: MoveDelta { x: 1, y: 0 },
                },
            );

        app.update();

        let mut player_query = app.world_mut().query::<(&PlayerIdentity, &TilePosition)>();
        let positions = player_query
            .iter(app.world())
            .map(|(identity, position)| (identity.id.0, *position))
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(positions[&1], TilePosition::ground(11, 10));
        assert_eq!(positions[&2], TilePosition::ground(12, 10));
        assert!(app.world().get_entity(player_one).is_ok());
        assert!(app.world().get_entity(player_two).is_ok());
    }

    #[test]
    fn inspect_respects_perception_based_range() {
        // Default BaseStats gives focus = 10 -> focus/5 = 2 bonus tiles.
        // An apple has no inspect_range set, so default_inspect_range = 3 applies.
        // Effective range = 3 + 2 = 5 tiles (Chebyshev).

        // Case A: player one tile beyond effective range -> "too far" message.
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 0, 0);
        let apple_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("apple");
        // Place the apple at Chebyshev distance 6 (just past effective range 5).
        spawn_world_object(&mut app, "apple", apple_id, TilePosition::ground(6, 0));

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::Inspect {
                    target: InspectTarget::Object(apple_id),
                },
            );
        app.update();

        let chat_log = app.world().get::<ChatLog>(player).unwrap();
        assert!(
            chat_log
                .lines
                .last()
                .map(|line| line.contains("too far"))
                .unwrap_or(false),
            "expected 'too far' message; got {:?}",
            chat_log.lines
        );

        // Case B: within effective range -> real description replaces the log.
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 0, 0);
        let apple_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("apple");
        // Chebyshev distance 5 == effective range, should succeed.
        spawn_world_object(&mut app, "apple", apple_id, TilePosition::ground(5, 0));

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::Inspect {
                    target: InspectTarget::Object(apple_id),
                },
            );
        app.update();

        let chat_log = app.world().get::<ChatLog>(player).unwrap();
        let last = chat_log.lines.last().cloned().unwrap_or_default();
        assert!(
            !last.contains("too far"),
            "expected a real description; got {:?}",
            chat_log.lines
        );
        assert!(
            last.starts_with("[Narrator]:") && last.to_lowercase().contains("apple"),
            "expected narrator line mentioning apple; got {:?}",
            last
        );
    }

    #[test]
    fn players_block_other_players_movement() {
        let mut app = setup_server_app();
        spawn_player(&mut app, 1, 10, 10);
        spawn_player(&mut app, 2, 11, 10);

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::MovePlayer {
                    delta: MoveDelta { x: 1, y: 0 },
                },
            );

        app.update();

        let mut player_query = app.world_mut().query::<(&PlayerIdentity, &TilePosition)>();
        let positions = player_query
            .iter(app.world())
            .map(|(identity, position)| (identity.id.0, *position))
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(positions[&1], TilePosition::ground(10, 10));
        assert_eq!(positions[&2], TilePosition::ground(11, 10));
    }

    #[test]
    fn move_item_changes_only_the_acting_players_inventory() {
        let mut app = setup_server_app();
        spawn_player(&mut app, 1, 10, 10);
        spawn_player(&mut app, 2, 14, 10);

        let apple_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("apple");
        spawn_world_object(&mut app, "apple", apple_id, TilePosition::ground(11, 10));

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::MoveItem {
                    source: ItemReference::WorldObject(apple_id),
                    destination: ItemDestination::Slot(ItemSlotRef::Backpack(0)),
                },
            );

        app.update();

        let mut inventory_query = app.world_mut().query::<(&PlayerIdentity, &Inventory)>();
        let inventories = inventory_query
            .iter(app.world())
            .map(|(identity, inventory)| (identity.id.0, inventory.backpack_slots.clone()))
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(
            inventories[&1][0],
            Some(InventoryStack {
                type_id: "apple".to_owned(),
                properties: ObjectProperties::new(),
                quantity: 1,
            })
        );
        assert_eq!(inventories[&2][0], None);
        let _ = apple_id;

        let mut object_query = app
            .world_mut()
            .query::<&crate::world::components::OverworldObject>();
        assert!(!object_query
            .iter(app.world())
            .any(|object| object.object_id == apple_id));
    }

    #[test]
    fn stairs_transition_teleports_player_up_one_floor() {
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 10, 10);

        let stairs_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("stairs_up");
        spawn_world_object(
            &mut app,
            "stairs_up",
            stairs_id,
            TilePosition::ground(11, 10),
        );
        // Upper-floor walkability rule: the destination of the stair transition
        // needs a walkable-surface object underneath.
        let plank_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("floor_plank");
        spawn_world_object(
            &mut app,
            "floor_plank",
            plank_id,
            TilePosition::new(11, 10, 1),
        );

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::MovePlayer {
                    delta: MoveDelta { x: 1, y: 0 },
                },
            );
        app.update();

        let tile = *app.world().get::<TilePosition>(player).unwrap();
        assert_eq!(tile, TilePosition::new(11, 10, 1));
    }

    #[test]
    fn upper_floor_walk_requires_walkable_surface() {
        let mut app = setup_server_app();
        // Player already on floor 1 standing on a plank; no plank to the east.
        let player = spawn_player(&mut app, 1, 10, 10);
        app.world_mut()
            .entity_mut(player)
            .insert(TilePosition::new(10, 10, 1));

        let plank_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("floor_plank");
        spawn_world_object(
            &mut app,
            "floor_plank",
            plank_id,
            TilePosition::new(10, 10, 1),
        );

        // Attempt to walk east into "empty air" on floor 1.
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::MovePlayer {
                    delta: MoveDelta { x: 1, y: 0 },
                },
            );
        app.update();

        let tile = *app.world().get::<TilePosition>(player).unwrap();
        assert_eq!(
            tile,
            TilePosition::new(10, 10, 1),
            "player should not walk off the edge of the plank"
        );

        // Add a plank to the east and retry — now the move should succeed.
        let plank_east_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("floor_plank");
        spawn_world_object(
            &mut app,
            "floor_plank",
            plank_east_id,
            TilePosition::new(11, 10, 1),
        );

        // Clear the movement cooldown so the next step fires immediately.
        app.world_mut()
            .entity_mut(player)
            .insert(MovementCooldown::default());
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::MovePlayer {
                    delta: MoveDelta { x: 1, y: 0 },
                },
            );
        app.update();

        let tile = *app.world().get::<TilePosition>(player).unwrap();
        assert_eq!(tile, TilePosition::new(11, 10, 1));
    }

    #[test]
    fn stairs_blocked_destination_prevents_transition() {
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 10, 10);

        let stairs_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("stairs_up");
        spawn_world_object(
            &mut app,
            "stairs_up",
            stairs_id,
            TilePosition::ground(11, 10),
        );

        // Wall at the destination floor blocks the transition.
        let wall_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("wall");
        spawn_world_object(&mut app, "wall", wall_id, TilePosition::new(11, 10, 1));

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                crate::player::components::PlayerId(1),
                GameCommand::MovePlayer {
                    delta: MoveDelta { x: 1, y: 0 },
                },
            );
        app.update();

        let tile = *app.world().get::<TilePosition>(player).unwrap();
        assert_eq!(tile, TilePosition::ground(11, 10));
        let chat_log = app.world().get::<ChatLog>(player).unwrap();
        assert!(
            chat_log
                .lines
                .last()
                .map(|line| line.contains("blocked"))
                .unwrap_or(false),
            "expected 'blocked' narrator line; got {:?}",
            chat_log.lines
        );
    }
}

fn is_valid_world_drop(
    target_tile: TilePosition,
    origin_tile: Option<TilePosition>,
    space_id: crate::world::components::SpaceId,
    player_position: &TilePosition,
    dragged_entity: Entity,
    collider_positions: &[TilePosition],
    movable_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    world_config: &WorldConfig,
) -> bool {
    if target_tile.x < 0
        || target_tile.y < 0
        || target_tile.x >= world_config.map_width
        || target_tile.y >= world_config.map_height
    {
        return false;
    }

    if !is_near_player(player_position, &target_tile) {
        return false;
    }

    if origin_tile == Some(target_tile) {
        return true;
    }

    if collider_positions
        .iter()
        .any(|collider_position| *collider_position == target_tile)
    {
        return false;
    }

    !movable_query
        .iter()
        .any(|(entity, resident, tile_position, _)| {
            resident.space_id == space_id
                && entity != dragged_entity
                && *tile_position == target_tile
        })
}

fn find_nearest_valid_world_drop_tile(
    target_tile: TilePosition,
    origin_tile: Option<TilePosition>,
    space_id: crate::world::components::SpaceId,
    player_position: &TilePosition,
    dragged_entity: Entity,
    collider_positions: &[TilePosition],
    movable_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    world_config: &WorldConfig,
) -> Option<TilePosition> {
    let mut candidates = Vec::new();
    for y in -1..=1 {
        for x in -1..=1 {
            candidates.push(TilePosition::new(
                target_tile.x + x,
                target_tile.y + y,
                target_tile.z,
            ));
        }
    }

    candidates.sort_by_key(|candidate| {
        (
            (candidate.x - target_tile.x).abs() + (candidate.y - target_tile.y).abs(),
            i32::from(candidate.x != target_tile.x && candidate.y != target_tile.y),
        )
    });

    candidates.into_iter().find(|candidate| {
        is_valid_world_drop(
            *candidate,
            origin_tile,
            space_id,
            player_position,
            dragged_entity,
            collider_positions,
            movable_query,
            world_config,
        )
    })
}
