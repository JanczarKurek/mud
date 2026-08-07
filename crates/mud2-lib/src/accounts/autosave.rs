use bevy::app::AppExit;
use bevy::ecs::message::MessageReader;
use bevy::prelude::*;

use crate::accounts::resources::{AccountDbHandle, AutosaveConfig};
use crate::combat::components::{AttackProfile, CombatLeash};
use crate::crafting::CharacterStash;
use crate::dialog::resources::CharacterVarStores;
use crate::magic::effects::MagicEffects;
use crate::network::resources::PendingPlayerSaves;
use crate::persistence::build_player_state_dump;
use crate::player::classes::Class;
use crate::player::components::{
    AwaitingRespawn, BaseStats, ChatLog, DerivedStats, DiscoveredTiles, Inventory,
    MovementCooldown, Player, PlayerAppearance, PlayerIdentity, VitalStats,
};
use crate::player::lifecycle::resolve_respawn_destination;
use crate::player::progression::Experience;
use crate::player::skills::SkillSheet;
use crate::world::components::{Facing, SpaceResident, TilePosition};
use crate::world::resources::SpaceManager;
use crate::world::WorldConfig;

/// Tracks time since the last autosave sweep; resets when the sweep fires.
#[derive(Resource, Default)]
pub struct AutosaveTimer {
    pub elapsed_since_save: f64,
    /// Character ids captured when the interval elapsed, drained one per
    /// frame. Serializing + writing every player synchronously in a single
    /// frame caused a periodic hitch that grew with player count; staggering
    /// bounds the per-frame cost at one blocking SQLite write.
    pub pending: Vec<i64>,
}

type PlayerStateQueryData<'a> = (
    Entity,
    &'a PlayerIdentity,
    &'a SpaceResident,
    &'a TilePosition,
    &'a Inventory,
    &'a ChatLog,
    &'a BaseStats,
    &'a DerivedStats,
    &'a VitalStats,
    &'a MovementCooldown,
    &'a AttackProfile,
    &'a CombatLeash,
    Option<&'a Facing>,
    Option<&'a Experience>,
    (
        Option<&'a Class>,
        Option<&'a MagicEffects>,
        Option<&'a CharacterStash>,
        Option<&'a SkillSheet>,
        Option<&'a PlayerAppearance>,
        Option<&'a DiscoveredTiles>,
        Option<&'a AwaitingRespawn>,
    ),
);

type PlayerStateQueryFilter = With<Player>;

fn save_entity(
    db: &AccountDbHandle,
    character_id: i64,
    row: <PlayerStateQueryData<'_> as bevy::ecs::query::QueryData>::Item<'_, '_>,
    var_stores: Option<&CharacterVarStores>,
    space_manager: Option<&SpaceManager>,
    world_config: Option<&WorldConfig>,
) {
    let (
        _entity,
        identity,
        space_resident,
        tile_position,
        inventory,
        chat_log,
        base_stats,
        derived_stats,
        vital_stats,
        movement_cooldown,
        attack_profile,
        combat_leash,
        facing,
        experience,
        (class, magic_effects, stash, skill_sheet, appearance, discovered_tiles, awaiting_respawn),
    ) = row;

    let empty_effects = MagicEffects::default();
    let effects_ref = magic_effects.unwrap_or(&empty_effects);
    let empty_stash = CharacterStash::default();
    let stash_ref = stash.unwrap_or(&empty_stash);
    let empty_sheet = SkillSheet::default();
    let sheet_ref = skill_sheet.unwrap_or(&empty_sheet);
    let empty_discovered = DiscoveredTiles::default();
    let discovered_ref = discovered_tiles.unwrap_or(&empty_discovered);

    let mut dump = build_player_state_dump(
        identity,
        space_resident,
        tile_position,
        inventory,
        chat_log,
        base_stats,
        derived_stats,
        vital_stats,
        movement_cooldown,
        attack_profile,
        combat_leash,
        facing.copied().unwrap_or_default().0,
        experience.copied().unwrap_or_default(),
        class.copied().unwrap_or_default(),
        effects_ref,
        stash_ref,
        sheet_ref,
        appearance.copied().unwrap_or_default(),
        discovered_ref,
    );

    if let Some(stores) = var_stores {
        dump.yarn_vars = stores.snapshot_for(identity.id.0);
    }

    // A player who disconnects (or autosaves) while dead and awaiting respawn is
    // sitting at HP 0 on the tile where they fell — the heal + teleport-home is
    // deferred to their "Continue" click, which never came. Persist them as if
    // they had acknowledged: at their respawn point, healed. Leaving the live
    // entity untouched keeps the death overlay valid if they're still connected.
    if awaiting_respawn.is_some() {
        if let (Some(space_manager), Some(world_config)) = (space_manager, world_config) {
            let (space, tile) =
                resolve_respawn_destination(identity.home_position, space_manager, world_config);
            dump.space_id = Some(space);
            dump.tile_position = tile;
        }
        dump.vital_stats.health = dump.vital_stats.max_health.max(1.0);
        dump.vital_stats.mana = dump.vital_stats.max_mana.max(0.0);
    }

    if let Err(err) = db.lock().save_character(character_id, &dump) {
        warn!("failed to save character {character_id}: {err}");
    }
}

/// Drains `PendingPlayerSaves`, snapshots each entity into the account DB, then
/// despawns it. Runs in the `Last` schedule so the pending queue populated
/// during Update is fully processed in the same frame.
pub fn persist_disconnected_players(
    mut pending_saves: ResMut<PendingPlayerSaves>,
    db: Option<Res<AccountDbHandle>>,
    var_stores: Option<Res<CharacterVarStores>>,
    space_manager: Option<Res<SpaceManager>>,
    world_config: Option<Res<WorldConfig>>,
    player_query: Query<PlayerStateQueryData, PlayerStateQueryFilter>,
    mut commands: Commands,
) {
    if pending_saves.entries.is_empty() {
        return;
    }
    let entries = std::mem::take(&mut pending_saves.entries);
    for entry in entries {
        if let (Some(db), Ok(row)) = (db.as_deref(), player_query.get(entry.player_entity)) {
            save_entity(
                db,
                entry.character_id,
                row,
                var_stores.as_deref(),
                space_manager.as_deref(),
                world_config.as_deref(),
            );
        }
        commands.entity(entry.player_entity).despawn();
    }
}

/// Periodic autosave of every `Player` entity currently in the ECS world. The
/// character id is derived from `PlayerIdentity.id`, which is now set to
/// `PlayerId(character_id as u64)` by the auth/character-selection path.
pub fn autosave_all_players(
    time: Res<Time>,
    config: Res<AutosaveConfig>,
    mut timer: ResMut<AutosaveTimer>,
    db: Option<Res<AccountDbHandle>>,
    var_stores: Option<Res<CharacterVarStores>>,
    space_manager: Option<Res<SpaceManager>>,
    world_config: Option<Res<WorldConfig>>,
    player_query: Query<PlayerStateQueryData, PlayerStateQueryFilter>,
) {
    timer.elapsed_since_save += time.delta_secs_f64();
    if timer.elapsed_since_save >= config.interval_seconds {
        timer.elapsed_since_save = 0.0;
        timer.pending = player_query.iter().map(|row| row.1.id.0 as i64).collect();
    }

    let Some(db) = db.as_deref() else {
        return;
    };

    // Drain at most one save per frame. Ids whose player vanished since the
    // sweep started (disconnect path already saved them) are skipped without
    // consuming this frame's save slot.
    while let Some(character_id) = timer.pending.pop() {
        let Some(row) = player_query
            .iter()
            .find(|row| row.1.id.0 as i64 == character_id)
        else {
            continue;
        };
        save_entity(
            db,
            character_id,
            row,
            var_stores.as_deref(),
            space_manager.as_deref(),
            world_config.as_deref(),
        );
        break;
    }
}

/// Save every currently-spawned player on `AppExit` so a clean shutdown is
/// persisted even for players who never periodically autosaved.
pub fn save_all_players_on_app_exit(
    mut app_exit: MessageReader<AppExit>,
    db: Option<Res<AccountDbHandle>>,
    var_stores: Option<Res<CharacterVarStores>>,
    space_manager: Option<Res<SpaceManager>>,
    world_config: Option<Res<WorldConfig>>,
    player_query: Query<PlayerStateQueryData, PlayerStateQueryFilter>,
) {
    if app_exit.read().next().is_none() {
        return;
    }
    let Some(db) = db.as_deref() else {
        return;
    };
    for row in player_query.iter() {
        let character_id = row.1.id.0 as i64;
        save_entity(
            db,
            character_id,
            row,
            var_stores.as_deref(),
            space_manager.as_deref(),
            world_config.as_deref(),
        );
    }
}
