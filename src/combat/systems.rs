use bevy::prelude::*;

use crate::combat::components::{AttackKind, AttackProfile, CombatLeash, CombatTarget};
use crate::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use crate::combat::damage_expr::DamageExpr;
use crate::combat::damage_type::DamageType;
use crate::combat::resources::BattleTurnTimer;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents, VfxAnchor};
use crate::magic::resources::{EffectSpec, SpellDefinitions};
use crate::player::components::{
    AmmoConsumption, AttributeSet, ChatLog, DefenseStats, DerivedStats, Inventory, Player,
    PlayerId, PlayerIdentity, VitalStats, WeaponDamage,
};
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
    object_id: u64,
    name: String,
    definition_id: String,
    attributes: AttributeSet,
    damage_expr: DamageExpr,
    damage_type: DamageType,
    health: f32,
    is_player: bool,
    player_id: Option<u64>,
    ranged_projectile_sprite: Option<String>,
    armor: i32,
    block: i32,
}

/// Pure mitigation math. `block_roll` and `armor_roll` are pre-rolled values in
/// `0..=defense_value`. Floor at 1 to preserve the no-zero-damage invariant.
fn apply_defenses(raw: i32, block_roll: i32, armor_roll: i32) -> i32 {
    (raw - block_roll - armor_roll).max(1)
}

/// Roll a uniform integer in `0..=max`. Uses the same nanosecond+salt pattern
/// as `roll_die` — see `damage_expr::roll_die`.
fn roll_defense(max: i32, salt: u64) -> i32 {
    if max <= 0 {
        return 0;
    }
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    (mixed % (max as u64 + 1)) as i32
}

/// Return `true` with probability `chance` (clamped to `[0, 1]`). Reuses the
/// nanosecond+salt jitter pattern from `roll_defense` — good enough for
/// triggers that aren't security-sensitive.
fn roll_chance(chance: f32, salt: u64) -> bool {
    let p = chance.clamp(0.0, 1.0);
    if p <= 0.0 {
        return false;
    }
    if p >= 1.0 {
        return true;
    }
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let roll = (mixed % 1_000_000) as f32 / 1_000_000.0;
    roll < p
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
            Option<&DefenseStats>,
        )>,
        Query<(
            &VitalStats,
            Option<&mut crate::magic::effects::MagicEffects>,
        )>,
        Query<&mut Inventory, With<Player>>,
    )>,
    definitions: Res<OverworldObjectDefinitions>,
    object_registry: Res<ObjectRegistry>,
    spell_definitions: Res<SpellDefinitions>,
    mut chat_log_query: Query<&mut ChatLog, With<Player>>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut pending_damage: ResMut<PendingDamageEvents>,
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
                defense_stats,
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
                    damage_type: attack_profile.damage_type,
                    health: vital_stats.health,
                    is_player,
                    player_id,
                    ranged_projectile_sprite,
                    armor: defense_stats.map(|d| d.armor).unwrap_or(0),
                    block: defense_stats.map(|d| d.block).unwrap_or(0),
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

        let raw = attacker.damage_expr.roll(&attacker.attributes).max(1);
        let block_roll = roll_defense(target.block, 0);
        let armor_roll = roll_defense(target.armor, 1);
        let damage = apply_defenses(raw, block_roll, armor_roll);

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
        let Ok((target_vitals, mut target_magic)) = target_query.get_mut(target_entity) else {
            continue;
        };

        if target_vitals.health <= 0.0 {
            continue;
        }

        let damage_source = if attacker.is_player {
            DamageSource::Player(PlayerId(attacker.player_id.unwrap_or(0)))
        } else {
            DamageSource::Npc {
                entity: attacker.entity,
            }
        };
        pending_damage.push(DamageEvent {
            target: target_entity,
            amount: damage as f32,
            source: damage_source,
        });

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
        // Done here (before on-hit rolls re-apply Sleep) to preserve the
        // existing semantic where a Sleep on-hit can re-sleep the target.
        if let Some(effects) = target_magic.as_mut() {
            effects.clear(crate::magic::resources::EffectKind::Sleep);
        }
        broadcast_chat_line(
            &mut chat_log_query,
            format!(
                "[{} hit {} for {damage} {} damage]",
                attacker.name,
                target.name,
                attacker.damage_type.display_name()
            ),
        );

        // Roll the attacker's on-hit effects. Each entry is rolled
        // independently; effects only apply when the target carries a
        // `MagicEffects` component (every player/NPC does).
        if let Some(on_hit_effects) = definitions
            .get(&attacker.definition_id)
            .and_then(|def| def.attack_profile.as_ref())
            .map(|profile| profile.on_hit_effects.as_slice())
        {
            if !on_hit_effects.is_empty() {
                if let Some(effects) = target_magic.as_mut() {
                    for (i, on_hit) in on_hit_effects.iter().enumerate() {
                        let salt = attacker.object_id.wrapping_add((i as u64) << 16);
                        if !roll_chance(on_hit.chance, salt) {
                            continue;
                        }
                        let caster = if attacker.is_player {
                            attacker.player_id.map(PlayerId)
                        } else {
                            None
                        };
                        effects.apply(
                            EffectSpec {
                                kind: on_hit.kind,
                                magnitude: on_hit.magnitude,
                                seconds: on_hit.seconds,
                                secondary_magnitude: on_hit.secondary_magnitude,
                            },
                            caster,
                        );
                        broadcast_chat_line(
                            &mut chat_log_query,
                            format!(
                                "[{} is afflicted by {}]",
                                target.name,
                                effect_kind_display_name(on_hit.kind)
                            ),
                        );
                    }
                }
            }
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

fn effect_kind_display_name(kind: crate::magic::resources::EffectKind) -> &'static str {
    use crate::magic::resources::EffectKind;
    match kind {
        EffectKind::Glimmer => "Glimmer",
        EffectKind::Haste => "Haste",
        EffectKind::Shield => "Shield",
        EffectKind::Bless => "Bless",
        EffectKind::Slow => "Slow",
        EffectKind::Sleep => "Sleep",
        EffectKind::Paralyze => "Paralysis",
        EffectKind::Chill => "Chill",
        EffectKind::Burning => "Burning",
        EffectKind::Poisoned => "Poison",
        EffectKind::Drunk => "Drunkenness",
    }
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

pub(crate) fn chebyshev_distance(a: &TilePosition, b: &TilePosition) -> i32 {
    if a.z != b.z {
        return i32::MAX;
    }
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_defenses_passes_raw_through() {
        assert_eq!(apply_defenses(5, 0, 0), 5);
        assert_eq!(apply_defenses(100, 0, 0), 100);
    }

    #[test]
    fn block_subtracts_from_raw() {
        assert_eq!(apply_defenses(10, 3, 0), 7);
    }

    #[test]
    fn armor_subtracts_from_raw() {
        assert_eq!(apply_defenses(10, 0, 4), 6);
    }

    #[test]
    fn block_and_armor_stack() {
        assert_eq!(apply_defenses(10, 2, 3), 5);
    }

    #[test]
    fn floor_holds_when_mitigation_exceeds_damage() {
        assert_eq!(apply_defenses(2, 100, 0), 1);
        assert_eq!(apply_defenses(2, 0, 100), 1);
        assert_eq!(apply_defenses(1, 50, 50), 1);
    }

    #[test]
    fn roll_defense_zero_max_returns_zero() {
        assert_eq!(roll_defense(0, 0), 0);
        assert_eq!(roll_defense(-5, 0), 0);
    }

    #[test]
    fn roll_defense_within_range() {
        for salt in 0..10 {
            let r = roll_defense(5, salt);
            assert!((0..=5).contains(&r), "roll {r} out of 0..=5 (salt={salt})");
        }
    }
}
