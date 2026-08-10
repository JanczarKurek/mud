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
    GameUiEvent, InventoryStackSummary, PendingGameCommands, PendingGameUiEvents,
};
use crate::magic::effects::MagicEffects;
use crate::player::components::{
    AwaitingRespawn, ChatLog, Inventory, InventoryStack, MovementCooldown, Player, PlayerIdentity,
    RegenBuffs, RegenTickers, VitalStats,
};
use crate::player::progression::{xp_for_level, Experience};
use crate::world::components::{Facing, SpaceId, SpaceResident, TilePosition};
use crate::world::loot::spawn_corpse_for_player;
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::{EquipmentSlot, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::setup::spawn_overworld_object;
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
    /// Whoever struck the killing blow, when it was attributable. Used to
    /// settle guilt: dying at the hands of a faction pays your debt to it.
    /// `None` for lava, falls, and other unattributed deaths.
    pub killer: Option<Entity>,
}

type DeathHandlerPlayerQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerIdentity,
        &'static mut Inventory,
        &'static mut RegenBuffs,
        &'static mut RegenTickers,
        &'static mut ChatLog,
        Option<&'static mut Experience>,
        Option<&'static mut MagicEffects>,
    ),
    With<Player>,
>;

/// Default per-equipment-slot drop chance applied on death (`progression.md`
/// §8 rule 3). `[tunable]`.
pub const SLOT_DROP_CHANCE_PERCENT: u32 = 10;

/// Asset id for the tombstone spawned on player death.
const TOMBSTONE_TYPE_ID: &str = "tombstone";

/// Resolve where a dead player should reappear: their saved `home_position` if
/// that space still exists (ephemeral dungeons can be torn down between
/// sessions), otherwise the center of the current overworld space. Shared by
/// the live respawn (`process_acknowledge_death_commands`) and the save path
/// (`save_entity`), which rewrites a disconnected-while-dead snapshot so the
/// player reloads at their respawn point instead of where they fell.
pub fn resolve_respawn_destination(
    home_position: Option<(SpaceId, TilePosition)>,
    space_manager: &SpaceManager,
    world_config: &WorldConfig,
) -> (SpaceId, TilePosition) {
    home_position
        .filter(|(space, _)| space_manager.get(*space).is_some())
        .unwrap_or_else(|| {
            (
                world_config.current_space_id,
                TilePosition::ground(world_config.map_width / 2, world_config.map_height / 2),
            )
        })
}

/// Drain `PendingPlayerDeaths` and resolve each one's *immediate* consequences:
/// spawn a corpse with the player's gear, apply the XP penalty, clear active
/// buffs and magic effects, mark the player `AwaitingRespawn`, and
/// **de-spatialize** them (remove `SpaceResident` + `TilePosition`) so the
/// world stops seeing the body. The player stays dead (HP 0) with the death
/// overlay up — the heal + re-spatialize at home is deferred to
/// `process_acknowledge_death_commands`, which runs when the player clicks
/// "Continue" on the death overlay.
pub fn handle_player_deaths(
    mut pending: ResMut<PendingPlayerDeaths>,
    mut commands: Commands,
    mut object_registry: ResMut<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    mut player_query: DeathHandlerPlayerQuery,
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    killer_faction_query: Query<&crate::npc::guilt::FactionMembership>,
    mut pending_guilt: ResMut<crate::npc::guilt::PendingGuiltEvents>,
) {
    let deaths = std::mem::take(&mut pending.deaths);

    for death in deaths {
        let Ok((
            identity,
            mut inventory,
            mut buffs,
            mut tickers,
            mut chat_log,
            experience,
            magic_effects,
        )) = player_query.get_mut(death.entity)
        else {
            continue;
        };

        // Dying at the hands of a faction settles your debt with it: the
        // sentence has been carried out. Only that faction forgives — being
        // executed by the Watch does nothing for your standing with the goblins.
        if let Some(factions) = death
            .killer
            .and_then(|killer| killer_faction_query.get(killer).ok())
        {
            pending_guilt.push(crate::npc::guilt::GuiltEvent::Clear {
                player: identity.id,
                factions: factions.mask,
            });
        }

        let dropped = drain_inventory_with_drop_chance(&mut inventory, SLOT_DROP_CHANCE_PERCENT);
        let items_summary = summarize_dropped(&dropped, &definitions);
        spawn_corpse_for_player(
            &mut commands,
            &definitions,
            &mut object_registry,
            death.space_id,
            death.tile_position,
            dropped,
        );

        // Drop a tombstone alongside the corpse so the world remembers who
        // fell here. Auto-engraved + read-only; persists in the world
        // snapshot via the standard runtime-object path.
        if definitions.get(TOMBSTONE_TYPE_ID).is_some() {
            let mut tombstone_props = ObjectProperties::new();
            tombstone_props.insert("title".to_owned(), format!("Tombstone of {}", death.name));
            tombstone_props.insert(
                "text".to_owned(),
                format!("Here lies {}, fallen in battle.", death.name),
            );
            let tombstone_id = object_registry
                .allocate_runtime_id_with_properties(TOMBSTONE_TYPE_ID.to_owned(), tombstone_props);
            spawn_overworld_object(
                &mut commands,
                &definitions,
                &object_registry,
                tombstone_id,
                TOMBSTONE_TYPE_ID,
                None,
                death.space_id,
                death.tile_position,
                None,
            );
        }

        // XP-zero rule: lose all progress *into* the current level, but never
        // de-level. progression.md §8 rule 1.
        let xp_lost = if let Some(mut experience) = experience {
            let baseline = xp_for_level(experience.level);
            let lost = experience.current_xp.saturating_sub(baseline);
            experience.current_xp = baseline;
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

        // Clear active food buff and reset accumulators so regen restarts
        // cleanly post-respawn. (Regen is gated off while HP is 0, so nothing
        // ticks until the player acknowledges and is healed.)
        buffs.multiplier = 1.0;
        buffs.remaining_seconds = 0.0;
        tickers.health_remaining = 0.0;
        tickers.mana_remaining = 0.0;

        // Wipe magical buffs/debuffs so they don't survive death. The projection
        // emits a cleared `PlayerEffectsChanged` next tick, despawning client VFX
        // and shrinking any Glimmer light.
        if let Some(mut effects) = magic_effects {
            effects.active.clear();
            effects.kind_tick_accumulators.clear();
        }

        // Mark the player as awaiting respawn and de-spatialize them: with
        // `SpaceResident`/`TilePosition` gone, NPC detection, combat, AoE, and
        // the remote-player projection all miss the body by construction — no
        // per-system dead checks needed. The session components (vitals, chat,
        // inventory, XP) stay so the death overlay keeps replicating.
        // `process_acknowledge_death_commands` re-inserts the spatial pair at
        // the respawn point when the player clicks "Continue". Dropping the
        // combat target keeps a player killer from auto-attacking the corpse.
        commands
            .entity(death.entity)
            .insert(AwaitingRespawn {
                death_space: death.space_id,
            })
            .remove::<(
                crate::combat::components::CombatTarget,
                SpaceResident,
                TilePosition,
            )>();

        chat_log.push_narrator(format!("{} fell in battle.", death.name));
    }
}

/// Drain `GameCommand::AcknowledgeDeath` from the pending command queue and
/// finalize respawn for the acking player: heal HP/MP to full and re-insert
/// the spatial components (`SpaceResident` + `TilePosition`, removed at death)
/// at their home tile (or map center as fallback), then remove
/// `AwaitingRespawn`.
///
/// Runs in `CommandIntercept` (before `process_game_commands`) so a dead
/// player's *other* commands are still blocked when the main processor runs.
/// The query is filtered `With<AwaitingRespawn>`, so an `AcknowledgeDeath` from
/// a player who isn't dead matches nobody and is a silent no-op. `cmd.player_id`
/// is `Option`: `None` falls back to the first matching player (embedded mode
/// has exactly one).
pub fn process_acknowledge_death_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut player_query: Query<
        (
            Entity,
            &PlayerIdentity,
            &mut VitalStats,
            &mut MovementCooldown,
            &mut ChatLog,
            Option<&mut Facing>,
        ),
        (With<Player>, With<AwaitingRespawn>),
    >,
    space_manager: Res<SpaceManager>,
    world_config: Res<WorldConfig>,
    mut commands: Commands,
) {
    for (player_id, ()) in pending_commands.drain_matching(|command| match command {
        GameCommand::AcknowledgeDeath => Ok(()),
        other => Err(other),
    }) {
        for (entity, identity, mut vitals, mut movement, mut chat_log, facing) in
            player_query.iter_mut()
        {
            let matches = match player_id {
                Some(id) => identity.id == id,
                None => true,
            };
            if !matches {
                continue;
            }

            // Heal to full (moved here from handle_player_deaths).
            vitals.health = vitals.max_health.max(1.0);
            vitals.mana = vitals.max_mana.max(0.0);

            let (target_space, target_tile) =
                resolve_respawn_destination(identity.home_position, &space_manager, &world_config);

            movement.remaining_seconds = 0.0;

            if let Some(mut facing) = facing {
                facing.0 = crate::world::direction::Direction::default();
            }

            chat_log.push_narrator("You are taken to safer ground.");

            // Re-spatialize at the respawn point; the projection streams the
            // new area to the client the same way a portal teleport would.
            commands
                .entity(entity)
                .insert((
                    SpaceResident {
                        space_id: target_space,
                    },
                    target_tile,
                ))
                .remove::<AwaitingRespawn>();
            break;
        }
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
        dropped.push(InventoryStack::item(
            item.type_id,
            item.properties,
            quantity,
        ));
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
    for (player_id, ()) in pending_commands.drain_matching(|command| match command {
        GameCommand::SetHome => Ok(()),
        other => Err(other),
    }) {
        let mut applied = false;
        for (mut identity, space_resident, tile_position, mut chat_log) in player_query.iter_mut() {
            let matches = match player_id {
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
                player_id
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::{PlayerId, VitalStats};

    fn death_test_app() -> App {
        let mut app = App::new();
        app.init_resource::<PendingPlayerDeaths>()
            .init_resource::<PendingGameCommands>()
            .init_resource::<PendingGameUiEvents>()
            .init_resource::<ObjectRegistry>()
            // Death settles the dead player's guilt with the killer's factions.
            .init_resource::<crate::npc::guilt::PendingGuiltEvents>()
            // Real definitions from disk: the corpse + tombstone spawn in
            // handle_player_deaths looks up 'generic_corpse'/'tombstone'.
            .insert_resource(OverworldObjectDefinitions::load_from_disk())
            .init_resource::<SpaceManager>()
            .insert_resource(WorldConfig {
                current_space_id: SpaceId(1),
                map_width: 32,
                map_height: 24,
                tile_size: 48.0,
                fill_floor_type: "grass".to_owned(),
            });
        app.add_systems(
            Update,
            (process_acknowledge_death_commands, handle_player_deaths).chain(),
        );
        app
    }

    fn spawn_player(app: &mut App, id: u64, space: SpaceId, tile: TilePosition) -> Entity {
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(id)),
                VitalStats::full(20.0, 10.0),
                Inventory::default(),
                crate::player::components::RegenBuffs::default(),
                crate::player::components::RegenTickers::default(),
                ChatLog::default(),
                MovementCooldown::default(),
                SpaceResident { space_id: space },
                tile,
            ))
            .id()
    }

    #[test]
    fn death_despatializes_and_ack_respatializes_at_respawn_point() {
        let mut app = death_test_app();
        let death_space = SpaceId(7);
        let death_tile = TilePosition::ground(5, 5);
        let player = spawn_player(&mut app, 1, death_space, death_tile);
        app.world_mut()
            .get_mut::<VitalStats>(player)
            .unwrap()
            .health = 0.0;

        app.world_mut()
            .resource_mut::<PendingPlayerDeaths>()
            .deaths
            .push(PendingPlayerDeath {
                entity: player,
                space_id: death_space,
                tile_position: death_tile,
                name: "Tester".to_owned(),
                killer: None,
            });
        app.update();

        // Dead: de-spatialized, marked, still at HP 0.
        assert!(
            app.world().get::<SpaceResident>(player).is_none(),
            "death must remove SpaceResident"
        );
        assert!(
            app.world().get::<TilePosition>(player).is_none(),
            "death must remove TilePosition"
        );
        let awaiting = app
            .world()
            .get::<AwaitingRespawn>(player)
            .expect("death must mark AwaitingRespawn");
        assert_eq!(awaiting.death_space, death_space);
        assert_eq!(app.world().get::<VitalStats>(player).unwrap().health, 0.0);

        // Ack: healed, re-spatialized at the respawn point (no home set →
        // map center of the configured overworld), marker gone.
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(PlayerId(1), GameCommand::AcknowledgeDeath);
        app.update();

        let vitals = app.world().get::<VitalStats>(player).unwrap();
        assert_eq!(vitals.health, vitals.max_health);
        assert!(app.world().get::<AwaitingRespawn>(player).is_none());
        assert_eq!(
            app.world().get::<SpaceResident>(player).map(|r| r.space_id),
            Some(SpaceId(1)),
            "respawn must re-insert SpaceResident at the fallback space"
        );
        assert_eq!(
            app.world().get::<TilePosition>(player).copied(),
            Some(TilePosition::ground(16, 12)),
            "respawn must re-insert TilePosition at map center"
        );
    }
}
