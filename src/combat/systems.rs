use bevy::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::combat::components::{AttackKind, AttackProfile, CombatLeash, CombatTarget};
use crate::combat::resources::BattleTurnTimer;
use crate::magic::resources::SpellDefinitions;
use crate::npc::components::Npc;
use crate::player::components::{ChatLog, DerivedStats, Player, VitalStats};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;

#[derive(Clone)]
struct CombatantSnapshot {
    entity: Entity,
    target: Option<Entity>,
    attack_profile: AttackProfile,
    space_id: crate::world::components::SpaceId,
    position: TilePosition,
    name: String,
    strength: i32,
    health: f32,
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
        )>,
        Query<(&mut VitalStats, Option<&Player>, Option<&Npc>)>,
    )>,
    definitions: Res<OverworldObjectDefinitions>,
    object_registry: Res<ObjectRegistry>,
    spell_definitions: Res<SpellDefinitions>,
    mut chat_log_query: Query<&mut ChatLog, With<Player>>,
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
            )| CombatantSnapshot {
                entity,
                target: combat_target.map(|target| target.entity),
                attack_profile: *attack_profile,
                space_id: space_resident.space_id,
                position: *position,
                name: combatant_name(
                    overworld_object,
                    &object_registry,
                    &definitions,
                    &spell_definitions,
                ),
                strength: derived_stats.attributes.strength,
                health: vital_stats.health,
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

        let damage = attack_damage(attacker.attack_profile.kind, attacker.strength).max(1);

        let mut target_query = combat_queries.p1();
        let Ok((mut target_vitals, is_player, is_npc)) = target_query.get_mut(target_entity) else {
            continue;
        };

        if target_vitals.health <= 0.0 {
            continue;
        }

        target_vitals.health = (target_vitals.health - damage as f32).max(0.0);
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
            commands.entity(target_entity).despawn();
            broadcast_chat_line(&mut chat_log_query, format!("[{} dies]", target.name));
            continue;
        }

        if is_player.is_some() {
            broadcast_chat_line(
                &mut chat_log_query,
                format!("[{} is defeated]", target.name),
            );
        }
    }
}

fn broadcast_chat_line(chat_log_query: &mut Query<&mut ChatLog, With<Player>>, message: String) {
    for mut chat_log in chat_log_query.iter_mut() {
        chat_log.push_line(message.clone());
    }
}

fn attack_damage(attack_kind: AttackKind, strength: i32) -> i32 {
    match attack_kind {
        AttackKind::Melee => roll_die(6) + strength / 5,
    }
}

fn roll_die(sides: usize) -> i32 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);

    (nanos % sides + 1) as i32
}

fn is_target_in_range(
    attack_kind: AttackKind,
    attacker_position: &TilePosition,
    target_position: &TilePosition,
) -> bool {
    match attack_kind {
        AttackKind::Melee => {
            let delta_x = (attacker_position.x - target_position.x).abs();
            let delta_y = (attacker_position.y - target_position.y).abs();
            delta_x <= 1 && delta_y <= 1 && (delta_x != 0 || delta_y != 0)
        }
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
    (a.x - b.x).abs().max((a.y - b.y).abs())
}
