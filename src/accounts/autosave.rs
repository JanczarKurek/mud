use bevy::app::AppExit;
use bevy::ecs::message::MessageReader;
use bevy::prelude::*;

use crate::accounts::resources::{AccountDbHandle, AutosaveConfig};
use crate::combat::components::{AttackProfile, CombatLeash, CombatTarget};
use crate::dialog::resources::CharacterVarStores;
use crate::network::resources::PendingPlayerSaves;
use crate::persistence::build_player_state_dump;
use crate::player::components::{
    BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, Player, PlayerIdentity,
    VitalStats,
};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};

/// Tracks time since the last autosave sweep; resets when the sweep fires.
#[derive(Resource, Default)]
pub struct AutosaveTimer {
    pub elapsed_since_save: f64,
}

type PlayerStateQueryData<'a> = (
    Entity,
    &'a PlayerIdentity,
    &'a OverworldObject,
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
    Option<&'a CombatTarget>,
);

type PlayerStateQueryFilter = With<Player>;

fn save_entity(
    db: &AccountDbHandle,
    account_id: i64,
    row: <PlayerStateQueryData<'_> as bevy::ecs::query::QueryData>::Item<'_, '_>,
    object_lookup: &Query<&OverworldObject>,
    var_stores: Option<&CharacterVarStores>,
) {
    let (
        _entity,
        identity,
        object,
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
        combat_target,
    ) = row;

    let combat_target_object_id = combat_target
        .and_then(|target| object_lookup.get(target.entity).ok())
        .map(|object| object.object_id);

    let mut dump = build_player_state_dump(
        identity,
        object,
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
        combat_target_object_id,
    );

    if let Some(stores) = var_stores {
        dump.yarn_vars = stores.snapshot_for(identity.id.0);
    }

    if let Err(err) = db.lock().save_character(account_id, &dump) {
        warn!("failed to save character for account {account_id}: {err}");
    }
}

/// Drains `PendingPlayerSaves`, snapshots each entity into the account DB, then
/// despawns it. Runs in the `Last` schedule so the pending queue populated
/// during Update is fully processed in the same frame.
pub fn persist_disconnected_players(
    mut pending_saves: ResMut<PendingPlayerSaves>,
    db: Option<Res<AccountDbHandle>>,
    var_stores: Option<Res<CharacterVarStores>>,
    player_query: Query<PlayerStateQueryData, PlayerStateQueryFilter>,
    object_lookup: Query<&OverworldObject>,
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
                entry.account_id,
                row,
                &object_lookup,
                var_stores.as_deref(),
            );
        }
        commands.entity(entry.player_entity).despawn();
    }
}

/// Periodic autosave of every `Player` entity currently in the ECS world. The
/// account id is derived from `PlayerIdentity.id`, which the auth path sets to
/// `PlayerId(account_id as u64)`; embedded mode uses `PlayerId(0)` which maps
/// to the reserved local account.
pub fn autosave_all_players(
    time: Res<Time>,
    config: Res<AutosaveConfig>,
    mut timer: ResMut<AutosaveTimer>,
    db: Option<Res<AccountDbHandle>>,
    var_stores: Option<Res<CharacterVarStores>>,
    player_query: Query<PlayerStateQueryData, PlayerStateQueryFilter>,
    object_lookup: Query<&OverworldObject>,
) {
    timer.elapsed_since_save += time.delta_secs_f64();
    if timer.elapsed_since_save < config.interval_seconds {
        return;
    }
    timer.elapsed_since_save = 0.0;

    let Some(db) = db.as_deref() else {
        return;
    };

    for row in player_query.iter() {
        let account_id = row.1.id.0 as i64;
        save_entity(db, account_id, row, &object_lookup, var_stores.as_deref());
    }
}

/// Save every currently-spawned player on `AppExit` so a clean shutdown is
/// persisted even for players who never periodically autosaved.
pub fn save_all_players_on_app_exit(
    mut app_exit: MessageReader<AppExit>,
    db: Option<Res<AccountDbHandle>>,
    var_stores: Option<Res<CharacterVarStores>>,
    player_query: Query<PlayerStateQueryData, PlayerStateQueryFilter>,
    object_lookup: Query<&OverworldObject>,
) {
    if app_exit.read().next().is_none() {
        return;
    }
    let Some(db) = db.as_deref() else {
        return;
    };
    for row in player_query.iter() {
        let account_id = row.1.id.0 as i64;
        save_entity(db, account_id, row, &object_lookup, var_stores.as_deref());
    }
}
