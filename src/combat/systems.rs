use std::collections::HashSet;

use bevy::prelude::*;

use crate::combat::components::{AttackKind, AttackProfile, CombatLeash, CombatTarget};
use crate::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use crate::combat::damage_expr::DamageExpr;
use crate::combat::damage_type::DamageType;
use crate::combat::npc_casting::{
    active_effect_kinds, apply_self_outcome, apply_target_buffs, build_npc_cast_outcome,
    pick_npc_spell, NpcCastContext,
};
use crate::combat::resources::BattleTurnTimer;
use crate::game::resources::{GameUiEvent, PendingGameUiEvents};
use crate::magic::effects::MagicEffects;
use crate::magic::resources::{EffectKind, EffectSpec, SpellDefinition, SpellDefinitions};
use crate::npc::spellcasting::{NpcSpellEntry, SpellcastingProfile};
use crate::player::components::{
    AmmoConsumption, AttributeSet, ChatLog, DefenseStats, DerivedStats, Inventory, Player,
    PlayerId, PlayerIdentity, VitalStats, WeaponDamage,
};
use crate::player::progression::Experience;
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
    max_health: f32,
    is_player: bool,
    player_id: Option<u64>,
    ranged_projectile_sprite: Option<String>,
    armor: i32,
    block: i32,
    dodge_bonus: i32,
    block_chance_pct: i32,
    has_shield: bool,
    level: u32,
    /// Cloned for read-only spell selection during the per-attacker loop.
    /// Cooldown writes (`last_cast_at`) are batched and applied via p3
    /// after the loop.
    spellcasting: Option<Vec<NpcSpellEntry>>,
    /// Set of currently active effect kinds on this combatant (used by
    /// `SelfWithoutEffect` / `TargetWithoutEffect` spell conditions).
    active_effect_kinds: HashSet<EffectKind>,
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

/// Roll 1..=20 inclusive (a d20). Same nanosecond+salt jitter as
/// `roll_defense` — sufficient for non-security-sensitive combat rolls.
fn roll_d20(salt: u64) -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    ((mixed % 20) as i32) + 1
}

/// Returns the attack roll's total: `d20 + ability_mod + (NPC ? level : 0)`.
/// Players currently use no BAB — see `docs/progression.md` §7.1 (BAB lands in
/// a later progression batch).
fn attack_roll_total(attacker: &CombatantSnapshot, salt: u64) -> i32 {
    roll_d20(salt)
        + crate::combat::formulas::attack_to_hit_bonus(
            attacker.attack_profile.kind,
            attacker.attributes,
            attacker.is_player,
            attacker.level,
        )
}

fn dodge_dc(target: &CombatantSnapshot) -> i32 {
    crate::combat::formulas::dodge_dc(target.attributes.agility, target.dodge_bonus)
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

#[allow(clippy::type_complexity)]
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
            Option<&Experience>,
            Option<&SpellcastingProfile>,
            Option<&MagicEffects>,
        )>,
        Query<(
            &mut VitalStats,
            Option<&mut crate::magic::effects::MagicEffects>,
        )>,
        Query<&mut Inventory, With<Player>>,
        Query<&mut SpellcastingProfile>,
    )>,
    definitions: Res<OverworldObjectDefinitions>,
    object_registry: Res<ObjectRegistry>,
    spell_definitions: Res<SpellDefinitions>,
    mut chat_log_query: Query<&mut ChatLog, With<Player>>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    mut pending_damage: ResMut<PendingDamageEvents>,
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
                defense_stats,
                experience,
                spellcasting,
                magic_effects,
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
                let armor = defense_stats.map(|d| d.armor).unwrap_or(0);
                let block = defense_stats.map(|d| d.block).unwrap_or(0);
                let block_chance_pct = defense_stats.map(|d| d.block_chance).unwrap_or(0);
                let dodge_bonus = defense_stats.map(|d| d.dodge_bonus).unwrap_or(0);
                // Players have a shield iff one is in the shield slot; NPCs
                // are credited with one when their YAML provides any block
                // value (mitigation amount or chance). Either is enough to
                // gate the block roll uniformly.
                let has_shield = if is_player {
                    inventory
                        .and_then(|inv| {
                            inv.equipment_item(
                                crate::world::object_definitions::EquipmentSlot::Shield,
                            )
                        })
                        .is_some()
                } else {
                    block > 0 || block_chance_pct > 0
                };
                let level = experience.map(|e| e.level).unwrap_or(1);
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
                    max_health: vital_stats.max_health,
                    is_player,
                    player_id,
                    ranged_projectile_sprite,
                    armor,
                    block,
                    dodge_bonus,
                    block_chance_pct,
                    has_shield,
                    level,
                    spellcasting: spellcasting.map(|p| p.spells.clone()),
                    active_effect_kinds: active_effect_kinds(magic_effects),
                }
            },
        )
        .collect();

    // (entity, spell_index, new_last_cast_at) — drained after the main
    // loop into p3 so SpellcastingProfile mutations don't conflict with the
    // read-only p0 snapshot we're iterating from.
    let mut npc_cast_updates: Vec<(Entity, usize, f32)> = Vec::new();
    let now_seconds = time.elapsed_secs();

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

        // NPC spellcasting: takes priority over the physical attack. A
        // successful cast skips melee/ranged dispatch for this turn.
        if !attacker.is_player {
            if let Some(spells) = attacker.spellcasting.as_ref() {
                let ctx = NpcCastContext {
                    now_seconds,
                    attacker_position: attacker.position,
                    attacker_health: attacker.health,
                    attacker_max_health: attacker.max_health,
                    attacker_active_effects: &attacker.active_effect_kinds,
                    target_position: target.position,
                    target_health: target.health,
                    target_max_health: target.max_health,
                    target_active_effects: &target.active_effect_kinds,
                };
                if let Some(spell_idx) = pick_npc_spell(spells, &ctx) {
                    let entry = &spells[spell_idx];
                    if let Some(spell) = spell_definitions.get(&entry.spell_id) {
                        execute_npc_spell_cast(
                            spell,
                            entry.target_kind,
                            attacker,
                            target,
                            &mut combat_queries,
                            &mut ui_events,
                            &mut pending_damage,
                            &mut chat_log_query,
                            &mut commands,
                        );
                        npc_cast_updates.push((attacker.entity, spell_idx, now_seconds));
                        continue;
                    }
                }
            }
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

        // Stage 1: to-hit roll vs dodge DC. Misses spend ammo and play the
        // projectile but deal no damage.
        let attack_total = attack_roll_total(attacker, attacker.object_id);
        let dc = dodge_dc(target);
        if attack_total < dc {
            ui_events.push_broadcast(GameUiEvent::AttackDodged {
                attacker_object_id: attacker.object_id,
                target_object_id: target.object_id,
            });
            broadcast_chat_line(
                &mut chat_log_query,
                format!("[{} dodges {}'s attack]", target.name, attacker.name),
            );
            continue;
        }

        // Stage 2: roll weapon damage as today.
        let mut damage = attacker.damage_expr.roll(&attacker.attributes).max(1);

        // Stage 3: block roll (only if defender wields a shield). Chance is
        // shield's `block_chance` + AGI_mod * 2, clamped to [0, 95] so a hit
        // is never fully unstoppable.
        if target.has_shield {
            let chance_pct = crate::combat::formulas::effective_block_chance_pct(
                target.block_chance_pct,
                target.attributes.agility,
            );
            let chance = chance_pct as f32 / 100.0;
            // Salt with target object id so attacker/defender pairs roll
            // independently from on-hit effect rolls.
            if roll_chance(chance, target.object_id.wrapping_add(0xB10C_B10C)) {
                let block_roll = roll_defense(target.block, 0);
                damage = (damage - block_roll).max(1);
                ui_events.push_broadcast(GameUiEvent::AttackBlocked {
                    attacker_object_id: attacker.object_id,
                    target_object_id: target.object_id,
                    amount: block_roll,
                });
                broadcast_chat_line(
                    &mut chat_log_query,
                    format!("[{} blocks {block_roll} damage]", target.name),
                );
            }
        }

        // Stage 4: armor mitigation (unchanged — additive uniform roll).
        let armor_roll = roll_defense(target.armor, 1);
        let damage = (damage - armor_roll).max(1);

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
        let vfx_override = definitions
            .get(&attacker.definition_id)
            .and_then(|def| def.attack_profile.as_ref())
            .and_then(|profile| profile.hit_vfx.clone());
        pending_damage.push(DamageEvent {
            target: target_entity,
            amount: damage as f32,
            source: damage_source,
            damage_type: attacker.damage_type,
            vfx_override,
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

    // Apply queued cooldown updates from NPC spell casts. Done after the
    // main loop because p3 (SpellcastingProfile) shares the storage that
    // p0 read above; ParamSet only lets one set be active at a time.
    if !npc_cast_updates.is_empty() {
        let mut profile_query = combat_queries.p3();
        for (entity, idx, now) in npc_cast_updates {
            if let Ok(mut profile) = profile_query.get_mut(entity) {
                if let Some(entry) = profile.spells.get_mut(idx) {
                    entry.last_cast_at = now;
                }
            }
        }
    }
}

/// Apply a single NPC spell cast: emit damage events, apply buffs to target
/// and caster, restore caster HP/mana for self-casts, broadcast VFX and
/// chat. Mirrors the player cast handler's effect-application surface but
/// goes through the NPC-side primitives so player class/level/scroll gates
/// don't interfere.
#[allow(clippy::too_many_arguments)]
fn execute_npc_spell_cast(
    spell: &SpellDefinition,
    target_kind: crate::npc::spellcasting::NpcSpellTargetKind,
    attacker: &CombatantSnapshot,
    target: &CombatantSnapshot,
    combat_queries: &mut ParamSet<(
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
            Option<&Experience>,
            Option<&SpellcastingProfile>,
            Option<&MagicEffects>,
        )>,
        Query<(
            &mut VitalStats,
            Option<&mut crate::magic::effects::MagicEffects>,
        )>,
        Query<&mut Inventory, With<Player>>,
        Query<&mut SpellcastingProfile>,
    )>,
    ui_events: &mut PendingGameUiEvents,
    pending_damage: &mut PendingDamageEvents,
    chat_log_query: &mut Query<&mut ChatLog, With<Player>>,
    commands: &mut Commands,
) {
    use crate::npc::spellcasting::NpcSpellTargetKind;

    let outcome = build_npc_cast_outcome(
        spell,
        target_kind,
        attacker.entity,
        &attacker.name,
        attacker.space_id,
        attacker.position,
        target.entity,
        &target.name,
        target.position,
    );

    for vfx in &outcome.vfx {
        ui_events.push_broadcast(vfx.clone());
    }
    for damage in &outcome.damage_events {
        pending_damage.push(damage.clone());
    }
    for msg in &outcome.chat_messages {
        broadcast_chat_line(chat_log_query, msg.clone());
    }

    // Mutate the attacker (self-cast heal + self-buffs) and the target
    // (target-buffs). For self-cast spells, attacker == target.
    {
        let mut entities_query = combat_queries.p1();
        if matches!(target_kind, NpcSpellTargetKind::SelfCast) {
            if let Ok((mut vitals, magic)) = entities_query.get_mut(attacker.entity) {
                if let Some(mut effects) = magic {
                    apply_self_outcome(&outcome, &mut vitals, &mut effects);
                } else {
                    // Lazily attach MagicEffects so self-buffs land.
                    let mut new_effects = MagicEffects::default();
                    apply_self_outcome(&outcome, &mut vitals, &mut new_effects);
                    if !new_effects.is_empty() {
                        commands.entity(attacker.entity).insert(new_effects);
                    }
                }
            }
        } else {
            // Apply target buffs first (separate query borrow).
            if !outcome.target_buffs.is_empty() {
                if let Ok((_, magic)) = entities_query.get_mut(target.entity) {
                    if let Some(mut effects) = magic {
                        apply_target_buffs(&outcome, &mut effects);
                    } else {
                        let mut new_effects = MagicEffects::default();
                        apply_target_buffs(&outcome, &mut new_effects);
                        if !new_effects.is_empty() {
                            commands.entity(target.entity).insert(new_effects);
                        }
                    }
                }
            }
            // Then apply self-buffs / clears on the caster.
            if !outcome.self_buffs.is_empty() || !outcome.self_clears.is_empty() {
                if let Ok((mut vitals, magic)) = entities_query.get_mut(attacker.entity) {
                    if let Some(mut effects) = magic {
                        apply_self_outcome(&outcome, &mut vitals, &mut effects);
                    } else {
                        let mut new_effects = MagicEffects::default();
                        apply_self_outcome(&outcome, &mut vitals, &mut new_effects);
                        if !new_effects.is_empty() {
                            commands.entity(attacker.entity).insert(new_effects);
                        }
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
    use crate::combat::damage_type::DamageType;

    fn snapshot(
        strength: i32,
        agility: i32,
        level: u32,
        is_player: bool,
        armor: i32,
        block: i32,
        block_chance_pct: i32,
        dodge_bonus: i32,
        has_shield: bool,
    ) -> CombatantSnapshot {
        CombatantSnapshot {
            entity: Entity::PLACEHOLDER,
            target: None,
            attack_profile: AttackProfile {
                kind: AttackKind::Melee,
                damage_type: DamageType::Blunt,
            },
            space_id: crate::world::components::SpaceId(0),
            position: TilePosition { x: 0, y: 0, z: 0 },
            object_id: 0,
            name: "dummy".to_string(),
            definition_id: "dummy".to_string(),
            attributes: AttributeSet {
                strength,
                agility,
                constitution: 10,
                willpower: 10,
                charisma: 10,
                focus: 10,
            },
            damage_expr: DamageExpr::melee_default(),
            damage_type: DamageType::Blunt,
            health: 100.0,
            max_health: 100.0,
            is_player,
            player_id: None,
            ranged_projectile_sprite: None,
            armor,
            block,
            dodge_bonus,
            block_chance_pct,
            has_shield,
            level,
            spellcasting: None,
            active_effect_kinds: HashSet::new(),
        }
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

    #[test]
    fn roll_d20_within_range() {
        for salt in 0..20 {
            let r = roll_d20(salt);
            assert!(
                (1..=20).contains(&r),
                "d20 roll {r} out of 1..=20 (salt={salt})"
            );
        }
    }

    #[test]
    fn dodge_dc_uses_agi_mod_and_item_bonus() {
        // AGI 14 → +2 mod; +3 dodge bonus from items → DC 15.
        let target = snapshot(10, 14, 1, true, 0, 0, 0, 3, false);
        assert_eq!(dodge_dc(&target), 15);
    }

    #[test]
    fn dodge_dc_floors_at_10_minus_agi_penalty() {
        // AGI 6 → -2 mod; no items → DC 8.
        let target = snapshot(10, 6, 1, true, 0, 0, 0, 0, false);
        assert_eq!(dodge_dc(&target), 8);
    }

    #[test]
    fn attack_roll_total_player_skips_level_bonus() {
        // Player STR 14 → +2 mod. Roll is d20 + 2, in [3, 22].
        let attacker = snapshot(14, 10, 5, true, 0, 0, 0, 0, false);
        for salt in 0..30 {
            let total = attack_roll_total(&attacker, salt);
            assert!(
                (3..=22).contains(&total),
                "player attack {total} out of [3,22] (salt={salt})"
            );
        }
    }

    #[test]
    fn attack_roll_total_npc_adds_level() {
        // NPC level 6, STR 12 → +1 mod. Roll is d20 + 1 + 6, in [8, 27].
        let attacker = snapshot(12, 10, 6, false, 0, 0, 0, 0, false);
        for salt in 0..30 {
            let total = attack_roll_total(&attacker, salt);
            assert!(
                (8..=27).contains(&total),
                "npc attack {total} out of [8,27] (salt={salt})"
            );
        }
    }
}
