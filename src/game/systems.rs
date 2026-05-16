use bevy::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::combat::components::CombatTarget;
use crate::game::commands::{
    GameCommand, InspectTarget, ItemDestination, ItemReference, ItemSlotRef, MoveDelta, UseTarget,
};
use crate::game::helpers::{colliders_in_space, is_near_player, player_space_id};
use crate::game::resources::{
    ChatLogState, ContainerViewers, GameUiEvent, InventoryState, PendingGameCommands,
    PendingGameUiEvents, VfxAnchor,
};
use crate::magic::resources::{SpellDefinition, SpellDefinitions};
use crate::npc::components::Npc;
use crate::player::components::{
    stack_weight, DerivedStats, Encumbered, EquippedItem, InventoryStack, MaxCarryWeight,
    MovementCooldown, Player, PlayerIdentity, VitalStats,
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
    AttackProfileKindDef, EquipmentSlot, OverworldObjectDefinition, OverworldObjectDefinitions,
};
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::setup::{resolve_portal_destination_space, spawn_overworld_object};
use crate::world::WorldConfig;
use bevy::ecs::system::SystemParam;

/// Bundle of side-output channels needed by `process_game_commands`. Bevy's
/// `IntoSystem` impl caps individual function-parameter count, so we pack
/// these together to leave headroom for the existing query mix. The
/// `player_regen_buffs` query is bundled in here for the same headroom
/// reason — it's mutated by `handle_use_item` when the player consumes a
/// food/drink with `regen_duration_seconds > 0`.
#[derive(SystemParam)]
pub struct CommandOutputs<'w, 's> {
    pub ui_events: ResMut<'w, PendingGameUiEvents>,
    pub container_viewers: ResMut<'w, ContainerViewers>,
    pub player_regen_buffs:
        Query<'w, 's, &'static mut crate::player::components::RegenBuffs, With<Player>>,
    pub player_magic_effects:
        Query<'w, 's, &'static mut crate::magic::effects::MagicEffects, With<Player>>,
    /// Per-NPC magical effects. Used by spell handlers to insert/apply
    /// debuffs on the target. Insertion happens lazily via `Commands` when
    /// an NPC doesn't already carry the component.
    pub npc_magic_effects: Query<
        'w,
        's,
        &'static mut crate::magic::effects::MagicEffects,
        (With<Npc>, Without<Player>),
    >,
    pub player_carry: Query<'w, 's, &'static MaxCarryWeight, With<Player>>,
    /// True iff the player entity carries the `Encumbered` marker. Doubles
    /// the movement cooldown when set.
    pub player_encumbered: Query<'w, 's, (), (With<Player>, With<Encumbered>)>,
    /// Read-only access to the player's `Class` + `Experience` so the spell
    /// cast paths can apply `class_access` / `min_caster_level` gating.
    pub player_class_level: Query<
        'w,
        's,
        (
            Option<&'static crate::player::classes::Class>,
            Option<&'static crate::player::progression::Experience>,
        ),
        With<Player>,
    >,
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
    mut floor_dirty: ResMut<crate::world::floor_render::FloorRenderDirty>,
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
                    if map.set(x, y, floor_type) {
                        // Notify the editor's render system which tile changed
                        // so it can do a per-corner incremental rebuild instead
                        // of redrawing the whole grid every paint.
                        floor_dirty.cells.push((space_id, z, x, y));
                    } else {
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
                let encumbered = command_outputs.player_encumbered.get(player_entity).is_ok();
                handle_move_player(
                    player_entity,
                    delta,
                    &collider_positions,
                    &object_query,
                    &mut player_queries.p2(),
                    &command_outputs.player_magic_effects,
                    &authored_spaces,
                    &definitions,
                    &mut space_authority.space_manager,
                    &mut space_authority.floor_maps,
                    encumbered,
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
                    &mut command_outputs.player_regen_buffs,
                    &mut command_outputs.player_magic_effects,
                    &command_outputs.player_class_level,
                    &mut object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut command_outputs.ui_events,
                    &mut pending_commands,
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
                    &mut command_outputs.player_regen_buffs,
                    &mut command_outputs.player_magic_effects,
                    &command_outputs.player_class_level,
                    &object_query,
                    &mut object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut command_outputs.ui_events,
                    &mut pending_commands,
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
                    &mut command_outputs.player_magic_effects,
                    &mut command_outputs.npc_magic_effects,
                    &command_outputs.player_class_level,
                    &mut object_registry,
                    &definitions,
                    &spell_definitions,
                    &mut command_outputs.ui_events,
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
                    &command_outputs.player_carry,
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
                    &command_outputs.player_carry,
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
                    &command_outputs.player_carry,
                );
            }
            GameCommand::TakeItem { type_id, count } => {
                handle_take_item(player_entity, &type_id, count, &mut player_queries.p2());
            }
            GameCommand::AdminTeleport {
                space_id,
                tile_position,
            } => {
                handle_admin_teleport(
                    player_entity,
                    space_id,
                    tile_position,
                    &space_authority.space_manager,
                    &mut player_queries.p2(),
                );
            }
            GameCommand::AdminDespawn { object_id } => {
                handle_admin_despawn(object_id, &object_query, &mut commands);
            }
            GameCommand::AdminSetVitals { health, mana } => {
                handle_admin_set_vitals(player_entity, health, mana, &mut player_queries.p2());
            }
            GameCommand::AdminSetObjectState { .. } => {
                // Drained by `process_interact_commands` in `CommandIntercept`.
                bevy::log::warn!(
                    "process_game_commands saw AdminSetObjectState — check system ordering"
                );
            }
            // Drained earlier by `handle_set_home_commands` (player plugin,
            // CommandIntercept set). If we reach this arm, no player matched
            // the queued command so silently drop it.
            GameCommand::SetHome => {}
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
            GameCommand::InteractWithObject { .. } | GameCommand::ApplyToolInteraction { .. } => {
                // Drained by `process_interact_commands` in `CommandIntercept`.
                bevy::log::warn!(
                    "process_game_commands saw an interact command — check system ordering"
                );
            }
            GameCommand::InitiateTrade { .. }
            | GameCommand::OfferTradeItem { .. }
            | GameCommand::WithdrawTradeItem { .. }
            | GameCommand::ToggleTradeReady { .. }
            | GameCommand::ConfirmTrade { .. }
            | GameCommand::CancelTrade { .. }
            | GameCommand::BrowseShopBuy { .. } => {
                // Drained by `process_trade_commands` in `CommandIntercept`
                // before this system runs.
                bevy::log::warn!(
                    "process_game_commands saw a trade command — check system ordering"
                );
            }
            GameCommand::StashMutate { .. }
            | GameCommand::LearnRecipe { .. }
            | GameCommand::CraftItem { .. } => {
                // Drained by crafting systems (CraftingServerPlugin) in
                // `CommandIntercept` before this system runs.
                bevy::log::warn!(
                    "process_game_commands saw a crafting command — check system ordering"
                );
            }
            GameCommand::Say { .. } => {
                // Drained by `process_say_commands` in `CommandIntercept`
                // before this system runs.
                bevy::log::warn!(
                    "process_game_commands saw a chat command — check system ordering"
                );
            }
            GameCommand::UpsertLogEntry { .. }
            | GameCommand::DeleteLogEntry { .. }
            | GameCommand::SetQuestPlayerNotes { .. } => {
                // Drained by `process_log_commands` (LogServerPlugin) in
                // `CommandIntercept` before this system runs.
                bevy::log::warn!("process_game_commands saw a log command — check system ordering");
            }
            GameCommand::AllocateSkillPoint { .. } => {
                // Drained by `process_allocate_skill_commands` (PlayerServerPlugin)
                // in `CommandIntercept` before this system runs.
                bevy::log::warn!(
                    "process_game_commands saw an allocate-skill command — check system ordering"
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
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
) {
    if count == 0 {
        return;
    }
    let Some(definition) = definitions.get(type_id) else {
        bevy::log::warn!("GiveItem: unknown type_id '{type_id}'");
        return;
    };
    let max_carry = max_carry_query
        .get(player_entity)
        .copied()
        .unwrap_or_default();
    let Ok((_, _, mut inventory, mut chat_log, _, _, _, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };

    let max_stack = definition.max_stack_size.max(1);
    let per_unit_weight = definition.weight;
    let mut remaining = count;

    let mut current_weight = inventory.total_weight(definitions);
    let mut weight_capped = false;

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
            let mut grant = remaining.min(available);
            if per_unit_weight > 0.0 {
                let headroom = (max_carry.hard_cap - current_weight).max(0.0);
                let max_by_weight = (headroom / per_unit_weight).floor() as u32;
                if grant > max_by_weight {
                    grant = max_by_weight;
                    weight_capped = true;
                }
            }
            if grant == 0 {
                continue;
            }
            stack.quantity += grant;
            current_weight += per_unit_weight * grant as f32;
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
        let mut grant = if max_stack > 1 {
            remaining.min(max_stack)
        } else {
            1
        };
        if per_unit_weight > 0.0 {
            let headroom = (max_carry.hard_cap - current_weight).max(0.0);
            let max_by_weight = (headroom / per_unit_weight).floor() as u32;
            if grant > max_by_weight {
                grant = max_by_weight;
                weight_capped = true;
            }
        }
        if grant == 0 {
            break;
        }
        let mut stack = InventoryStack::item(type_id.to_owned(), ObjectProperties::new(), grant);
        // Pouches granted this way (admin /give, dialog give_item, crafting
        // outputs, scripting) need their contents vec pre-initialized so
        // the inventory UI treats them as openable containers. Without
        // this, the player has to drop and re-pick the pouch before the
        // "Open" action shows up.
        if let Some(capacity) = definition.container_capacity {
            stack.contained_slots = Some(vec![None; capacity]);
        }
        // Charged items spawn fully-charged. Infinite items never carry a
        // `charges_remaining` key (decoded as ∞ by the use/tooltip paths).
        if let Some(max_charges) = definition.max_charges {
            if !definition.infinite_uses {
                stack.set_charges_remaining(max_charges);
            }
        }
        inventory.backpack_slots[empty_index] = Some(stack);
        current_weight += per_unit_weight * grant as f32;
        remaining -= grant;
    }

    if weight_capped && remaining > 0 {
        chat_log.push_narrator(format!(
            "Too heavy — you cannot carry any more {}.",
            definition.name
        ));
    }
    let granted = count - remaining;
    if granted > 0 {
        chat_log.push_narrator(format!("You receive {} {}.", granted, definition.name));
    }
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

#[allow(clippy::too_many_arguments)]
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
    player_magic_effects: &Query<&mut crate::magic::effects::MagicEffects, With<Player>>,
    authored_spaces: &SpaceDefinitions,
    definitions: &OverworldObjectDefinitions,
    space_manager: &mut SpaceManager,
    floor_maps: &mut FloorMaps,
    encumbered: bool,
    commands: &mut Commands,
) {
    let Ok((_, _, _, _, mut space_resident, mut tile_position, mut movement_cooldown, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };

    if movement_cooldown.remaining_seconds > 0.0 {
        return;
    }

    // Paralyze blocks movement entirely. Drunk fumbles the direction.
    let (effective_delta, drunk_cooldown_penalty) =
        if let Ok(effects) = player_magic_effects.get(player_entity) {
            if effects.is_paralyzed() {
                return;
            }
            let deviation = effects.drunk_deviation_probability();
            match deviation {
                Some(probability) if drunk_should_deviate(player_entity, probability) => (
                    rotate_delta(delta, drunk_rotation_sign(player_entity)),
                    true,
                ),
                _ => (delta, false),
            }
        } else {
            (delta, false)
        };

    let Some(runtime_space) = space_manager.get(space_resident.space_id).cloned() else {
        return;
    };

    let target_xy = (
        (tile_position.x + effective_delta.x).clamp(0, runtime_space.width - 1),
        (tile_position.y + effective_delta.y).clamp(0, runtime_space.height - 1),
    );

    let Some(target_position) = resolve_step_with_climb(
        target_xy,
        tile_position.z,
        space_resident.space_id,
        collider_positions,
        object_query,
        definitions,
    ) else {
        return;
    };

    *tile_position = target_position;
    let mut cooldown_scale = if encumbered { 2.0 } else { 1.0 };
    if let Ok(effects) = player_magic_effects.get(player_entity) {
        cooldown_scale *= effects.haste_multiplier();
    }
    if effective_delta.x != 0 && effective_delta.y != 0 {
        cooldown_scale *= std::f32::consts::SQRT_2;
    }
    if drunk_cooldown_penalty {
        // A fumbled drunken step takes a beat to recover from.
        cooldown_scale *= 1.25;
    }
    movement_cooldown.remaining_seconds = movement_cooldown.step_interval_seconds * cooldown_scale;

    if let Some(direction) = Direction::from_delta(effective_delta.x, effective_delta.y) {
        commands.entity(player_entity).insert(Facing(direction));
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

/// Sample a deterministic boolean for drunken fumbling. The nanosecond +
/// entity-index salt mirrors `combat::systems::roll_defense` — good enough
/// for "occasional" without bringing in a real RNG resource.
fn drunk_should_deviate(player_entity: Entity, probability: f32) -> bool {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let salt = (player_entity.to_bits()).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mixed = nanos.wrapping_add(salt);
    let roll = (mixed % 1_000_000) as f32 / 1_000_000.0;
    roll < probability.clamp(0.0, 1.0)
}

/// Returns `+1` or `-1` deterministically — which way to rotate a drunken
/// step. Mirror of `drunk_should_deviate` so successive calls within the
/// same step are stable.
fn drunk_rotation_sign(player_entity: Entity) -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let salt = (player_entity.to_bits()).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    if nanos.wrapping_add(salt) & 1 == 0 {
        1
    } else {
        -1
    }
}

/// Rotate a `MoveDelta` 45° clockwise (sign = +1) or counter-clockwise
/// (sign = -1). All 8 compass directions stay on the compass after rotation.
fn rotate_delta(delta: MoveDelta, sign: i32) -> MoveDelta {
    // Map (dx, dy) to angle index 0..8 around the compass, then ±1 step.
    const COMPASS: [(i32, i32); 8] = [
        (0, 1),   // N
        (1, 1),   // NE
        (1, 0),   // E
        (1, -1),  // SE
        (0, -1),  // S
        (-1, -1), // SW
        (-1, 0),  // W
        (-1, 1),  // NW
    ];
    let Some(index) = COMPASS
        .iter()
        .position(|&(x, y)| x == delta.x.signum() && y == delta.y.signum())
    else {
        return delta;
    };
    let rotated = (index as i32 + sign).rem_euclid(8) as usize;
    let (dx, dy) = COMPASS[rotated];
    MoveDelta { x: dx, y: dy }
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
                ItemSlotRef::PouchInBackpack {
                    backpack_slot,
                    sub_slot,
                } => inventory_state
                    .backpack_slots
                    .get(backpack_slot)
                    .and_then(|slot| slot.as_ref())
                    .and_then(|parent| parent.contained_slots.as_ref())
                    .and_then(|inner| inner.get(sub_slot))
                    .cloned()
                    .flatten()
                    .map(|s| (s.type_id, s.properties, s.quantity, None)),
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

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
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
    regen_buffs_query: &mut Query<&mut crate::player::components::RegenBuffs, With<Player>>,
    magic_effects_query: &mut Query<&mut crate::magic::effects::MagicEffects, With<Player>>,
    player_class_level: &Query<
        (
            Option<&crate::player::classes::Class>,
            Option<&crate::player::progression::Experience>,
        ),
        With<Player>,
    >,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    ui_events: &mut PendingGameUiEvents,
    pending_commands: &mut PendingGameCommands,
    commands: &mut Commands,
) {
    let Ok((
        _,
        identity,
        mut inventory_state,
        mut chat_log_state,
        player_space_resident,
        player_position,
        _,
        mut vital_stats,
        _,
    )) = player_query.get_mut(player_entity)
    else {
        return;
    };
    let acting_player_id = identity.id;
    let player_space_id = player_space_resident.space_id;
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
        // Today everything routed through this branch is a scroll-shaped item
        // (its definition declares `spell_id`). When a memorized-spell cast
        // path lands later, `is_scroll` should reflect *that* distinction; for
        // now mark as scroll so class_access is bypassed but level is checked.
        let is_scroll = true;
        let (class, level) = player_class_level
            .get(player_entity)
            .map(|(c, e)| (c.copied(), e.map_or(1, |exp| exp.level)))
            .unwrap_or((None, 1));
        if let Err(reason) = check_caster_eligibility(spell, is_scroll, class, level) {
            chat_log_state.push_narrator(reason);
            return;
        }
        if vital_stats.mana < spell.mana_cost {
            chat_log_state.push_narrator(format!("Not enough mana to cast {}.", spell.name));
            return;
        }
        vital_stats.mana = (vital_stats.mana - spell.mana_cost).max(0.0);
        let cast_vfx_id = spell
            .effects
            .vfx_on_cast
            .clone()
            .unwrap_or_else(|| "cast_flash".to_owned());
        ui_events.push_broadcast(GameUiEvent::VfxSpawn {
            definition_id: cast_vfx_id,
            anchor: VfxAnchor::tile(player_space_id, player_position),
        });
        apply_spell_effects(spell, &mut vital_stats);
        if let Ok(mut effects) = magic_effects_query.get_mut(player_entity) {
            apply_spell_self_effects(spell, &mut effects);
        }
        if let Some(spawn_spec) = spell.effects.spawns_object.as_ref() {
            spawn_spell_object(
                commands,
                definitions,
                object_registry,
                spawn_spec,
                player_space_id,
                player_position,
            );
        }
        let outcome = consume_or_decrement_charge(
            source,
            &mut inventory_state,
            container_query,
            object_query,
            object_registry,
            definitions,
            commands,
        );
        chat_log_state.push_line(format!("[Player]: \"{}\"", spell.incantation));
        chat_log_state.push_narrator(charge_narrator_line(
            &spell.name,
            &view.type_id,
            definitions,
            outcome,
        ));
        return;
    }

    // Recipe-scroll path: a one-shot consumable that teaches a recipe.
    // Mirrors the spell-scroll branch above — queue a `LearnRecipe`
    // command (drained next frame by `process_learn_recipe_commands`),
    // consume the scroll, and emit a narrator line. We do NOT short-
    // circuit the `use_effects` path below; a scroll with both
    // `learns_recipe` and `restore_health` would heal on use too.
    // Skip if the recipe is unknown so the scroll isn't wasted on a typo.
    if let Some(recipe_id) = definition.learns_recipe.as_ref() {
        pending_commands.push_for_player(
            acting_player_id,
            crate::game::commands::GameCommand::LearnRecipe {
                recipe_id: recipe_id.clone(),
            },
        );
        consume_or_decrement_charge(
            source,
            &mut inventory_state,
            container_query,
            object_query,
            object_registry,
            definitions,
            commands,
        );
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

    let new_multiplier = definition.use_effects.regen_multiplier.max(1.0);
    let new_duration = definition.use_effects.regen_duration_seconds.max(0.0);
    if new_duration > 0.0 && new_multiplier > 1.0 {
        match regen_buffs_query.get_mut(player_entity) {
            Ok(mut buffs) => {
                buffs.remaining_seconds += new_duration;
                buffs.multiplier = buffs.multiplier.max(new_multiplier);
                bevy::log::info!(
                    "regen buff applied: x{:.1} for {:.0}s (now {:.1}s remaining)",
                    buffs.multiplier,
                    new_duration,
                    buffs.remaining_seconds,
                );
            }
            Err(err) => {
                bevy::log::warn!(
                    "regen buff dropped: player entity has no RegenBuffs component ({err:?})"
                );
            }
        }
    }

    consume_or_decrement_charge(
        source,
        &mut inventory_state,
        container_query,
        object_query,
        object_registry,
        definitions,
        commands,
    );
    chat_log_state.push_narrator(use_text(definition, &source_name));
}

#[allow(clippy::too_many_arguments)]
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
    regen_buffs_query: &mut Query<&mut crate::player::components::RegenBuffs, With<Player>>,
    magic_effects_query: &mut Query<&mut crate::magic::effects::MagicEffects, With<Player>>,
    player_class_level: &Query<
        (
            Option<&crate::player::classes::Class>,
            Option<&crate::player::progression::Experience>,
        ),
        With<Player>,
    >,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    ui_events: &mut PendingGameUiEvents,
    pending_commands: &mut PendingGameCommands,
    commands: &mut Commands,
) {
    match target {
        UseTarget::Player => handle_use_item(
            player_entity,
            source,
            container_query,
            object_query,
            player_query,
            regen_buffs_query,
            magic_effects_query,
            player_class_level,
            object_registry,
            definitions,
            spell_definitions,
            ui_events,
            pending_commands,
            commands,
        ),
        UseTarget::Object(target_object_id) => {
            let Ok((
                _,
                identity,
                mut inventory_state,
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
            let acting_player_id = identity.id;
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

            // Gather flow: if the target carries an interaction whose tool_gate
            // names the source item's type, consume a charge and re-queue the
            // interaction via `ApplyToolInteraction` (drained next frame by
            // `process_interact_commands`). The same handler runs skill_gate +
            // transition + grants + respawn + side_effects, skipping the
            // tool_gate check since we already matched on it here.
            if let Some(verb) = find_tool_gate_verb_on_target(
                &source_view.type_id,
                target_object_id,
                object_registry,
                definitions,
            ) {
                consume_or_decrement_charge(
                    source,
                    &mut inventory_state,
                    container_query,
                    object_query,
                    object_registry,
                    definitions,
                    commands,
                );
                pending_commands.push_for_player(
                    acting_player_id,
                    crate::game::commands::GameCommand::ApplyToolInteraction {
                        target_object_id,
                        verb,
                    },
                );
                chat_log_state.push_narrator(use_on_text(
                    source_definition,
                    &source_name,
                    &target_name,
                ));
                return;
            }

            chat_log_state.push_narrator(use_on_text(
                source_definition,
                &source_name,
                &target_name,
            ));
        }
    }
}

/// Walk the target object's interactions looking for one whose `tool_gate`
/// names `source_type_id`. Returns the matching verb on success. The interaction's
/// `from`-state filter must match the target's current state (empty `from` = any).
fn find_tool_gate_verb_on_target(
    source_type_id: &str,
    target_object_id: u64,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
) -> Option<String> {
    let target_type_id = object_registry.type_id(target_object_id)?;
    let target_def = definitions.get(target_type_id)?;
    let current_state = object_registry
        .properties(target_object_id)
        .and_then(|p| p.get("state").cloned())
        .or_else(|| target_def.initial_state.clone());
    for interaction in &target_def.interactions {
        let Some(gate) = &interaction.tool_gate else {
            continue;
        };
        if gate.required_type_id != source_type_id {
            continue;
        }
        if !interaction.from.is_empty() {
            match &current_state {
                Some(state) if interaction.from.iter().any(|s| s == state) => {}
                _ => continue,
            }
        }
        return Some(interaction.verb.clone());
    }
    None
}

#[allow(clippy::too_many_arguments)]
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
    player_magic_effects_query: &mut Query<&mut crate::magic::effects::MagicEffects, With<Player>>,
    npc_magic_effects_query: &mut Query<
        &mut crate::magic::effects::MagicEffects,
        (With<Npc>, Without<Player>),
    >,
    player_class_level: &Query<
        (
            Option<&crate::player::classes::Class>,
            Option<&crate::player::progression::Experience>,
        ),
        With<Player>,
    >,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
    ui_events: &mut PendingGameUiEvents,
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

    // Class + level gating. Same scroll-bypass rule as `handle_use_item`.
    let is_scroll = true;
    let (class, level) = player_class_level
        .get(player_entity)
        .map(|(c, e)| (c.copied(), e.map_or(1, |exp| exp.level)))
        .unwrap_or((None, 1));
    if let Err(reason) = check_caster_eligibility(spell, is_scroll, class, level) {
        chat_log_state.push_narrator(reason);
        return;
    }

    // Paralyzed casters can't form the incantation. Cheaper to read effects
    // through the dedicated query than to thread a separate parameter.
    if let Ok(effects) = player_magic_effects_query.get(player_entity) {
        if effects.is_paralyzed() {
            chat_log_state
                .push_narrator(format!("You're paralyzed and can't cast {}.", spell.name));
            return;
        }
    }

    if player_vitals.mana < spell.mana_cost {
        chat_log_state.push_narrator(format!("Not enough mana to cast {}.", spell.name));
        return;
    }
    player_vitals.mana = (player_vitals.mana - spell.mana_cost).max(0.0);

    let caster_space_id = player_space_resident.space_id;
    let caster_tile = *player_position;
    let cast_vfx_id = spell
        .effects
        .vfx_on_cast
        .clone()
        .unwrap_or_else(|| "cast_flash".to_owned());
    ui_events.push_broadcast(GameUiEvent::VfxSpawn {
        definition_id: cast_vfx_id,
        anchor: VfxAnchor::tile(caster_space_id, caster_tile),
    });

    let (target_died, target_name, target_definition_id) = {
        let Ok((mut target_vitals, target_object)) = npc_vitals_query.get_mut(target_entity) else {
            return;
        };
        let name = object_registry
            .display_name(target_object.object_id, definitions, spell_definitions)
            .unwrap_or_else(|| target_object.definition_id.clone());
        let definition_id = target_object.definition_id.clone();
        apply_spell_effects(spell, &mut target_vitals);
        (target_vitals.health <= 0.0, name, definition_id)
    };

    let impact_vfx_id = spell
        .effects
        .vfx_on_target_hit
        .clone()
        .unwrap_or_else(|| "hit_flash".to_owned());
    ui_events.push_broadcast(GameUiEvent::VfxSpawn {
        definition_id: impact_vfx_id,
        anchor: VfxAnchor::follow(target_object_id),
    });

    if !spell.effects.buffs_target.is_empty() && !target_died {
        apply_buffs_target(
            target_entity,
            &spell.effects.buffs_target,
            npc_magic_effects_query,
            commands,
        );
    }

    if let Ok(mut effects) = player_magic_effects_query.get_mut(player_entity) {
        apply_spell_self_effects(spell, &mut effects);
    }

    if let Some(spawn_spec) = spell.effects.spawns_object.as_ref() {
        spawn_spell_object(
            commands,
            definitions,
            object_registry,
            spawn_spec,
            player_space_resident.space_id,
            target_position,
        );
    }

    consume_or_decrement_charge(
        source,
        &mut inventory_state,
        container_query,
        object_query,
        object_registry,
        definitions,
        commands,
    );
    chat_log_state.push_line(format!("[Player]: \"{}\"", spell.incantation));
    if spell.effects.damage > 0.0 {
        chat_log_state.push_narrator(format!(
            "Cast {} on {} ({} damage).",
            spell.name,
            target_name,
            spell.effects.effective_damage_type().display_name()
        ));
    } else {
        chat_log_state.push_narrator(format!("Cast {} on {}.", spell.name, target_name));
    }

    if target_died {
        if let Some(loot_table) = definitions
            .get(&target_definition_id)
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

/// Returns `Ok(())` when the caster is permitted to cast `spell` from the
/// given source.
///
/// Rules (matches the project plan):
/// - `min_caster_level` is always enforced. Missing `Experience` defaults to
///   level 1.
/// - `class_access` is enforced **only when the source is not a scroll**
///   (today every cast goes through an item with `spell_id`, so this is
///   effectively a no-op until a memorized-spell path lands in Phase E).
/// - Empty `class_access` means "any class".
fn check_caster_eligibility(
    spell: &SpellDefinition,
    is_scroll: bool,
    class: Option<crate::player::classes::Class>,
    level: u32,
) -> Result<(), String> {
    if spell.min_caster_level > 0 && level < spell.min_caster_level {
        return Err(format!(
            "Not high enough level to cast {} (requires {}).",
            spell.name, spell.min_caster_level
        ));
    }
    if !is_scroll && !spell.class_access.is_empty() {
        match class {
            Some(c) if spell.class_access.contains(&c) => {}
            _ => {
                return Err(format!("You can't cast {} — wrong class.", spell.name));
            }
        }
    }
    Ok(())
}

/// Spawn the transient world object declared by a spell's `spawns_object`
/// effect (currently only `magic_light`). Attaches a `Ttl` so the object
/// auto-cleans up after `lifetime_seconds`.
fn spawn_spell_object(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    object_registry: &mut ObjectRegistry,
    spawn_spec: &crate::magic::resources::SpawnObjectSpec,
    space_id: crate::world::components::SpaceId,
    tile_position: TilePosition,
) {
    let type_id = spawn_spec.type_id.as_str();
    if definitions.get(type_id).is_none() {
        return;
    }
    let object_id = object_registry.allocate_runtime_id(type_id);
    let entity = crate::world::setup::spawn_overworld_object(
        commands,
        definitions,
        object_id,
        type_id,
        None,
        space_id,
        tile_position,
        None,
    );
    commands.entity(entity).insert(crate::world::ttl::Ttl {
        remaining_seconds: spawn_spec.lifetime_seconds.max(1.0),
    });
}

/// Inserts (or merges) `MagicEffects` on an NPC target. Lazily attaches the
/// component if missing.
fn apply_buffs_target(
    target_entity: Entity,
    specs: &[crate::magic::resources::EffectSpec],
    npc_magic_effects_query: &mut Query<
        &mut crate::magic::effects::MagicEffects,
        (With<Npc>, Without<Player>),
    >,
    commands: &mut Commands,
) {
    if let Ok(mut effects) = npc_magic_effects_query.get_mut(target_entity) {
        for spec in specs {
            effects.apply(*spec);
        }
    } else {
        let mut new_effects = crate::magic::effects::MagicEffects::default();
        for spec in specs {
            new_effects.apply(*spec);
        }
        commands.entity(target_entity).insert(new_effects);
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
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
    commands: &mut Commands,
) {
    let max_carry = max_carry_query
        .get(player_entity)
        .copied()
        .unwrap_or_default();
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
            // Pouches: capture container contents so they round-trip through
            // pickup → drop. The Container component lives on the world entity
            // and would otherwise vanish at despawn. Skip storage for non-
            // container types to keep `contained_slots: None` for normal items.
            let mut stack = InventoryStack::item(definition_id, properties, quantity);
            if let Ok(container) = container_query.get(entity) {
                stack.contained_slots = Some(container.slots.clone());
            }
            if is_player_destination(slot_ref)
                && would_overload_carry(&inventory_state, &stack, &max_carry, definitions)
            {
                chat_log_state.push_narrator("Too heavy — you can't lift that.");
                return;
            }
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
            // Check carry weight only on cross-boundary (non-player → player)
            // moves. Within-player rearranges are always allowed; the source
            // weight has already been removed by the take above so the helper
            // would mis-report otherwise.
            let crosses_into_player =
                !is_player_source_slot(slot_ref) && is_player_destination(destination_ref);
            let stack_for_restore = stack.clone();
            if crosses_into_player
                && would_overload_carry(&inventory_state, &stack, &max_carry, definitions)
            {
                chat_log_state.push_narrator("Too heavy — you can't lift that.");
                restore_stack_to_slot_ref(
                    &mut inventory_state,
                    container_query,
                    object_query,
                    slot_ref,
                    stack_for_restore,
                );
                return;
            }
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
                    stack.contained_slots.clone(),
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
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
    commands: &mut Commands,
) {
    let max_carry = max_carry_query
        .get(player_entity)
        .copied()
        .unwrap_or_default();
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
                    let crosses_into_player =
                        !is_player_source_slot(src_slot_ref) && is_player_destination(dst_slot_ref);
                    if crosses_into_player {
                        let probe_stack = InventoryStack::item(
                            src_type_id.clone(),
                            src_properties.clone(),
                            amount,
                        );
                        if would_overload_carry(
                            &inventory_state,
                            &probe_stack,
                            &max_carry,
                            definitions,
                        ) {
                            chat_log_state.push_narrator("Too heavy — you can't lift that.");
                            return;
                        }
                    }
                    let dst_stack = stack_in_slot_ref(
                        &inventory_state,
                        container_query,
                        object_query,
                        dst_slot_ref,
                    );
                    match dst_stack {
                        None => {
                            let new_stack = InventoryStack::item(
                                src_type_id.clone(),
                                src_properties.clone(),
                                amount,
                            );
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

            let new_stack = InventoryStack::item(definition_id, properties, actual_amount);
            let destination_slot = match destination {
                ItemDestination::Slot(s) => s,
                ItemDestination::WorldTile(_) => return,
            };
            if is_player_destination(destination_slot)
                && would_overload_carry(&inventory_state, &new_stack, &max_carry, definitions)
            {
                chat_log_state.push_narrator("Too heavy — you can't lift that.");
                return;
            }
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

    // Seed `charges_remaining` for charged-but-not-infinite items so the
    // freshly-spawned wand is at full charges. Skipped for infinite items so
    // the tooltip path correctly renders "∞".
    let mut initial_properties = ObjectProperties::new();
    if let Some(max_charges) = definition.max_charges {
        if !definition.infinite_uses {
            initial_properties.insert(
                crate::player::components::CHARGES_KEY.to_owned(),
                max_charges.to_string(),
            );
        }
    }
    let object_id = if initial_properties.is_empty() {
        object_registry.allocate_runtime_id(type_id.to_owned())
    } else {
        object_registry.allocate_runtime_id_with_properties(type_id.to_owned(), initial_properties)
    };
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

fn handle_admin_teleport(
    player_entity: Entity,
    target_space_id: Option<crate::world::components::SpaceId>,
    target_tile: TilePosition,
    space_manager: &SpaceManager,
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
    let Ok((_, _, _, mut chat_log_state, mut space_resident, mut tile_position, _, _, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };

    let resolved_space_id = target_space_id.unwrap_or(space_resident.space_id);
    let Some(runtime_space) = space_manager.get(resolved_space_id) else {
        chat_log_state.push_narrator(format!(
            "Teleport rejected: unknown space id {}.",
            resolved_space_id.0
        ));
        return;
    };
    if !runtime_space.contains(target_tile) {
        chat_log_state.push_narrator(format!(
            "Teleport rejected: ({}, {}) outside space {}.",
            target_tile.x, target_tile.y, resolved_space_id.0
        ));
        return;
    }

    space_resident.space_id = resolved_space_id;
    *tile_position = target_tile;
    chat_log_state.push_narrator(format!(
        "Teleported to ({}, {}, z={}) in space {}.",
        target_tile.x, target_tile.y, target_tile.z, resolved_space_id.0
    ));
}

fn handle_admin_despawn(
    object_id: u64,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    commands: &mut Commands,
) {
    let Some((entity, _, _, _)) = object_query
        .iter()
        .find(|(_, _, _, object)| object.object_id == object_id)
    else {
        bevy::log::debug!("AdminDespawn: object {object_id} not found");
        return;
    };
    commands.entity(entity).despawn();
}

fn handle_admin_set_vitals(
    player_entity: Entity,
    health: Option<f32>,
    mana: Option<f32>,
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
    let Ok((_, _, _, mut chat_log_state, _, _, _, mut vitals, _)) =
        player_query.get_mut(player_entity)
    else {
        return;
    };
    if let Some(value) = health {
        vitals.health = value.clamp(0.0, vitals.max_health);
    }
    if let Some(value) = mana {
        vitals.mana = value.clamp(0.0, vitals.max_mana);
    }
    chat_log_state.push_narrator(format!(
        "Vitals updated: health={}/{}, mana={}/{}",
        vitals.health, vitals.max_health, vitals.mana, vitals.max_mana
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
            InventoryStack::item(item.type_id.clone(), item.properties.clone(), quantity)
        }),
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            let entity = find_container_entity(object_id, object_query)?;
            let container = container_query.get_mut(entity).ok()?;
            container.slots.get(slot_index).cloned().flatten()
        }
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => inventory_state
            .backpack_slots
            .get(backpack_slot)?
            .as_ref()?
            .contained_slots
            .as_ref()?
            .get(sub_slot)
            .cloned()
            .flatten(),
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
                .map(|item| InventoryStack::item(item.type_id, item.properties, quantity))
        }
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            let entity = find_container_entity(object_id, object_query)?;
            let mut container = container_query.get_mut(entity).ok()?;
            container.slots.get_mut(slot_index)?.take()
        }
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => inventory_state
            .backpack_slots
            .get_mut(backpack_slot)?
            .as_mut()?
            .contained_slots
            .as_mut()?
            .get_mut(sub_slot)?
            .take(),
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
            // Per-instance properties (charges_remaining, templated spell_id,
            // future fillable fields) must match exactly for a merge. Without
            // this guard, two wands at different charge levels would silently
            // collapse into a single stack, and same-type but
            // differently-templated scrolls would clobber each other.
            if stack.properties != existing.properties {
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
            // Recursion guard: storable container into a container that
            // refuses storable containers (e.g. pouch) is rejected. The flag
            // lives on the *destination* container's definition so the rule
            // is fully YAML-driven.
            let dest_def = object_query
                .iter()
                .find(|(_, _, _, obj)| obj.object_id == container_object_id)
                .and_then(|(_, _, _, obj)| definitions.get(&obj.definition_id));
            if let Some(dest) = dest_def {
                if !dest.accepts_storable_containers
                    && is_storable_container(&stack.type_id, definitions)
                {
                    return false;
                }
            }
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
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => {
            // Recursion guard: the parent must be a pouch (storable
            // container) and pouches set `accepts_storable_containers: false`,
            // so reject any incoming storable container item.
            let Some(Some(parent)) = inventory_state.backpack_slots.get(backpack_slot) else {
                return false;
            };
            let parent_def = match definitions.get(&parent.type_id) {
                Some(d) => d,
                None => return false,
            };
            if !parent_def.accepts_storable_containers
                && is_storable_container(&stack.type_id, definitions)
            {
                return false;
            }
            let Some(parent_mut) = inventory_state
                .backpack_slots
                .get_mut(backpack_slot)
                .and_then(|slot| slot.as_mut())
            else {
                return false;
            };
            let Some(inner) = parent_mut.contained_slots.as_mut() else {
                return false;
            };
            let Some(slot) = inner.get_mut(sub_slot) else {
                return false;
            };
            place_stack_in_option_slot(slot, stack, definitions)
        }
    }
}

/// True if this type is itself a storable container item (a pouch). Used to
/// gate placement into containers that disallow nesting.
fn is_storable_container(type_id: &str, definitions: &OverworldObjectDefinitions) -> bool {
    definitions
        .get(type_id)
        .is_some_and(|d| d.storable && d.container_capacity.is_some())
}

fn is_player_destination(slot_ref: ItemSlotRef) -> bool {
    matches!(
        slot_ref,
        ItemSlotRef::Backpack(_) | ItemSlotRef::Equipment(_) | ItemSlotRef::PouchInBackpack { .. }
    )
}

fn is_player_source_slot(slot_ref: ItemSlotRef) -> bool {
    matches!(
        slot_ref,
        ItemSlotRef::Backpack(_) | ItemSlotRef::Equipment(_) | ItemSlotRef::PouchInBackpack { .. }
    )
}

/// Whether *adding* `stack` to the player's inventory would exceed the hard
/// carry cap. Caller must guarantee the stack is not currently counted by
/// `inventory_state.total_weight()` (e.g. just removed via `take_*`).
fn would_overload_carry(
    inventory_state: &InventoryState,
    stack: &InventoryStack,
    max_carry: &MaxCarryWeight,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    let current = inventory_state.total_weight(definitions);
    let added = stack_weight(stack, definitions);
    current + added > max_carry.hard_cap
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
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => {
            if let Some(parent) = inventory_state
                .backpack_slots
                .get_mut(backpack_slot)
                .and_then(|slot| slot.as_mut())
            {
                if let Some(inner) = parent.contained_slots.as_mut() {
                    if let Some(slot) = inner.get_mut(sub_slot) {
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
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => {
            if let Some(parent) = inventory_state
                .backpack_slots
                .get_mut(backpack_slot)
                .and_then(|slot| slot.as_mut())
            {
                if let Some(inner) = parent.contained_slots.as_mut() {
                    if let Some(slot) = inner.get_mut(sub_slot) {
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
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => {
            if let Some(parent) = inventory_state
                .backpack_slots
                .get_mut(backpack_slot)
                .and_then(|slot| slot.as_mut())
            {
                if let Some(inner) = parent.contained_slots.as_mut() {
                    if let Some(slot) = inner.get_mut(sub_slot) {
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
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => {
            if let Some(parent) = inventory_state
                .backpack_slots
                .get_mut(backpack_slot)
                .and_then(|slot| slot.as_mut())
            {
                if let Some(inner) = parent.contained_slots.as_mut() {
                    if let Some(Some(stack)) = inner.get_mut(sub_slot) {
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

/// Outcome of a `consume_or_decrement_charge` call. The call sites use this to
/// drive chat-line wording — they don't otherwise need to know which branch ran.
#[derive(Clone, Copy, Debug)]
enum ChargeOutcome {
    /// Item was destroyed (legacy single-use OR last charge spent).
    Consumed,
    /// Item survived; this many charges remain.
    Decremented(u32),
    /// `infinite_uses` — item is never consumed, no charge state.
    Unlimited,
}

/// Set `properties["charges_remaining"]` on whatever the item reference points
/// at. Handles all four `ItemSlotRef` variants plus `WorldObject`.
fn write_charges_at(
    item_reference: ItemReference,
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    object_registry: &mut ObjectRegistry,
    new_charges: u32,
) {
    let value = new_charges.to_string();
    match item_reference {
        ItemReference::WorldObject(object_id) => {
            if let Some(props) = object_registry.properties_mut(object_id) {
                props.insert(crate::player::components::CHARGES_KEY.to_string(), value);
            }
        }
        ItemReference::Slot(slot_ref) => match slot_ref {
            ItemSlotRef::Backpack(slot_index) => {
                if let Some(Some(stack)) = inventory_state.backpack_slots.get_mut(slot_index) {
                    stack.set_charges_remaining(new_charges);
                }
            }
            ItemSlotRef::Equipment(slot) => {
                for (eq_slot, item) in inventory_state.equipment_slots.iter_mut() {
                    if *eq_slot == slot {
                        if let Some(item) = item.as_mut() {
                            item.properties.insert(
                                crate::player::components::CHARGES_KEY.to_string(),
                                value.clone(),
                            );
                        }
                        break;
                    }
                }
            }
            ItemSlotRef::Container {
                object_id,
                slot_index,
            } => {
                if let Some(entity) = find_container_entity(object_id, object_query) {
                    if let Ok(mut container) = container_query.get_mut(entity) {
                        if let Some(Some(stack)) = container.slots.get_mut(slot_index) {
                            stack.set_charges_remaining(new_charges);
                        }
                    }
                }
            }
            ItemSlotRef::PouchInBackpack {
                backpack_slot,
                sub_slot,
            } => {
                if let Some(parent) = inventory_state
                    .backpack_slots
                    .get_mut(backpack_slot)
                    .and_then(|slot| slot.as_mut())
                {
                    if let Some(inner) = parent.contained_slots.as_mut() {
                        if let Some(Some(stack)) = inner.get_mut(sub_slot) {
                            stack.set_charges_remaining(new_charges);
                        }
                    }
                }
            }
        },
    }
}

/// Apply one "use" to an item with potential charge accounting. Returns:
/// - `Unlimited` for `infinite_uses` items (no state change, no consumption).
/// - `Decremented(n)` when the item carried `max_charges`, had > 1 charge, and
///   was written back with `charges_remaining = n`.
/// - `Consumed` when the item was destroyed (last charge spent OR legacy single
///   consume on items without `max_charges`).
///
/// Mana / eligibility checks must happen BEFORE this call so a failed cast
/// never burns a charge.
#[allow(clippy::too_many_arguments)]
fn consume_or_decrement_charge(
    item_reference: ItemReference,
    inventory_state: &mut InventoryState,
    container_query: &mut Query<&mut Container>,
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    object_registry: &mut ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    commands: &mut Commands,
) -> ChargeOutcome {
    let view = item_reference_view(
        item_reference,
        inventory_state,
        container_query,
        object_query,
        object_registry,
    );
    let Some(view) = view else {
        consume_item_reference(
            item_reference,
            inventory_state,
            container_query,
            object_query,
            commands,
        );
        return ChargeOutcome::Consumed;
    };
    let Some(definition) = definitions.get(&view.type_id) else {
        consume_item_reference(
            item_reference,
            inventory_state,
            container_query,
            object_query,
            commands,
        );
        return ChargeOutcome::Consumed;
    };

    if definition.infinite_uses {
        return ChargeOutcome::Unlimited;
    }

    if let Some(max_charges) = definition.max_charges {
        // Legacy stacks (pre-`max_charges`) may have no `charges_remaining`
        // key — treat that as a fully-charged item so existing items don't
        // become single-use after the patch.
        let current = view
            .properties
            .get(crate::player::components::CHARGES_KEY)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(max_charges);
        if current > 1 {
            let remaining = current - 1;
            write_charges_at(
                item_reference,
                inventory_state,
                container_query,
                object_query,
                object_registry,
                remaining,
            );
            return ChargeOutcome::Decremented(remaining);
        }
        // Either 0 or 1 charge left → destroy.
    }

    consume_item_reference(
        item_reference,
        inventory_state,
        container_query,
        object_query,
        commands,
    );
    ChargeOutcome::Consumed
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
    let mut text = if description.is_empty() {
        format!("Just a {}.", display_name.to_lowercase())
    } else {
        description.to_owned()
    };

    if definition.equipment_slot == Some(EquipmentSlot::Weapon) {
        if let Some(damage) = &definition.damage {
            text.push_str(&format!("\nDamage: {damage}"));
        }
        if let Some(profile) = &definition.attack_profile {
            match profile.kind {
                AttackProfileKindDef::Melee => text.push_str("\nAttack: melee"),
                AttackProfileKindDef::Ranged => {
                    let range = definition.base_range_tiles.unwrap_or(4);
                    text.push_str(&format!("\nAttack: ranged ({range} tiles)"));
                }
            }
        }
    }
    if definition.armor > 0 {
        text.push_str(&format!("\nArmor: {}", definition.armor));
    }
    if definition.block > 0 {
        text.push_str(&format!("\nBlock: {}", definition.block));
    }
    // Casting items get one structured line covering what they cast, mana
    // cost, and remaining uses. Reads naturally as
    //   "Casts Spark Bolt for 12 MP (27/30 uses left)"
    // or, for infinite-use casters,
    //   "Casts Light for 4 MP (∞)"
    let resolved_spell = ObjectRegistry::resolved_spell_id_for_type(
        type_id,
        Some(properties),
        definitions,
        spell_definitions,
    )
    .and_then(|id| spell_definitions.get(&id).cloned());
    let charge_suffix: Option<String> = if definition.infinite_uses {
        Some("(∞)".to_owned())
    } else if let Some(max_charges) = definition.max_charges {
        let remaining = properties
            .get(crate::player::components::CHARGES_KEY)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(max_charges);
        Some(format!("({remaining}/{max_charges} uses left)"))
    } else {
        None
    };
    if let Some(spell) = resolved_spell {
        let mana_part = if spell.mana_cost.fract() == 0.0 {
            format!("{} MP", spell.mana_cost as u32)
        } else {
            format!("{:.1} MP", spell.mana_cost)
        };
        let line = match &charge_suffix {
            Some(suffix) => format!("\nCasts {} for {} {}", spell.name, mana_part, suffix),
            None => format!("\nCasts {} for {}", spell.name, mana_part),
        };
        text.push_str(&line);
    } else if let Some(suffix) = charge_suffix {
        // No spell attached (e.g. a charged consumable that only restores
        // health), but the uses line is still useful.
        text.push_str(&format!("\nUses: {}", suffix.trim_matches(|c| c == '(' || c == ')')));
    }
    Some(text)
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

fn charge_narrator_line(
    spell_name: &str,
    type_id: &str,
    definitions: &OverworldObjectDefinitions,
    outcome: ChargeOutcome,
) -> String {
    match outcome {
        ChargeOutcome::Unlimited => format!("Cast {}.", spell_name),
        ChargeOutcome::Decremented(remaining) => {
            format!("Cast {}. ({} charges remaining)", spell_name, remaining)
        }
        ChargeOutcome::Consumed => {
            let was_charged = definitions
                .get(type_id)
                .is_some_and(|d| d.max_charges.is_some());
            if was_charged {
                format!("Cast {}. The item is spent.", spell_name)
            } else {
                format!("Cast {}.", spell_name)
            }
        }
    }
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

/// Apply self-buff + clears entries from a spell to the caster's
/// `MagicEffects`. `buffs_target` is applied separately by the targeted-cast
/// handler so the target NPC can be looked up and lazily granted the
/// component.
fn apply_spell_self_effects(
    spell: &SpellDefinition,
    caster_effects: &mut crate::magic::effects::MagicEffects,
) {
    for spec in &spell.effects.buffs_self {
        caster_effects.apply(*spec);
    }
    for kind in &spell.effects.clears_self {
        caster_effects.clear(*kind);
    }
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
/// (`z != 0`) require either:
///   - a flat walkable object AT the target tile (planks, stair landings —
///     `walkable_surface: true`, `display_height == 0`), or
///   - a tall walkable object on the tile BELOW the target whose top reaches
///     up to this z (barrels, chests, low rocks — `walkable_surface: true`
///     AND `display_height > 0`). This is the auto-climb surface.
/// This makes upper floors "built from positive-space tiles" rather than
/// infinite planes, so players can't walk past the edge of an authored
/// building unless they fall.
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

    let flat_walkable_here = object_query
        .iter()
        .filter(|(_, resident, tile, _)| resident.space_id == space_id && **tile == target)
        .any(|(_, _, _, object)| {
            definitions
                .get(&object.definition_id)
                .is_some_and(|def| def.render.walkable_surface && def.render.display_height == 0.0)
        });
    if flat_walkable_here {
        return true;
    }

    let climbable_below = TilePosition::new(target.x, target.y, target.z - 1);
    object_query
        .iter()
        .filter(|(_, resident, tile, _)| resident.space_id == space_id && **tile == climbable_below)
        .any(|(_, _, _, object)| {
            definitions
                .get(&object.definition_id)
                .is_some_and(|def| def.render.walkable_surface && def.render.display_height > 0.0)
        })
}

/// Tibia-style step resolver: when the player tries to step onto
/// `(target_xy, current_z)`, work out where they actually land.
///
/// - If that tile is walkable as-is, use it.
/// - If it is blocked by a collider AND the tile one floor above is walkable
///   (some object there has `walkable_surface: true`), auto-climb +1 z.
/// - If the tile is unsupported (no collider but no walkable surface either,
///   i.e. you walked off a plank) AND `current_z > 0` AND the tile one floor
///   below is walkable, drop -1 z.
/// - Otherwise return `None`: the move is blocked.
fn resolve_step_with_climb(
    target_xy: (i32, i32),
    current_z: i32,
    space_id: crate::world::components::SpaceId,
    collider_positions: &[TilePosition],
    object_query: &Query<
        (Entity, &SpaceResident, &TilePosition, &OverworldObject),
        Without<Player>,
    >,
    definitions: &OverworldObjectDefinitions,
) -> Option<TilePosition> {
    let (x, y) = target_xy;
    let here = TilePosition::new(x, y, current_z);
    if is_walkable_tile(
        here,
        space_id,
        collider_positions,
        object_query,
        definitions,
    ) {
        return Some(here);
    }

    let blocked_by_collider = collider_positions.iter().any(|p| *p == here);

    if blocked_by_collider {
        // Climb-up rules (Tibia-style stairs of stacked steps):
        //   - The tile being walked INTO must have a `walkable_surface` object
        //     with `display_height > 0` — i.e. an actual step / barrel / chest
        //     whose top we can stand on. Walls (no walkable_surface) reject.
        //   - The target z+1 must be open: no collider, AND no flat walkable
        //     (plank/ground) directly above. A flat walkable above is a
        //     ceiling — you'd bonk your head, so the climb is refused.
        //
        // This lets authors carve a proper staircase by placing steps on the
        // lower floor and *omitting* the plank tiles directly above each
        // step. Players climb up through the holes; they fall back down
        // through the same holes when walking off the rooftop.
        let above = TilePosition::new(x, y, current_z + 1);
        let blocked_above = collider_positions.iter().any(|p| *p == above);
        let ceiling_above = object_query
            .iter()
            .filter(|(_, resident, tile, _)| resident.space_id == space_id && **tile == above)
            .any(|(_, _, _, object)| {
                definitions.get(&object.definition_id).is_some_and(|def| {
                    def.render.walkable_surface && def.render.display_height == 0.0
                })
            });
        let step_below = object_query
            .iter()
            .filter(|(_, resident, tile, _)| resident.space_id == space_id && **tile == here)
            .any(|(_, _, _, object)| {
                definitions.get(&object.definition_id).is_some_and(|def| {
                    def.render.walkable_surface && def.render.display_height > 0.0
                })
            });
        if step_below && !blocked_above && !ceiling_above {
            return Some(above);
        }
        return None;
    }

    // Not blocked by a collider AND not walkable → unsupported. Drop down if
    // there's solid ground one z below.
    if current_z > 0 {
        let below = TilePosition::new(x, y, current_z - 1);
        if is_walkable_tile(
            below,
            space_id,
            collider_positions,
            object_query,
            definitions,
        ) {
            return Some(below);
        }
    }

    None
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
        BaseStats, ChatLog, DefenseStats, DerivedStats, Inventory, MovementCooldown, Player,
        PlayerId, PlayerIdentity, VitalStats, WeaponDamage,
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
                PlayerIdentity::new(PlayerId(player_id)),
                Inventory::default(),
                ChatLog::default(),
                base_stats,
                derived_stats,
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
                crate::magic::effects::MagicEffects::default(),
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
            Some(InventoryStack::item(
                "apple".to_owned(),
                ObjectProperties::new(),
                1,
            ))
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
    fn upper_floor_walk_requires_walkable_surface_or_drops_down() {
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

        // Walk east into "empty air" on floor 1 — Tibia-style, the player
        // drops to the ground floor (z=0) underneath rather than being
        // blocked.
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
            TilePosition::new(11, 10, 0),
            "player should drop off the plank to the ground floor"
        );
    }

    #[test]
    fn auto_climb_steps_player_up_onto_walkable_top() {
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 10, 10);

        // A barrel directly east of the player. Barrel is colliding and has
        // walkable_surface (top is walkable). Walking east should snap the
        // player to (11, 10, 1) — atop the barrel.
        let barrel_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("barrel");
        spawn_world_object(&mut app, "barrel", barrel_id, TilePosition::ground(11, 10));

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
            TilePosition::new(11, 10, 1),
            "player should auto-climb onto the barrel"
        );
    }

    #[test]
    fn auto_climb_blocked_when_ceiling_above() {
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 10, 10);

        // Barrel east of the player → would normally auto-climb. But there's
        // a floor plank directly above the barrel (at z+1) acting as a
        // ceiling, so the climb must be refused — the player would bonk.
        let barrel_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("barrel");
        spawn_world_object(&mut app, "barrel", barrel_id, TilePosition::ground(11, 10));
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
        assert_eq!(
            tile,
            TilePosition::ground(10, 10),
            "ceiling above the barrel should block the climb"
        );
    }

    #[test]
    fn auto_climb_blocked_when_no_walkable_top() {
        let mut app = setup_server_app();
        let player = spawn_player(&mut app, 1, 10, 10);

        // A wall directly east. Walls collide and have NO walkable_surface,
        // so the move should be blocked outright (no climb, no drop).
        let wall_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("wall");
        spawn_world_object(&mut app, "wall", wall_id, TilePosition::ground(11, 10));

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
            TilePosition::ground(10, 10),
            "player should be blocked by a non-climbable wall"
        );
    }

    /// `place_stack_in_option_slot` must refuse to merge two stacks of the same
    /// type if their per-instance `properties` differ. Without this guard, two
    /// wands with different `charges_remaining` would collapse into one slot.
    #[test]
    fn stack_merge_refuses_when_properties_differ() {
        use crate::player::components::{InventoryStack, CHARGES_KEY};
        use crate::world::map_layout::ObjectProperties;
        use crate::world::object_definitions::OverworldObjectDefinitions;

        // Use a real, normally-stackable consumable. Apples have
        // max_stack_size 100 via the consumable base, so the guard is the
        // only thing that can prevent the merge.
        let definitions = OverworldObjectDefinitions::load_from_disk();
        assert!(
            definitions
                .get("apple")
                .is_some_and(|d| d.max_stack_size > 1),
            "expected apple to be a stackable consumable for this test"
        );

        let mut existing_props = ObjectProperties::new();
        existing_props.insert("imaginary_marker".to_owned(), "left".to_owned());
        let mut slot: Option<InventoryStack> =
            Some(InventoryStack::item("apple", existing_props, 1));

        let mut incoming_props = ObjectProperties::new();
        incoming_props.insert("imaginary_marker".to_owned(), "right".to_owned());
        let incoming = InventoryStack::item("apple", incoming_props, 1);

        let merged = place_stack_in_option_slot(&mut slot, incoming, &definitions);
        assert!(
            !merged,
            "place_stack_in_option_slot must refuse to merge stacks whose properties differ"
        );
        let existing = slot.as_ref().expect("slot still has the original stack");
        assert_eq!(
            existing.quantity, 1,
            "original stack quantity must not change on a refused merge"
        );
        assert_eq!(
            existing
                .properties
                .get("imaginary_marker")
                .map(String::as_str),
            Some("left"),
            "original property must not be overwritten"
        );
        // And the inverse: same properties → merge succeeds.
        let mut shared_props = ObjectProperties::new();
        shared_props.insert("imaginary_marker".to_owned(), "left".to_owned());
        let same = InventoryStack::item("apple", shared_props, 2);
        let merged_same = place_stack_in_option_slot(&mut slot, same, &definitions);
        assert!(
            merged_same,
            "stacks with identical properties must still merge"
        );
        assert_eq!(slot.as_ref().unwrap().quantity, 3);
        let _ = CHARGES_KEY; // keep the import alive even if charges aren't used in this test
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
