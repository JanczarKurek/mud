use bevy::prelude::*;

use crate::combat::components::{AttackKind, AttackProfile, CombatLeash, CombatTarget};
use crate::combat::damage_expr::DamageExpr;
use crate::combat::resources::BattleTurnTimer;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents, VfxAnchor};
use crate::magic::resources::SpellDefinitions;
use crate::npc::components::Npc;
use crate::player::components::{
    AmmoConsumption, AttributeSet, ChatLog, DerivedStats, Inventory, Player, PlayerId,
    PlayerIdentity, VitalStats, WeaponDamage,
};
use crate::player::lifecycle::{PendingPlayerDeath, PendingPlayerDeaths};
use crate::player::progression::{xp_grant_for_kill, Experience, PendingXpGrant, PendingXpGrants};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::loot::spawn_corpse_for_npc;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;

#[derive(Clone)]
struct CombatantSnapshot {
    entity: Entity,
    target: Option<Entity>,
    attack_profile: AttackProfile,
    space_id: crate::world::components::SpaceId,
    position: TilePosition,
    object_id: u64,
    name: String,
    definition_id: String,
    attributes: AttributeSet,
    damage_expr: DamageExpr,
    health: f32,
    is_player: bool,
    player_id: Option<u64>,
    ranged_projectile_sprite: Option<String>,
    level: Option<u32>,
}

pub fn clear_invalid_combat_targets(
    mut commands: Commands,
    target_query: Query<(
        Entity,
        &CombatTarget,
        &SpaceResident,
        &TilePosition,
        Option<&CombatLeash>,
    )>,
    entity_query: Query<(&SpaceResident, &TilePosition)>,
) {
    for (entity, combat_target, attacker_space, attacker_position, leash) in &target_query {
        if combat_target.entity == entity {
            commands.entity(entity).remove::<CombatTarget>();
            continue;
        }

        let Ok((target_space, target_position)) = entity_query.get(combat_target.entity) else {
            commands.entity(entity).remove::<CombatTarget>();
            continue;
        };

        if attacker_space.space_id != target_space.space_id {
            commands.entity(entity).remove::<CombatTarget>();
            continue;
        }

        if let Some(leash) = leash {
            let distance = chebyshev_distance(attacker_position, target_position);
            if distance > leash.max_distance_tiles {
                commands.entity(entity).remove::<CombatTarget>();
            }
        }
    }
}

pub fn resolve_battle_turn(
    time: Res<Time>,
    mut battle_turn_timer: ResMut<BattleTurnTimer>,
    mut combat_queries: ParamSet<(
        Query<(
            Entity,
            Option<&CombatTarget>,
            &AttackProfile,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &DerivedStats,
            &VitalStats,
            Option<&WeaponDamage>,
            Option<&PlayerIdentity>,
            Option<&Inventory>,
            Option<&Experience>,
        )>,
        Query<(
            &mut VitalStats,
            Option<&Player>,
            Option<&Npc>,
            Option<&mut crate::magic::effects::MagicEffects>,
        )>,
        Query<&mut Inventory, With<Player>>,
    )>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    spell_definitions: Res<SpellDefinitions>,
    mut chat_log_query: Query<&mut ChatLog, With<Player>>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut quest_events: ResMut<crate::quest::events::PendingQuestEvents>,
    mut pending_player_deaths: ResMut<PendingPlayerDeaths>,
    mut pending_xp_grants: ResMut<PendingXpGrants>,
    mut commands: Commands,
) {
    battle_turn_timer.remaining_seconds -= time.delta_secs();
    if battle_turn_timer.remaining_seconds > 0.0 {
        return;
    }

    while battle_turn_timer.remaining_seconds <= 0.0 {
        battle_turn_timer.remaining_seconds += battle_turn_timer.interval_seconds;
    }

    let combatants: Vec<CombatantSnapshot> = combat_queries
        .p0()
        .iter()
        .map(
            |(
                entity,
                combat_target,
                attack_profile,
                space_resident,
                position,
                overworld_object,
                derived_stats,
                vital_stats,
                weapon_damage,
                player_identity,
                inventory,
                experience,
            )| {
                let damage_expr = weapon_damage
                    .map(|wd| wd.0.clone())
                    .unwrap_or_else(DamageExpr::melee_default);
                let is_player = player_identity.is_some();
                let player_id = player_identity.map(|identity| identity.id.0);
                let ammo_type_id = inventory.and_then(|inv| {
                    inv.equipment_item(crate::world::object_definitions::EquipmentSlot::Ammo)
                        .map(|item| item.type_id.clone())
                });
                let ranged_projectile_sprite = ranged_sprite_id(
                    is_player,
                    ammo_type_id.as_deref(),
                    &overworld_object.definition_id,
                    &definitions,
                );
                CombatantSnapshot {
                    entity,
                    target: combat_target.map(|target| target.entity),
                    attack_profile: *attack_profile,
                    space_id: space_resident.space_id,
                    position: *position,
                    object_id: overworld_object.object_id,
                    name: combatant_name(
                        overworld_object,
                        &object_registry,
                        &definitions,
                        &spell_definitions,
                    ),
                    definition_id: overworld_object.definition_id.clone(),
                    attributes: derived_stats.attributes,
                    damage_expr,
                    health: vital_stats.health,
                    is_player,
                    player_id,
                    ranged_projectile_sprite,
                    level: experience.map(|e| e.level),
                }
            },
        )
        .collect();

    for attacker in &combatants {
        let Some(target_entity) = attacker.target else {
            continue;
        };

        if target_entity == attacker.entity || attacker.health <= 0.0 {
            continue;
        }

        let Some(target) = combatants
            .iter()
            .find(|combatant| combatant.entity == target_entity)
        else {
            continue;
        };

        if target.health <= 0.0 || target.space_id != attacker.space_id {
            continue;
        }

        if !is_target_in_range(
            attacker.attack_profile.kind,
            &attacker.position,
            &target.position,
        ) {
            continue;
        }

        let is_ranged = matches!(attacker.attack_profile.kind, AttackKind::Ranged { .. });
        if is_ranged && attacker.is_player {
            let mut inventory_query = combat_queries.p2();
            let Ok(mut inventory) = inventory_query.get_mut(attacker.entity) else {
                continue;
            };
            match inventory.consume_one_ammo() {
                AmmoConsumption::None => {
                    broadcast_chat_line(
                        &mut chat_log_query,
                        format!("[{} is out of ammo]", attacker.name),
                    );
                    continue;
                }
                AmmoConsumption::Decremented | AmmoConsumption::Emptied { .. } => {}
            }
        }

        let damage = attacker.damage_expr.roll(&attacker.attributes).max(1);

        if is_ranged {
            let sprite_id = attacker
                .ranged_projectile_sprite
                .clone()
                .unwrap_or_else(|| "arrow".to_owned());
            ui_events.push_broadcast(GameUiEvent::ProjectileFired {
                from_tile: attacker.position,
                to_tile: target.position,
                sprite_definition_id: sprite_id,
            });
        }

        let mut target_query = combat_queries.p1();
        let Ok((mut target_vitals, is_player, is_npc, mut target_magic)) =
            target_query.get_mut(target_entity)
        else {
            continue;
        };

        if target_vitals.health <= 0.0 {
            continue;
        }

        target_vitals.health = (target_vitals.health - damage as f32).max(0.0);

        let hit_vfx_id = definitions
            .get(&attacker.definition_id)
            .and_then(|def| def.attack_profile.as_ref())
            .and_then(|profile| profile.hit_vfx.clone())
            .unwrap_or_else(|| "blood_splash".to_owned());
        ui_events.push_broadcast(GameUiEvent::VfxSpawn {
            definition_id: hit_vfx_id,
            anchor: VfxAnchor::follow(target.object_id),
        });

        // Damage wakes a sleeping target (and clears any pending Sleep
        // entry). NPCs keep their CombatTarget so they re-engage immediately.
        if let Some(effects) = target_magic.as_mut() {
            effects.clear(crate::magic::resources::EffectKind::Sleep);
        }
        broadcast_chat_line(
            &mut chat_log_query,
            format!(
                "[{} hit {} for {damage} damage]",
                attacker.name, target.name
            ),
        );

        if target_vitals.health > 0.0 {
            continue;
        }

        if is_npc.is_some() {
            if let Some(loot_table) = definitions
                .get(&target.definition_id)
                .and_then(|def| def.loot_table.as_ref())
            {
                spawn_corpse_for_npc(
                    &mut commands,
                    &definitions,
                    &mut object_registry,
                    loot_table,
                    target.space_id,
                    target.position,
                );
            }
            quest_events
                .events
                .push(crate::quest::events::QuestEvent::ObjectKilled {
                    type_id: target.definition_id.clone(),
                    killer_player_id: attacker.player_id,
                });

            if attacker.is_player {
                if let Some(killer_player_id) = attacker.player_id {
                    let amount = xp_grant_for_kill(target.level.unwrap_or(1));
                    pending_xp_grants.grants.push(PendingXpGrant {
                        player_id: PlayerId(killer_player_id),
                        amount,
                    });
                    broadcast_chat_line(
                        &mut chat_log_query,
                        format!("[{} gained {} XP]", attacker.name, amount),
                    );
                }
            }

            ui_events.push_broadcast(GameUiEvent::VfxSpawn {
                definition_id: "death_poof".to_owned(),
                anchor: VfxAnchor::tile(target.space_id, target.position),
            });
            commands.entity(target_entity).despawn();
            broadcast_chat_line(&mut chat_log_query, format!("[{} dies]", target.name));
            continue;
        }

        if is_player.is_some() {
            broadcast_chat_line(
                &mut chat_log_query,
                format!("[{} is defeated]", target.name),
            );
            pending_player_deaths.deaths.push(PendingPlayerDeath {
                entity: target_entity,
                space_id: target.space_id,
                tile_position: target.position,
                name: target.name.clone(),
            });
        }
    }
}

fn ranged_sprite_id(
    is_player: bool,
    ammo_type_id: Option<&str>,
    attacker_def_id: &str,
    definitions: &OverworldObjectDefinitions,
) -> Option<String> {
    if is_player {
        return ammo_type_id.map(|s| s.to_owned());
    }
    if let Some(def) = definitions.get(attacker_def_id) {
        if let Some(ammo) = &def.ammo_type {
            return Some(ammo.clone());
        }
    }
    Some("arrow".to_owned())
}

fn broadcast_chat_line(chat_log_query: &mut Query<&mut ChatLog, With<Player>>, message: String) {
    for mut chat_log in chat_log_query.iter_mut() {
        chat_log.push_line(message.clone());
    }
}

fn is_target_in_range(
    attack_kind: AttackKind,
    attacker_position: &TilePosition,
    target_position: &TilePosition,
) -> bool {
    let distance = chebyshev_distance(attacker_position, target_position);
    if distance == 0 {
        return false;
    }
    match attack_kind {
        AttackKind::Melee => distance <= 1,
        AttackKind::Ranged { range_tiles } => distance <= range_tiles,
    }
}

fn combatant_name(
    overworld_object: &OverworldObject,
    object_registry: &ObjectRegistry,
    definitions: &OverworldObjectDefinitions,
    spell_definitions: &SpellDefinitions,
) -> String {
    object_registry
        .display_name(overworld_object.object_id, definitions, spell_definitions)
        .unwrap_or_else(|| overworld_object.definition_id.clone())
}

fn chebyshev_distance(a: &TilePosition, b: &TilePosition) -> i32 {
    if a.z != b.z {
        return i32::MAX;
    }
    (a.x - b.x).abs().max((a.y - b.y).abs())
}
