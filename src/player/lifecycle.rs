//! Player death/respawn flow and home-point management.
//!
//! Death detection happens during combat (`resolve_battle_turn`), but actually
//! moving the player and dropping the corpse runs *after* combat finishes via
//! `PendingPlayerDeaths`. Doing it inside the combat loop would invalidate the
//! query iterator we're holding mid-resolution.

use bevy::prelude::*;

use crate::accounts::AccountDbHandle;
use crate::game::commands::GameCommand;
use crate::game::resources::{
    GameEvent, GameUiEvent, InventoryStackSummary, PendingGameCommands, PendingGameEvents,
    PendingGameUiEvents,
};
use crate::player::components::{
    ChatLog, DerivedStats, Inventory, InventoryStack, MovementCooldown, Player, PlayerIdentity,
    RegenBuffs, RegenTickers, VitalStats,
};
use crate::player::progression::{xp_for_level, Experience};
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
        Option<&'static mut Experience>,
    ),
    With<Player>,
>;

/// Default per-equipment-slot drop chance applied on death (`progression.md`
/// §8 rule 3). `[tunable]`.
pub const SLOT_DROP_CHANCE_PERCENT: u32 = 10;

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
    mut pending_events: ResMut<PendingGameEvents>,
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
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
            experience,
        )) = player_query.get_mut(death.entity)
        else {
            continue;
        };

        let dropped =
            drain_inventory_with_drop_chance(&mut inventory, SLOT_DROP_CHANCE_PERCENT);
        let items_summary = summarize_dropped(&dropped, &definitions);
        spawn_corpse_for_player(
            &mut commands,
            &definitions,
            &mut object_registry,
            death.space_id,
            death.tile_position,
            dropped,
        );

        // XP-zero rule: lose all progress *into* the current level, but never
        // de-level. progression.md §8 rule 1.
        let xp_lost = if let Some(mut experience) = experience {
            let baseline = xp_for_level(experience.level);
            let lost = experience.current_xp.saturating_sub(baseline);
            experience.current_xp = baseline;
            if lost > 0 {
                pending_events
                    .events
                    .push(GameEvent::ExperienceLost { amount: lost });
            }
            lost
        } else {
            0
        };

        pending_ui_events.push(
            identity.id,
            GameUiEvent::DeathSummary {
                items_dropped: items_summary,
                xp_lost,
            },
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

/// Death drain (`progression.md` §8): backpack always empties; each
/// equipped slot rolls 1..=100 independently and drops on `<=
/// slot_drop_chance_percent`. Returns the dropped stacks for corpse
/// placement.
fn drain_inventory_with_drop_chance(
    inventory: &mut Inventory,
    slot_drop_chance_percent: u32,
) -> Vec<InventoryStack> {
    let mut dropped = Vec::new();

    // Rule 2 — backpack always drops.
    for slot in inventory.backpack_slots.iter_mut() {
        if let Some(stack) = slot.take() {
            dropped.push(stack);
        }
    }

    // Rule 3 — equipment slots roll independently.
    let ammo_qty = inventory.ammo_quantity;
    let mut ammo_dropped = false;
    for (slot_index, (slot_kind, slot_item)) in inventory.equipment_slots.iter_mut().enumerate() {
        let Some(item) = slot_item.as_ref() else {
            continue;
        };
        let roll = roll_drop_d100(slot_index as u64, &item.type_id);
        if roll > slot_drop_chance_percent {
            continue;
        }
        let item = slot_item.take().expect("checked above");
        let quantity = if matches!(slot_kind, EquipmentSlot::Ammo) {
            let q = ammo_qty.max(1);
            ammo_dropped = true;
            q
        } else {
            1
        };
        dropped.push(InventoryStack::item(item.type_id, item.properties, quantity));
    }
    if ammo_dropped {
        inventory.ammo_quantity = 0;
    }

    dropped
}

/// Slot drop roll: 1..=100, mixed with slot index + item id so each slot
/// rolls independently within the same nanosecond. Mirrors the time-based
/// pattern used elsewhere in the codebase (`damage_expr::roll_die`).
fn roll_drop_d100(salt: u64, item_id: &str) -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let id_hash = item_id
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let mixed = nanos
        .wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(id_hash);
    ((mixed % 100) + 1) as u32
}

/// Build a HUD-friendly summary of the dropped stacks. Looks up the display
/// name from object definitions; falls back to `type_id` if the definition
/// is missing.
fn summarize_dropped(
    dropped: &[InventoryStack],
    definitions: &OverworldObjectDefinitions,
) -> Vec<InventoryStackSummary> {
    dropped
        .iter()
        .map(|stack| {
            let display_name = definitions
                .get(&stack.type_id)
                .map(|def| def.name.clone())
                .unwrap_or_else(|| stack.type_id.clone());
            InventoryStackSummary {
                type_id: stack.type_id.clone(),
                display_name,
                quantity: stack.quantity.max(1),
            }
        })
        .collect()
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
