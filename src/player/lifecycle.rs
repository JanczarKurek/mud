//! Player death/respawn flow and home-point management.
//!
//! Death detection happens during combat (`resolve_battle_turn`), but actually
//! moving the player and dropping the corpse runs *after* combat finishes via
//! `PendingPlayerDeaths`. Doing it inside the combat loop would invalidate the
//! query iterator we're holding mid-resolution.

use bevy::prelude::*;

use crate::accounts::AccountDbHandle;
use crate::game::commands::GameCommand;
use crate::game::resources::PendingGameCommands;
use crate::player::components::{
    ChatLog, DerivedStats, Inventory, InventoryStack, MovementCooldown, Player, PlayerIdentity,
    RegenBuffs, RegenTickers, VitalStats,
};
use crate::world::components::{Facing, SpaceId, SpaceResident, TilePosition, ViewPosition};
use crate::world::loot::spawn_corpse_for_player;
use crate::world::object_definitions::{EquipmentSlot, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::WorldConfig;

/// Queued death events. Combat detects HP→0 and pushes here; the death
/// handler drains and processes after combat finishes.
#[derive(Resource, Default)]
pub struct PendingPlayerDeaths {
    pub deaths: Vec<PendingPlayerDeath>,
}

#[derive(Clone, Debug)]
pub struct PendingPlayerDeath {
    pub entity: Entity,
    pub space_id: SpaceId,
    pub tile_position: TilePosition,
    pub name: String,
}

type DeathHandlerPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerIdentity,
        &'static mut VitalStats,
        &'static DerivedStats,
        &'static mut Inventory,
        &'static mut SpaceResident,
        &'static mut TilePosition,
        &'static mut MovementCooldown,
        &'static mut RegenBuffs,
        &'static mut RegenTickers,
        &'static mut ChatLog,
        Option<&'static mut ViewPosition>,
        Option<&'static mut Facing>,
    ),
    With<Player>,
>;

/// Drain `PendingPlayerDeaths` and resolve each one: spawn a corpse with the
/// player's gear, reset HP/MP, clear active buffs, and teleport the player to
/// their home tile (or map center as fallback).
pub fn handle_player_deaths(
    mut pending: ResMut<PendingPlayerDeaths>,
    mut commands: Commands,
    mut object_registry: ResMut<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    space_manager: Res<SpaceManager>,
    world_config: Res<WorldConfig>,
    mut player_query: DeathHandlerPlayerQuery,
) {
    let deaths = std::mem::take(&mut pending.deaths);

    for death in deaths {
        let Ok((
            identity,
            mut vitals,
            derived,
            mut inventory,
            mut space_resident,
            mut tile_position,
            mut movement,
            mut buffs,
            mut tickers,
            mut chat_log,
            view_position,
            facing,
        )) = player_query.get_mut(death.entity)
        else {
            continue;
        };

        let dropped = drain_inventory(&mut inventory);
        spawn_corpse_for_player(
            &mut commands,
            &definitions,
            &mut object_registry,
            death.space_id,
            death.tile_position,
            dropped,
        );

        vitals.health = vitals.max_health.max(1.0);
        vitals.mana = vitals.max_mana.max(0.0);

        // Restore base derived sizing in case max_health drifted (e.g. equipment
        // bonus that briefly raised the cap was the source of an off-by-one).
        let _ = derived;

        // Clear active food buff and reset accumulators so regen restarts
        // cleanly post-respawn.
        buffs.multiplier = 1.0;
        buffs.remaining_seconds = 0.0;
        tickers.health_remaining = 0.0;
        tickers.mana_remaining = 0.0;

        // Resolve respawn destination. Validate that the saved space still
        // exists (ephemeral dungeons can be torn down between sessions).
        let (target_space, target_tile) = identity
            .home_position
            .filter(|(space, _)| space_manager.get(*space).is_some())
            .unwrap_or_else(|| {
                (
                    world_config.current_space_id,
                    TilePosition::ground(
                        world_config.map_width / 2,
                        world_config.map_height / 2,
                    ),
                )
            });

        space_resident.space_id = target_space;
        *tile_position = target_tile;
        movement.remaining_seconds = 0.0;

        if let Some(mut view) = view_position {
            view.space_id = target_space;
            view.tile = target_tile;
        }
        if let Some(mut facing) = facing {
            facing.0 = crate::world::direction::Direction::default();
        }

        // Drop the combat target so the killer doesn't keep auto-attacking
        // after respawn (only relevant if the killer is a player).
        commands
            .entity(death.entity)
            .remove::<crate::combat::components::CombatTarget>();

        chat_log.push_narrator(format!(
            "{} fell in battle and is taken to safer ground.",
            death.name
        ));
    }
}

/// Pull every stack out of the inventory (backpack + equipped) and return
/// them as a flat `Vec<InventoryStack>`. Mutates `inventory` to empty.
fn drain_inventory(inventory: &mut Inventory) -> Vec<InventoryStack> {
    let mut dropped = Vec::new();

    for slot in inventory.backpack_slots.iter_mut() {
        if let Some(stack) = slot.take() {
            dropped.push(stack);
        }
    }

    let ammo_qty = inventory.ammo_quantity;
    for (slot_kind, slot_item) in inventory.equipment_slots.iter_mut() {
        if let Some(item) = slot_item.take() {
            let quantity = if matches!(slot_kind, EquipmentSlot::Ammo) {
                ammo_qty.max(1)
            } else {
                1
            };
            dropped.push(InventoryStack {
                type_id: item.type_id,
                properties: item.properties,
                quantity,
            });
        }
    }
    inventory.ammo_quantity = 0;

    dropped
}

/// Drain `GameCommand::SetHome` from the pending command queue, writing the
/// player's current `(space, tile)` into their `PlayerIdentity::home_position`.
/// Confirms via narrator. `cmd.player_id` is `Option`: `None` falls back to
/// the first Player entity (embedded mode has exactly one).
pub fn handle_set_home_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut player_query: Query<
        (
            &mut PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &mut ChatLog,
        ),
        With<Player>,
    >,
    db: Option<Res<AccountDbHandle>>,
) {
    let queued = std::mem::take(&mut pending_commands.commands);
    let mut remaining = Vec::with_capacity(queued.len());

    for cmd in queued {
        match cmd.command {
            GameCommand::SetHome => {
                let mut applied = false;
                for (mut identity, space_resident, tile_position, mut chat_log) in
                    player_query.iter_mut()
                {
                    let matches = match cmd.player_id {
                        Some(id) => identity.id == id,
                        None => true,
                    };
                    if !matches {
                        continue;
                    }
                    identity.home_position = Some((space_resident.space_id, *tile_position));
                    chat_log.push_narrator("This place is now your home — you'll respawn here.");
                    applied = true;

                    // Persist immediately so a crash before the next autosave
                    // doesn't lose the choice. Best-effort: log and continue
                    // on DB error.
                    if let Some(db_handle) = db.as_deref() {
                        if let Err(err) = persist_home(
                            db_handle,
                            identity.id.0,
                            space_resident.space_id,
                            *tile_position,
                        ) {
                            bevy::log::warn!("failed to persist home_position: {err}");
                        }
                    }
                    break;
                }
                if !applied {
                    bevy::log::debug!(
                        "SetHome command for player {:?} dropped: no matching player",
                        cmd.player_id
                    );
                }
            }
            other => remaining.push(crate::game::resources::QueuedGameCommand {
                player_id: cmd.player_id,
                command: other,
            }),
        }
    }

    pending_commands.commands = remaining;
}

fn persist_home(
    db: &AccountDbHandle,
    player_id: u64,
    space_id: SpaceId,
    tile: TilePosition,
) -> Result<(), rusqlite::Error> {
    let account_id = player_id as i64;
    let guard = db.lock();
    let Some(mut dump) = guard.load_character(account_id)? else {
        // Character row hasn't been created yet (fresh player pre-first-save) —
        // skip; the next autosave will pick up the in-memory home_position.
        return Ok(());
    };
    dump.home_position = Some((space_id, tile));
    guard.save_character(account_id, &dump)?;
    Ok(())
}
