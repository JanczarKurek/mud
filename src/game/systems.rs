use bevy::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::combat::components::CombatTarget;
use crate::game::commands::{
    GameCommand, InspectTarget, ItemDestination, ItemReference, ItemSlotRef, MoveDelta, UseTarget,
};
use crate::game::resources::{
    ChatLogState, ClientGameState, ClientVitalStats, ClientWorldObjectState, GameEvent,
    GameUiEvent, InventoryState, PendingGameCommands, PendingGameEvents, PendingGameUiEvents,
};
use crate::magic::resources::{SpellDefinition, SpellDefinitions};
use crate::npc::components::Npc;
use crate::player::components::{
    DerivedStats, MovementCooldown, Player, PlayerIdentity, VitalStats,
};
use crate::world::components::{Collider, Container, Movable, OverworldObject, TilePosition};
use crate::world::object_definitions::{
    EquipmentSlot, OverworldObjectDefinition, OverworldObjectDefinitions,
};
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::spawn_overworld_object;
use crate::world::WorldConfig;

pub fn tick_player_movement_cooldowns(
    time: Res<Time>,
    mut player_query: Query<&mut MovementCooldown, With<Player>>,
) {
    for mut movement_cooldown in &mut player_query {
        movement_cooldown.remaining_seconds =
            (movement_cooldown.remaining_seconds - time.delta_secs()).max(0.0);
    }
}

pub fn process_game_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    definitions: Res<OverworldObjectDefinitions>,
    spell_definitions: Res<SpellDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    world_config: Res<WorldConfig>,
    collider_query: Query<&TilePosition, (With<Collider>, Without<Player>)>,
    object_query: Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    movable_query: Query<
        (Entity, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    mut container_query: Query<&mut Container>,
    player_lookup_query: Query<(Entity, &PlayerIdentity), With<Player>>,
    mut player_query: Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    mut npc_vitals_query: Query<(&mut VitalStats, &OverworldObject), (With<Npc>, Without<Player>)>,
    mut commands: Commands,
) {
    let queued_commands = std::mem::take(&mut pending_commands.commands);

    for queued_command in queued_commands {
        let Some(player_entity) =
            resolve_player_entity(queued_command.player_id, &player_lookup_query)
        else {
            continue;
        };

        match queued_command.command {
            GameCommand::MovePlayer { delta } => {
                handle_move_player(
                    player_entity,
                    delta,
                    &world_config,
                    &collider_query,
                    &mut player_query,
                );
            }
            GameCommand::SetCombatTarget { target_object_id } => {
                handle_set_combat_target(
                    player_entity,
                    target_object_id,
                    &object_query,
                    &mut player_query,
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
                    &mut player_query,
                    &mut ui_events,
                );
            }
            GameCommand::Inspect { target } => {
                handle_inspect(
                    player_entity,
                    target,
                    &mut player_query,
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
                    &mut player_query,
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
                    &mut player_query,
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
                    &mut player_query,
                    &mut npc_vitals_query,
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut commands,
                );
            }
            GameCommand::MoveItem {
                source,
                destination,
            } => {
                handle_move_item(
                    player_entity,
                    source,
                    destination,
                    &mut container_query,
                    &mut player_query,
                    &collider_query,
                    &movable_query,
                    &object_query,
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
                handle_admin_spawn(
                    player_entity,
                    &type_id,
                    tile_position,
                    &definitions,
                    &world_config,
                    &collider_query,
                    &mut object_registry,
                    &mut commands,
                    &mut player_query,
                );
            }
        }
    }
}

pub fn collect_game_events_from_authority(
    mut client_state: ResMut<ClientGameState>,
    player_query: Query<
        (
            &PlayerIdentity,
            &InventoryState,
            &ChatLogState,
            &TilePosition,
            &VitalStats,
            &DerivedStats,
            &OverworldObject,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    object_query: Query<&OverworldObject>,
    world_object_query: Query<
        (
            &TilePosition,
            &OverworldObject,
            Has<Container>,
            Has<Npc>,
            Has<Movable>,
        ),
        Without<Player>,
    >,
    container_query: Query<(&Container, &OverworldObject), Without<Player>>,
    mut pending_game_events: ResMut<PendingGameEvents>,
) {
    pending_game_events.events.clear();

    if let Ok((
        player_identity,
        inventory_state,
        chat_log_state,
        player_tile_position,
        vital_stats,
        derived_stats,
        player_object,
        combat_target,
    )) = player_query.single()
    {
        if client_state.inventory != *inventory_state {
            pending_game_events
                .events
                .push(GameEvent::InventoryChanged {
                    inventory: inventory_state.clone(),
                });
        }

        if client_state.chat_log_lines != chat_log_state.lines {
            pending_game_events.events.push(GameEvent::ChatLogChanged {
                lines: chat_log_state.lines.clone(),
            });
        }

        if client_state.player_tile_position != Some(*player_tile_position) {
            pending_game_events
                .events
                .push(GameEvent::PlayerPositionChanged {
                    tile_position: *player_tile_position,
                });
        }

        let current_vitals = ClientVitalStats {
            health: vital_stats.health,
            max_health: vital_stats.max_health,
            mana: vital_stats.mana,
            max_mana: vital_stats.max_mana,
        };
        if client_state.player_vitals != Some(current_vitals) {
            pending_game_events
                .events
                .push(GameEvent::PlayerVitalsChanged {
                    vitals: current_vitals,
                });
        }

        if client_state.player_storage_slots != derived_stats.storage_slots {
            pending_game_events
                .events
                .push(GameEvent::PlayerStorageChanged {
                    storage_slots: derived_stats.storage_slots,
                });
        }

        let current_target_object_id = combat_target
            .and_then(|combat_target| object_query.get(combat_target.entity).ok())
            .map(|object| object.object_id);
        if client_state.current_target_object_id != current_target_object_id {
            pending_game_events
                .events
                .push(GameEvent::CombatTargetChanged {
                    target_object_id: current_target_object_id,
                });
        }

        if client_state.local_player_id != Some(player_identity.id) {
            client_state.local_player_id = Some(player_identity.id);
        }
        if client_state.local_player_object_id != Some(player_object.object_id) {
            client_state.local_player_object_id = Some(player_object.object_id);
        }
    }

    let mut current_container_ids = Vec::new();
    for (container, object) in &container_query {
        current_container_ids.push(object.object_id);
        let current_slots = container.slots.clone();
        if client_state.container_slots.get(&object.object_id) != Some(&current_slots) {
            pending_game_events
                .events
                .push(GameEvent::ContainerChanged {
                    object_id: object.object_id,
                    slots: current_slots,
                });
        }
    }

    for stale_object_id in client_state.container_slots.keys() {
        if !current_container_ids.contains(stale_object_id) {
            pending_game_events
                .events
                .push(GameEvent::ContainerRemoved {
                    object_id: *stale_object_id,
                });
        }
    }

    let mut current_world_object_ids = Vec::new();
    for (tile_position, object, has_container, has_npc, has_movable) in &world_object_query {
        current_world_object_ids.push(object.object_id);
        let projected_object = ClientWorldObjectState {
            object_id: object.object_id,
            definition_id: object.definition_id.clone(),
            tile_position: *tile_position,
            is_container: has_container,
            is_npc: has_npc,
            is_movable: has_movable,
        };

        if client_state.world_objects.get(&object.object_id) != Some(&projected_object) {
            pending_game_events
                .events
                .push(GameEvent::WorldObjectUpserted {
                    object: projected_object,
                });
        }
    }

    for stale_object_id in client_state.world_objects.keys() {
        if !current_world_object_ids.contains(stale_object_id) {
            pending_game_events
                .events
                .push(GameEvent::WorldObjectRemoved {
                    object_id: *stale_object_id,
                });
        }
    }
}

pub fn apply_game_events_to_client_state(
    mut client_state: ResMut<ClientGameState>,
    mut pending_game_events: ResMut<PendingGameEvents>,
) {
    let events = std::mem::take(&mut pending_game_events.events);

    for event in events {
        match event {
            GameEvent::InventoryChanged { inventory } => {
                client_state.inventory = inventory;
            }
            GameEvent::ChatLogChanged { lines } => {
                client_state.chat_log_lines = lines;
            }
            GameEvent::PlayerPositionChanged { tile_position } => {
                client_state.player_tile_position = Some(tile_position);
            }
            GameEvent::PlayerVitalsChanged { vitals } => {
                client_state.player_vitals = Some(vitals);
            }
            GameEvent::PlayerStorageChanged { storage_slots } => {
                client_state.player_storage_slots = storage_slots;
            }
            GameEvent::CombatTargetChanged { target_object_id } => {
                client_state.current_target_object_id = target_object_id;
            }
            GameEvent::ContainerChanged { object_id, slots } => {
                client_state.container_slots.insert(object_id, slots);
            }
            GameEvent::ContainerRemoved { object_id } => {
                client_state.container_slots.remove(&object_id);
            }
            GameEvent::WorldObjectUpserted { object } => {
                client_state.world_objects.insert(object.object_id, object);
            }
            GameEvent::WorldObjectRemoved { object_id } => {
                client_state.world_objects.remove(&object_id);
            }
            GameEvent::RemotePlayerUpserted { player } => {
                client_state.remote_players.insert(player.player_id, player);
            }
            GameEvent::RemotePlayerRemoved { player_id } => {
                client_state.remote_players.remove(&player_id);
            }
        }
    }
}

fn handle_move_player(
    player_entity: Entity,
    delta: MoveDelta,
    world_config: &WorldConfig,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
) {
    let Ok((_, _, _, _, mut tile_position, mut movement_cooldown, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };

    if movement_cooldown.remaining_seconds > 0.0 {
        return;
    }

    let target_position = TilePosition::new(
        (tile_position.x + delta.x).clamp(0, world_config.map_width - 1),
        (tile_position.y + delta.y).clamp(0, world_config.map_height - 1),
    );

    let blocked = collider_query
        .iter()
        .any(|collider_position| *collider_position == target_position);

    if blocked {
        return;
    }

    *tile_position = target_position;
    movement_cooldown.remaining_seconds = movement_cooldown.step_interval_seconds;
}

fn handle_set_combat_target(
    player_entity: Entity,
    target_object_id: Option<u64>,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
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
    let Ok((_, _, _, mut chat_log_state, _, _, _, _)) = player_query.get_mut(player_entity) else {
        return;
    };

    match target_object_id {
        Some(object_id) => {
            let Some((target_entity, _)) = find_object_entity(object_id, object_query) else {
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
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    container_query: &mut Query<&mut Container>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    ui_events: &mut PendingGameUiEvents,
) {
    let Ok((_, player_identity, _, mut chat_log_state, player_position, _, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };
    let Some((entity, tile_position)) = find_object_entity(object_id, object_query) else {
        return;
    };

    if container_query.get_mut(entity).is_err() || !is_near_player(&player_position, &tile_position)
    {
        chat_log_state.push_narrator("That container is out of reach.");
        return;
    }

    ui_events.push(player_identity.id, GameUiEvent::OpenContainer { object_id });
}

fn handle_inspect(
    player_entity: Entity,
    target: InspectTarget,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
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
) {
    let Ok((_, _, _, mut chat_log_state, _, _, _, _)) = player_query.get_mut(player_entity) else {
        return;
    };
    let InspectTarget::Object(object_id) = target;
    if let Some(description) =
        object_description(object_id, object_registry, definitions, spell_definitions)
    {
        chat_log_state.push_narrator(description);
    }
}

fn handle_use_item(
    player_entity: Entity,
    source: ItemReference,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
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
    let Ok((_, _, mut inventory_state, mut chat_log_state, _, _, mut vital_stats, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };

    let object_id =
        item_reference_object_id(source, &inventory_state, container_query, object_query);
    let Some(object_id) = object_id else {
        return;
    };

    let Some(type_id) = object_registry.type_id(object_id) else {
        return;
    };
    let Some(definition) = definitions.get(type_id) else {
        return;
    };

    if let Some(spell_id) =
        object_registry.resolved_spell_id(object_id, definitions, spell_definitions)
    {
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

    let source_name = object_registry
        .display_name(object_id, definitions, spell_definitions)
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
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
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
            let Ok((_, _, inventory_state, mut chat_log_state, player_position, _, _, _)) =
                player_query.get_mut(player_entity)
            else {
                return;
            };
            let Some(source_object_id) =
                item_reference_object_id(source, &inventory_state, container_query, object_query)
            else {
                return;
            };
            let Some(source_type_id) = object_registry.type_id(source_object_id) else {
                return;
            };
            let Some(source_definition) = definitions.get(source_type_id) else {
                return;
            };
            let Some((_, target_position)) = find_object_entity(target_object_id, object_query)
            else {
                return;
            };
            if !is_near_player(&player_position, &target_position) {
                chat_log_state.push_narrator("That target is out of reach.");
                return;
            }
            let source_name = object_registry
                .display_name(source_object_id, definitions, spell_definitions)
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
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    npc_vitals_query: &mut Query<(&mut VitalStats, &OverworldObject), (With<Npc>, Without<Player>)>,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    commands: &mut Commands,
) {
    let Some(spell) = spell_definitions.get(spell_id) else {
        return;
    };
    let Some((target_entity, target_position)) = find_object_entity(target_object_id, object_query)
    else {
        return;
    };

    let Ok((
        _,
        _,
        mut inventory_state,
        mut chat_log_state,
        player_position,
        _,
        mut player_vitals,
        _,
    )) = player_query.get_mut(player_entity)
    else {
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
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    movable_query: &Query<
        (Entity, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    commands: &mut Commands,
) {
    let Ok((_, _, mut inventory_state, mut chat_log_state, player_position, _, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };

    match (source, destination) {
        (ItemReference::WorldObject(object_id), ItemDestination::Slot(slot_ref)) => {
            let Some((entity, tile_position)) = find_movable_entity(object_id, movable_query)
            else {
                return;
            };
            if !is_near_player(&player_position, &tile_position) {
                chat_log_state.push_narrator("That item is out of reach.");
                return;
            }
            if !place_item_in_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                object_id,
                slot_ref,
                object_registry,
                definitions,
            ) {
                return;
            }
            commands.entity(entity).despawn();
        }
        (ItemReference::WorldObject(object_id), ItemDestination::WorldTile(target_tile)) => {
            let Some((entity, origin)) = find_movable_entity(object_id, movable_query) else {
                return;
            };
            if is_valid_world_drop(
                target_tile,
                Some(origin),
                &player_position,
                entity,
                collider_query,
                movable_query,
                world_config,
            ) {
                commands.entity(entity).insert(target_tile);
            }
        }
        (ItemReference::Slot(slot_ref), ItemDestination::Slot(destination_ref)) => {
            if slot_ref == destination_ref {
                return;
            }
            let Some(object_id) = take_item_from_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                slot_ref,
            ) else {
                return;
            };
            if !place_item_in_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                object_id,
                destination_ref,
                object_registry,
                definitions,
            ) {
                restore_item_to_slot_ref(
                    &mut inventory_state,
                    container_query,
                    object_query,
                    slot_ref,
                    object_id,
                );
            }
        }
        (ItemReference::Slot(slot_ref), ItemDestination::WorldTile(target_tile)) => {
            let Some(object_id) = take_item_from_slot_ref(
                &mut inventory_state,
                container_query,
                object_query,
                slot_ref,
            ) else {
                return;
            };
            let Some(world_drop_tile) = find_nearest_valid_world_drop_tile(
                target_tile,
                None,
                &player_position,
                Entity::PLACEHOLDER,
                collider_query,
                movable_query,
                world_config,
            ) else {
                restore_item_to_slot_ref(
                    &mut inventory_state,
                    container_query,
                    object_query,
                    slot_ref,
                    object_id,
                );
                return;
            };

            let Some(type_id) = object_registry.type_id(object_id).map(str::to_owned) else {
                restore_item_to_slot_ref(
                    &mut inventory_state,
                    container_query,
                    object_query,
                    slot_ref,
                    object_id,
                );
                return;
            };

            spawn_overworld_object(
                commands,
                definitions,
                object_id,
                &type_id,
                None,
                world_drop_tile,
            );
        }
    }
}

fn handle_admin_spawn(
    player_entity: Entity,
    type_id: &str,
    tile_position: TilePosition,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    object_registry: &mut ObjectRegistry,
    commands: &mut Commands,
    player_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
) {
    let Ok((_, _, _, mut chat_log_state, _, _, _, _)) = player_query.get_mut(player_entity) else {
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
        && collider_query
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
        tile_position,
    );
    chat_log_state.push_narrator(format!(
        "Spawned {} as id {} at ({}, {}).",
        type_id, object_id, tile_position.x, tile_position.y
    ));
}

fn resolve_player_entity(
    player_id: Option<crate::player::components::PlayerId>,
    player_lookup_query: &Query<(Entity, &PlayerIdentity), With<Player>>,
) -> Option<Entity> {
    match player_id {
        Some(player_id) => player_lookup_query
            .iter()
            .find_map(|(entity, identity)| (identity.id == player_id).then_some(entity)),
        None => player_lookup_query.iter().next().map(|(entity, _)| entity),
    }
}

fn find_object_entity<'a>(
    object_id: u64,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
) -> Option<(Entity, TilePosition)> {
    object_query
        .iter()
        .find_map(|(entity, tile_position, object)| {
            (object.object_id == object_id).then_some((entity, *tile_position))
        })
}

fn find_movable_entity(
    object_id: u64,
    movable_query: &Query<
        (Entity, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
) -> Option<(Entity, TilePosition)> {
    movable_query
        .iter()
        .find_map(|(entity, tile_position, object)| {
            (object.object_id == object_id).then_some((entity, *tile_position))
        })
}

fn item_reference_object_id(
    item_reference: ItemReference,
    inventory_state: &InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
) -> Option<u64> {
    match item_reference {
        ItemReference::WorldObject(object_id) => Some(object_id),
        ItemReference::Slot(slot_ref) => {
            object_id_in_slot_ref(inventory_state, container_query, object_query, slot_ref)
        }
    }
}

fn object_id_in_slot_ref(
    inventory_state: &InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    slot_ref: ItemSlotRef,
) -> Option<u64> {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => inventory_state
            .backpack_slots
            .get(slot_index)
            .copied()
            .flatten(),
        ItemSlotRef::Equipment(slot) => inventory_state.equipment_item(slot),
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            let entity = find_container_entity(object_id, object_query)?;
            let container = container_query.get_mut(entity).ok()?;
            container.slots.get(slot_index).copied().flatten()
        }
    }
}

fn take_item_from_slot_ref(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    slot_ref: ItemSlotRef,
) -> Option<u64> {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            inventory_state.backpack_slots.get_mut(slot_index)?.take()
        }
        ItemSlotRef::Equipment(slot) => inventory_state.take_equipment_item(slot),
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

fn place_item_in_slot_ref(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    object_id: u64,
    slot_ref: ItemSlotRef,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    if !object_is_storable(object_id, object_registry, definitions) {
        return false;
    }

    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) else {
                return false;
            };
            if slot.is_some() {
                return false;
            }
            *slot = Some(object_id);
            true
        }
        ItemSlotRef::Equipment(slot) => place_item_in_equipment_slot(
            inventory_state,
            object_registry,
            definitions,
            slot,
            object_id,
        ),
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
            if slot.is_some() {
                return false;
            }
            *slot = Some(object_id);
            true
        }
    }
}

fn restore_item_to_slot_ref(
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    slot_ref: ItemSlotRef,
    object_id: u64,
) {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => {
            if let Some(slot) = inventory_state.backpack_slots.get_mut(slot_index) {
                *slot = Some(object_id);
            }
        }
        ItemSlotRef::Equipment(slot) => inventory_state.restore_equipment_item(slot, object_id),
        ItemSlotRef::Container {
            object_id: container_object_id,
            slot_index,
        } => {
            if let Some(entity) = find_container_entity(container_object_id, object_query) {
                if let Ok(mut container) = container_query.get_mut(entity) {
                    if let Some(slot) = container.slots.get_mut(slot_index) {
                        *slot = Some(object_id);
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
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
    commands: &mut Commands,
) {
    match item_reference {
        ItemReference::WorldObject(object_id) => {
            if let Some((entity, _)) = find_object_entity(object_id, object_query) {
                commands.entity(entity).despawn();
            }
        }
        ItemReference::Slot(slot_ref) => {
            let _ =
                take_item_from_slot_ref(inventory_state, container_query, object_query, slot_ref);
        }
    }
}

fn find_container_entity(
    object_id: u64,
    object_query: &Query<(Entity, &TilePosition, &OverworldObject), Without<Player>>,
) -> Option<Entity> {
    object_query
        .iter()
        .find_map(|(entity, _, object)| (object.object_id == object_id).then_some(entity))
}

fn place_item_in_equipment_slot(
    inventory_state: &mut InventoryState,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    slot: EquipmentSlot,
    object_id: u64,
) -> bool {
    let Some(type_id) = object_registry.type_id(object_id) else {
        return false;
    };
    let Some(definition) = definitions.get(type_id) else {
        return false;
    };
    if definition.equipment_slot != Some(slot) {
        return false;
    }

    inventory_state.place_equipment_item(slot, object_id)
}

fn object_description(
    object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
) -> Option<String> {
    let type_id = object_registry.type_id(object_id)?;
    let definition = definitions.get(type_id)?;
    let display_name = object_registry
        .display_name(object_id, definitions, spell_definitions)
        .unwrap_or_else(|| definition.name.clone());
    let description_text = object_registry
        .description(object_id, definitions, spell_definitions)
        .unwrap_or_else(|| definition.description.clone());
    let description = description_text.trim();
    if description.is_empty() {
        Some(format!("Just a {}.", display_name.to_lowercase()))
    } else {
        Some(description.to_owned())
    }
}

fn object_is_storable(
    object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    let Some(type_id) = object_registry.type_id(object_id) else {
        return false;
    };
    let Some(definition) = definitions.get(type_id) else {
        return false;
    };

    definition.storable
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
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::combat::components::{AttackProfile, CombatLeash};
    use crate::game::commands::{
        GameCommand, ItemDestination, ItemReference, ItemSlotRef, MoveDelta,
    };
    use crate::game::GameServerPlugin;
    use crate::magic::MagicPlugin;
    use crate::player::components::{
        BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, Player, PlayerId,
        PlayerIdentity, VitalStats,
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
        app
    }

    fn spawn_player(app: &mut App, player_id: u64, x: i32, y: i32) -> Entity {
        let base_stats = BaseStats::default();
        let derived_stats = DerivedStats::from_base(&base_stats);
        let max_health = derived_stats.max_health as f32;
        let max_mana = derived_stats.max_mana as f32;
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
                AttackProfile::melee(),
                CombatLeash {
                    max_distance_tiles: 6,
                },
                Collider,
                OverworldObject {
                    object_id,
                    definition_id: "player".to_owned(),
                },
                TilePosition::new(x, y),
            ))
            .id()
    }

    fn spawn_world_object(app: &mut App, type_id: &str, object_id: u64, tile: TilePosition) {
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
            tile,
        ));
        if definition.colliding {
            entity.insert(Collider);
        }
        if definition.movable {
            entity.insert(crate::world::components::Movable);
        }
        if definition.storable {
            entity.insert(crate::world::components::Storable);
        }
        if let Some(capacity) = definition.container_capacity {
            entity.insert(crate::world::components::Container {
                slots: vec![None; capacity],
            });
        }
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

        assert_eq!(positions[&1], TilePosition::new(11, 10));
        assert_eq!(positions[&2], TilePosition::new(12, 10));
        assert!(app.world().get_entity(player_one).is_ok());
        assert!(app.world().get_entity(player_two).is_ok());
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
        spawn_world_object(&mut app, "apple", apple_id, TilePosition::new(11, 10));

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

        assert_eq!(inventories[&1][0], Some(apple_id));
        assert_eq!(inventories[&2][0], None);

        let mut object_query = app
            .world_mut()
            .query::<&crate::world::components::OverworldObject>();
        assert!(!object_query
            .iter(app.world())
            .any(|object| object.object_id == apple_id));
    }
}

fn is_near_player(player_position: &TilePosition, target_position: &TilePosition) -> bool {
    (player_position.x - target_position.x).abs() <= 1
        && (player_position.y - target_position.y).abs() <= 1
}

fn is_valid_world_drop(
    target_tile: TilePosition,
    origin_tile: Option<TilePosition>,
    player_position: &TilePosition,
    dragged_entity: Entity,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    movable_query: &Query<
        (Entity, &TilePosition, &OverworldObject),
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

    if collider_query
        .iter()
        .any(|collider_position| *collider_position == target_tile)
    {
        return false;
    }

    !movable_query
        .iter()
        .any(|(entity, tile_position, _)| entity != dragged_entity && *tile_position == target_tile)
}

fn find_nearest_valid_world_drop_tile(
    target_tile: TilePosition,
    origin_tile: Option<TilePosition>,
    player_position: &TilePosition,
    dragged_entity: Entity,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
    movable_query: &Query<
        (Entity, &TilePosition, &OverworldObject),
        (With<Movable>, Without<Player>),
    >,
    world_config: &WorldConfig,
) -> Option<TilePosition> {
    let mut candidates = Vec::new();
    for y in -1..=1 {
        for x in -1..=1 {
            candidates.push(TilePosition::new(target_tile.x + x, target_tile.y + y));
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
            player_position,
            dragged_entity,
            collider_query,
            movable_query,
            world_config,
        )
    })
}
