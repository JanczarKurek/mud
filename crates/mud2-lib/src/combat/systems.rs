use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::combat::components::{AttackKind, AttackProfile, CombatLeash, CombatTarget};
use crate::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use crate::combat::damage_expr::DamageExpr;
use crate::combat::damage_type::DamageType;
use crate::combat::modifiers::{roll_bonus_damage, ItemModifier, ModifierDuration, ModifierEffect};
use crate::combat::npc_casting::{
    active_effect_kinds, apply_self_outcome, apply_target_buffs, build_npc_cast_outcome,
    pick_npc_spell, NpcCastContext,
};
use crate::combat::resources::{BattleTurnTimer, PendingModifierConsumption};
use crate::game::resources::{GameUiEvent, PendingGameUiEvents};
use crate::magic::effects::MagicEffects;
use crate::magic::resources::{EffectKind, EffectSpec, SpellDefinition, SpellDefinitions};
use crate::npc::components::Companion;
use crate::npc::spellcasting::{NpcSpellEntry, NpcSpellTargetKind, SpellcastingProfile};
use crate::player::classes::{class_data, BabTrack, Class};
use crate::player::components::{
    AmmoConsumption, AttributeSet, ChatLog, DefenseStats, DerivedStats, Inventory, Player,
    PlayerId, PlayerIdentity, VitalStats, WeaponDamage,
};
use crate::player::progression::Experience;
use crate::world::components::{tile_distance_3d, OverworldObject, SpaceResident, TilePosition};
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
    /// `Some(owner)` iff this combatant is a companion owned by a player. Its
    /// attacks are credited to that player (`DamageSource::OwnedByPlayer`), so a
    /// summoned creature's kills grant the summoner XP / quest progress.
    owner_player: Option<PlayerId>,
    ranged_projectile_sprite: Option<String>,
    armor: i32,
    block: i32,
    dodge_bonus: i32,
    block_chance_pct: i32,
    has_shield: bool,
    level: u32,
    /// BAB advancement track: a player's class track, or a creature's YAML
    /// `bab_track` (default ¾). Feeds `attack_to_hit_bonus`.
    bab_track: BabTrack,
    /// The player's class (`None` for NPCs). Feeds the Fighter Weapon Focus
    /// to-hit bonus and the Vagabond Backstab dice.
    class: Option<Class>,
    /// `true` iff this combatant is a player currently in sneak mode. Feeds
    /// the backstab check (attacking from undetected stealth).
    sneaking: bool,
    /// Lowest raw d20 face that turns a landed hit into a critical (double
    /// damage roll). Players resolve it from the equipped weapon's
    /// `crit_range`, NPCs from their own definition; default 20, clamped
    /// `2..=20` here so YAML can't make every hit crit.
    crit_threshold: i32,
    /// Cloned for read-only spell selection during the per-attacker loop.
    /// Cooldown writes (`last_cast_at`) are batched and applied via p3
    /// after the loop.
    spellcasting: Option<Vec<NpcSpellEntry>>,
    /// Set of currently active effect kinds on this combatant (used by
    /// `SelfWithoutEffect` / `TargetWithoutEffect` spell conditions).
    active_effect_kinds: HashSet<EffectKind>,
    /// Per-instance modifiers on this combatant's equipped weapon. Empty for
    /// NPCs and for players wielding an un-enchanted weapon. Read-only here;
    /// charge consumption is deferred via `PendingModifierConsumption`.
    weapon_modifiers: Vec<ItemModifier>,
}

/// Roll 1..=20 inclusive (a d20). Same nanosecond+salt jitter as
/// `damage_expr::roll_die` — sufficient for non-security-sensitive combat rolls.
fn roll_d20(salt: u64) -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    ((mixed % 20) as i32) + 1
}

/// Rolls an attack and returns `(raw_d20, total)` where
/// `total = d20 + ability_mod + bab_at(track, level) + elevation_mod`. The
/// elevation bonus is ranged-only — melee and spells get no high/low ground
/// term. The raw d20 is returned so callers can apply the natural-20 (auto-hit)
/// / natural-1 (auto-miss) rule from `progression.md` §7.1.
fn attack_roll_total(
    attacker: &CombatantSnapshot,
    target: &CombatantSnapshot,
    salt: u64,
) -> (i32, i32) {
    let d20 = roll_d20(salt);
    let mut total = d20
        + crate::combat::formulas::attack_to_hit_bonus(
            attacker.attack_profile.kind,
            attacker.attributes,
            attacker.bab_track,
            attacker.level,
            attacker.class,
        );
    if matches!(attacker.attack_profile.kind, AttackKind::Ranged { .. }) {
        total +=
            crate::combat::formulas::elevation_to_hit_mod(attacker.position.z, target.position.z);
    }
    (d20, total)
}

fn dodge_dc(target: &CombatantSnapshot) -> i32 {
    crate::combat::formulas::dodge_dc(target.level, target.attributes.agility, target.dodge_bonus)
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
    entity_query: Query<(&SpaceResident, &TilePosition, Option<&VitalStats>)>,
) {
    for (entity, combat_target, attacker_space, attacker_position, leash) in &target_query {
        if combat_target.entity == entity {
            commands.entity(entity).remove::<CombatTarget>();
            continue;
        }

        let Ok((target_space, target_position, target_vitals)) =
            entity_query.get(combat_target.entity)
        else {
            commands.entity(entity).remove::<CombatTarget>();
            continue;
        };

        // Dead targets are as gone as despawned ones. Matters for players,
        // whose body entity stays in place at HP 0 until the respawn click —
        // without this, mobs keep swinging at (and crowding around) the grave.
        if target_vitals.is_some_and(|v| v.health <= 0.0) {
            commands.entity(entity).remove::<CombatTarget>();
            continue;
        }

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
        Query<&mut crate::player::components::Exertion, With<Player>>,
    )>,
    definitions: Res<OverworldObjectDefinitions>,
    object_registry: Res<ObjectRegistry>,
    spell_definitions: Res<SpellDefinitions>,
    // Separate from the p0 ParamSet on purpose: these components are touched by
    // no other combat query, so disjoint read-only access avoids both a
    // tuple-arity overflow on p0 and any aliasing conflict. Tupled so the
    // system stays under Bevy's 16-param cap.
    // `.0`: Companion (kill credit for player-owned summons).
    // `.1`: AiState (backstab awareness check — its writer
    //       `update_roaming_npcs` is ordered before this system).
    // `.2`: a player's class (BAB track, Weapon Focus, Backstab) and live
    //       sneak flag; p0 is already at the max query tuple arity, so this
    //       is pre-collected into a map before the p0 snapshot loop.
    aux_reads: (
        Query<&Companion>,
        Query<&crate::npc::components::AiState, With<crate::npc::components::Npc>>,
        Query<(Entity, &Class, Has<crate::player::components::Sneaking>)>,
    ),
    collider_query: Query<
        (&SpaceResident, &TilePosition, Option<&OverworldObject>),
        (With<crate::world::components::Collider>, Without<Player>),
    >,
    floor_maps: Option<Res<crate::world::floor_map::FloorMaps>>,
    floor_defs: Option<Res<crate::world::floor_definitions::FloorTilesetDefinitions>>,
    mut chat_log_query: ScopedChatLogQuery,
    mut ui_events: ResMut<PendingGameUiEvents>,
    // Tupled for the same reason as `aux_reads`: this system is at Bevy's
    // 16-system-param cap, and NPC summons need their own deferred queue.
    mut pending_writes: (
        ResMut<PendingDamageEvents>,
        ResMut<crate::combat::resources::PendingNpcSummons>,
        ResMut<crate::combat::resources::PendingRetaliations>,
    ),
    mut pending_noise: ResMut<crate::world::noise::PendingNoiseEvents>,
    mut pending_modifier_consumption: ResMut<PendingModifierConsumption>,
    mut commands: Commands,
) {
    let _t = crate::diagnostics::SystemTimer::new("combat:resolve_battle_turn", 1.0);
    let (ref mut pending_damage, ref mut pending_summons, ref mut pending_retaliations) =
        pending_writes;
    battle_turn_timer.remaining_seconds -= time.delta_secs();
    if battle_turn_timer.remaining_seconds > 0.0 {
        return;
    }

    while battle_turn_timer.remaining_seconds <= 0.0 {
        battle_turn_timer.remaining_seconds += battle_turn_timer.interval_seconds;
    }

    // Rebuilt every battle tick so painted-floor ceilings block ranged shots
    // and spells exactly like they block NPC vision. Excluding the player from
    // the collider query is intentional — we don't want the player's own body
    // to occlude their shot to a target on the next tile.
    let los_blockers = crate::world::spatial::build_los_blockers(
        collider_query.iter(),
        Some(&definitions),
        floor_maps.as_deref(),
        floor_defs.as_deref(),
    );

    // Pre-collect each player's class, BAB track, and sneak flag keyed by
    // entity, so the p0 snapshot loop below can look them up per combatant.
    let player_tracks: HashMap<Entity, (Class, BabTrack, bool)> = aux_reads
        .2
        .iter()
        .map(|(entity, class, sneaking)| (entity, (*class, class_data(*class).bab_track, sneaking)))
        .collect();

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
                let owner_player = aux_reads
                    .0
                    .get(entity)
                    .ok()
                    .and_then(|companion| companion.owner_player);
                let ammo_type_id = inventory.and_then(|inv| {
                    inv.equipment_item(crate::world::object_definitions::EquipmentSlot::Ammo)
                        .map(|item| item.type_id.clone())
                });
                let weapon_modifiers = inventory
                    .and_then(|inv| {
                        inv.equipment_item(crate::world::object_definitions::EquipmentSlot::Weapon)
                    })
                    .map(|item| item.modifiers.clone())
                    .unwrap_or_default();
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
                let player_class_info = player_tracks.get(&entity).copied();
                // Crit threshold: players read the equipped weapon's
                // `crit_range`, NPCs their own definition. Default 20.
                let crit_threshold = if is_player {
                    inventory
                        .and_then(|inv| {
                            inv.equipment_item(
                                crate::world::object_definitions::EquipmentSlot::Weapon,
                            )
                        })
                        .and_then(|item| definitions.get(&item.type_id))
                        .and_then(|def| def.crit_range)
                } else {
                    definitions
                        .get(&overworld_object.definition_id)
                        .and_then(|def| def.crit_range)
                }
                .unwrap_or(20)
                .clamp(2, 20);
                // Players: BAB track from class (default Fighter/Full if somehow
                // unset). NPCs: from the creature's YAML `bab_track`, default ¾.
                let bab_track = if is_player {
                    player_class_info
                        .map(|(_, track, _)| track)
                        .unwrap_or(BabTrack::Full)
                } else {
                    definitions
                        .get(&overworld_object.definition_id)
                        .and_then(|def| def.bab_track)
                        .unwrap_or(BabTrack::ThreeQuarter)
                };
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
                    owner_player,
                    ranged_projectile_sprite,
                    armor,
                    block,
                    dodge_bonus,
                    block_chance_pct,
                    has_shield,
                    level,
                    bab_track,
                    class: player_class_info.map(|(class, _, _)| class),
                    sneaking: player_class_info
                        .map(|(_, _, sneaking)| sneaking)
                        .unwrap_or(false),
                    crit_threshold,
                    spellcasting: spellcasting.map(|p| p.spells.clone()),
                    active_effect_kinds: active_effect_kinds(magic_effects),
                    weapon_modifiers,
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
                            pending_damage,
                            pending_summons,
                            &mut chat_log_query,
                            &mut commands,
                        );
                        npc_cast_updates.push((attacker.entity, spell_idx, now_seconds));
                        // A hostile cast aimed at a player counts as being
                        // attacked for auto-retaliate; self-buffs/heals don't.
                        if target.is_player && entry.target_kind != NpcSpellTargetKind::SelfCast {
                            pending_retaliations.items.push(
                                crate::combat::resources::RetaliationHit {
                                    player: target.entity,
                                    attacker: attacker.entity,
                                    attacker_name: attacker.name.clone(),
                                },
                            );
                        }
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
        // Ranged attacks need a clear voxel line — without this, a player
        // standing on the upper floor can shoot through the ceiling at an
        // enemy on floor 0. Melee already implies adjacency in `is_target_in_range`
        // so we skip the check for it. `los_blockers` covers walls (with
        // `block_size` inflated), object colliders, and painted ceilings.
        if is_ranged
            && attacker.space_id == target.space_id
            && !crate::world::spatial::has_line_of_sight(
                attacker.position,
                target.position,
                attacker.space_id,
                &los_blockers,
            )
        {
            if attacker.is_player {
                push_chat_line_near(
                    &mut chat_log_query,
                    attacker.space_id,
                    attacker.position,
                    &format!("[{} has no clear shot]", attacker.name),
                );
            }
            continue;
        }

        if is_ranged && attacker.is_player {
            let mut inventory_query = combat_queries.p2();
            let Ok(mut inventory) = inventory_query.get_mut(attacker.entity) else {
                continue;
            };
            match inventory.consume_one_ammo() {
                AmmoConsumption::None => {
                    push_chat_line_near(
                        &mut chat_log_query,
                        attacker.space_id,
                        attacker.position,
                        &format!("[{} is out of ammo]", attacker.name),
                    );
                    continue;
                }
                AmmoConsumption::Decremented | AmmoConsumption::Emptied => {}
            }
        }

        if is_ranged {
            let sprite_id = attacker
                .ranged_projectile_sprite
                .clone()
                .unwrap_or_else(|| "arrow".to_owned());
            ui_events.push_broadcast_near(
                attacker.space_id,
                attacker.position,
                GameUiEvent::ProjectileFired {
                    from_tile: attacker.position,
                    to_tile: target.position,
                    sprite_definition_id: sprite_id,
                    // Ranged physical attacks keep the original fixed flight
                    // feel; their damage already resolved this turn (cosmetic
                    // only).
                    duration_seconds:
                        crate::client_effects::projectile::PROJECTILE_DURATION_SECONDS,
                    target_object_id: None,
                },
            );
        }

        // A committed attack (range, LoS, and ammo all cleared) is loud — even
        // a miss. Nearby NPCs hear the scuffle and investigate.
        pending_noise.push(
            attacker.space_id,
            attacker.position,
            crate::world::noise::ATTACK_NOISE,
        );

        // Recorded before the to-hit roll on purpose: a dodged or blocked
        // swing still counts as being attacked for auto-retaliate.
        if !attacker.is_player && target.is_player {
            pending_retaliations
                .items
                .push(crate::combat::resources::RetaliationHit {
                    player: target.entity,
                    attacker: attacker.entity,
                    attacker_name: attacker.name.clone(),
                });
        }

        // Swinging a weapon is tiring — a committed attack costs the player
        // exertion whether it lands or misses (`utility_systems.md` §6.1).
        if attacker.is_player {
            let mut exertion_query = combat_queries.p4();
            if let Ok(mut exertion) = exertion_query.get_mut(attacker.entity) {
                exertion.add(crate::player::exertion::EXERTION_COST_ATTACK);
            }
        }

        // A committed attack breaks stealth, hit or miss. The snapshot keeps
        // `sneaking: true` for THIS swing (the backstab opener below); the
        // component removal replicates via the projection's
        // `PlayerSneakingChanged` diff, and the next AI tick re-detects the
        // now-visible player normally (the attack noise above already pulls
        // out-of-sight NPCs into Alert).
        if attacker.is_player && attacker.sneaking {
            commands
                .entity(attacker.entity)
                .remove::<crate::player::components::Sneaking>();
        }

        // Stage 1: to-hit roll.
        let Some(d20) = roll_to_hit(attacker, target, &mut ui_events, &mut chat_log_query) else {
            continue;
        };

        // Stage 2: roll weapon damage.
        let (mut damage, crit) = roll_weapon_damage(attacker, d20);

        // Backstab opener bonus (never doubled by crit).
        damage += apply_backstab(attacker, target, &aux_reads.1, &mut chat_log_query);

        // Stage 3: block roll.
        damage = roll_block(
            attacker,
            target,
            damage,
            &mut ui_events,
            &mut chat_log_query,
        );

        // Stage 4: armor mitigation.
        let damage = apply_armor_mitigation(target, damage);

        let mut target_query = combat_queries.p1();
        let Ok((target_vitals, mut target_magic)) = target_query.get_mut(target_entity) else {
            continue;
        };

        if target_vitals.health <= 0.0 {
            continue;
        }

        let damage_source = if attacker.is_player {
            DamageSource::Player(PlayerId(attacker.player_id.unwrap_or(0)))
        } else if let Some(owner) = attacker.owner_player {
            // Player-owned companion (summon): credit the kill to its owner so
            // XP / quest progress / kill feed land on the summoner.
            DamageSource::OwnedByPlayer(owner)
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
            attacker: Some(attacker.entity),
        });

        // Modifier-driven bonus elemental damage.
        apply_bonus_damage(
            attacker,
            target,
            damage_source,
            pending_damage,
            &mut chat_log_query,
            &mut pending_modifier_consumption,
        );

        // Sleep-wakes-on-damage is handled centrally in
        // `apply_pending_damage` so every damage source — melee, ranged,
        // spell, DoT, environment — wakes the target uniformly. NPCs keep
        // their CombatTarget so they re-engage immediately after waking.
        if crit {
            ui_events.push_broadcast_near(
                attacker.space_id,
                attacker.position,
                GameUiEvent::AttackCrit {
                    attacker_object_id: attacker.object_id,
                    target_object_id: target.object_id,
                },
            );
            push_chat_line_near(
                &mut chat_log_query,
                attacker.space_id,
                attacker.position,
                &format!(
                    "[{} CRITS {} for {damage} {} damage!]",
                    attacker.name,
                    target.name,
                    attacker.damage_type.display_name()
                ),
            );
        } else {
            push_chat_line_near(
                &mut chat_log_query,
                attacker.space_id,
                attacker.position,
                &format!(
                    "[{} hit {} for {damage} {} damage]",
                    attacker.name,
                    target.name,
                    attacker.damage_type.display_name()
                ),
            );
        }

        // On-hit effects (definition + per-instance enchantments).
        apply_on_hit_effects(
            attacker,
            target,
            &definitions,
            &mut chat_log_query,
            &mut pending_modifier_consumption,
            target_magic.as_deref_mut(),
            &mut commands,
        );
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

/// Stage 1: to-hit roll vs dodge DC. Misses spend ammo and play the
/// projectile but deal no damage. A natural 20 always hits and a natural
/// 1 always misses regardless of modifiers (`progression.md` §7.1), so
/// even lopsided matchups keep a 5% hit/whiff floor.
///
/// Returns `Some(raw_d20)` on a landed hit (the raw face feeds the crit
/// check) or `None` on a miss, after broadcasting the dodge feedback.
fn roll_to_hit(
    attacker: &CombatantSnapshot,
    target: &CombatantSnapshot,
    ui_events: &mut PendingGameUiEvents,
    chat_log_query: &mut ScopedChatLogQuery,
) -> Option<i32> {
    let (d20, attack_total) = attack_roll_total(attacker, target, attacker.object_id);
    let dc = dodge_dc(target);
    let hit = d20 == 20 || (d20 != 1 && attack_total >= dc);
    if !hit {
        ui_events.push_broadcast_near(
            attacker.space_id,
            attacker.position,
            GameUiEvent::AttackDodged {
                attacker_object_id: attacker.object_id,
                target_object_id: target.object_id,
            },
        );
        push_chat_line_near(
            chat_log_query,
            attacker.space_id,
            attacker.position,
            &format!("[{} dodges {}'s attack]", target.name, attacker.name),
        );
        return None;
    }
    Some(d20)
}

/// Stage 2: roll weapon damage. Level is passed for expressions with a
/// `level` term. A raw d20 at or above the attacker's crit threshold
/// upgrades the landed hit to a critical: the damage expression is
/// rolled TWICE (3.5e-style, distinct salt so same-nanosecond rolls
/// stay independent) and summed, before block/armor. Enchant
/// `BonusDamage` riders are not doubled.
///
/// Returns `(damage, crit)`.
fn roll_weapon_damage(attacker: &CombatantSnapshot, d20: i32) -> (i32, bool) {
    let crit = d20 >= attacker.crit_threshold;
    let mut damage = attacker
        .damage_expr
        .roll(&attacker.attributes, attacker.level as i32)
        .max(1);
    if crit {
        damage += attacker
            .damage_expr
            .roll_salted(
                &attacker.attributes,
                attacker.level as i32,
                attacker.object_id.wrapping_add(0xC417_C417),
            )
            .max(1);
    }
    (damage, crit)
}

/// Backstab (`progression.md` §3.4): a sneaking player striking an NPC
/// that is UNAWARE of them (not targeting, pursuing, engaging, or
/// fleeing from the attacker — Alert/searching still counts as
/// unaware). Vagabonds add their scaling class dice; anyone else gets
/// a small flat opener bonus. Applied once (never doubled by crit) and
/// still subject to block/armor.
///
/// Returns the bonus damage to add, `0` when the backstab conditions
/// don't hold.
fn apply_backstab(
    attacker: &CombatantSnapshot,
    target: &CombatantSnapshot,
    ai_state_query: &Query<&crate::npc::components::AiState, With<crate::npc::components::Npc>>,
    chat_log_query: &mut ScopedChatLogQuery,
) -> i32 {
    let backstab = attacker.is_player
        && attacker.sneaking
        && !target.is_player
        && !crate::npc::detection::npc_aware_of(
            ai_state_query.get(target.entity).ok(),
            target.target,
            attacker.entity,
        );
    if !backstab {
        return 0;
    }
    let bonus = match attacker.class {
        Some(Class::Vagabond) => {
            let dice = crate::combat::formulas::backstab_dice(attacker.level);
            let mut total = 0i32;
            for i in 0..dice {
                total += crate::combat::damage_expr::roll_die(
                    6,
                    attacker.object_id.wrapping_add(0x00BA_C5AB + i as u64),
                );
            }
            total
        }
        _ => crate::combat::formulas::BACKSTAB_FLAT_BONUS,
    };
    push_chat_line_near(
        chat_log_query,
        attacker.space_id,
        attacker.position,
        &format!(
            "[{} strikes {} from the shadows: +{bonus} backstab damage]",
            attacker.name, target.name
        ),
    );
    bonus.max(0)
}

/// Stage 3: block roll (only if defender wields a shield). Chance is
/// shield's `block_chance` + AGI_mod * 2, clamped to [0, 95] so a hit
/// is never fully unstoppable. On a successful block the FULL `block`
/// value is removed (deterministic) — the old uniform 0..block roll made
/// a wooden shield worth <0.5 dmg/hit, a defensive system that did
/// nothing. The randomness now lives solely in the chance gate.
///
/// Returns the post-block damage (unchanged when there is no shield or
/// the block roll fails).
fn roll_block(
    attacker: &CombatantSnapshot,
    target: &CombatantSnapshot,
    damage: i32,
    ui_events: &mut PendingGameUiEvents,
    chat_log_query: &mut ScopedChatLogQuery,
) -> i32 {
    if !target.has_shield {
        return damage;
    }
    let chance_pct = crate::combat::formulas::effective_block_chance_pct(
        target.block_chance_pct,
        target.attributes.agility,
    );
    let chance = chance_pct as f32 / 100.0;
    // Salt with target object id so attacker/defender pairs roll
    // independently from on-hit effect rolls.
    if !roll_chance(chance, target.object_id.wrapping_add(0xB10C_B10C)) {
        return damage;
    }
    let block_amount = target.block.max(0);
    ui_events.push_broadcast_near(
        target.space_id,
        target.position,
        GameUiEvent::AttackBlocked {
            attacker_object_id: attacker.object_id,
            target_object_id: target.object_id,
            amount: block_amount,
        },
    );
    push_chat_line_near(
        chat_log_query,
        target.space_id,
        target.position,
        &format!("[{} blocks {block_amount} damage]", target.name),
    );
    (damage - block_amount).max(1)
}

/// Stage 4: armor mitigation — deterministic FULL `armor` value, floored
/// so a hit always lands at least 1. The item card now means what it
/// says (`armor: 4` blocks 4); armor values on items/creatures are tuned
/// for full-value subtraction (roughly half the old numbers).
fn apply_armor_mitigation(target: &CombatantSnapshot, damage: i32) -> i32 {
    (damage - target.armor.max(0)).max(1)
}

/// Modifier-driven bonus elemental damage. Each `BonusDamage`
/// enchantment on the attacker's equipped weapon lands as its own
/// `DamageEvent` so the element shows its own number and hit VFX
/// (`vfx_override: None` falls back to `damage_type.default_hit_vfx_id`).
fn apply_bonus_damage(
    attacker: &CombatantSnapshot,
    target: &CombatantSnapshot,
    damage_source: DamageSource,
    pending_damage: &mut PendingDamageEvents,
    chat_log_query: &mut ScopedChatLogQuery,
    pending_modifier_consumption: &mut PendingModifierConsumption,
) {
    for (i, m) in attacker.weapon_modifiers.iter().enumerate() {
        let ModifierEffect::BonusDamage {
            dice,
            bonus,
            damage_type,
        } = &m.effect
        else {
            continue;
        };
        let salt = attacker.object_id.wrapping_add(0x00B0_0000 + i as u64);
        let extra = roll_bonus_damage(*dice, *bonus, salt).max(0);
        if extra == 0 {
            continue;
        }
        pending_damage.push(DamageEvent {
            target: target.entity,
            amount: extra as f32,
            source: damage_source,
            damage_type: *damage_type,
            vfx_override: None,
            attacker: Some(attacker.entity),
        });
        push_chat_line_near(
            chat_log_query,
            attacker.space_id,
            attacker.position,
            &format!(
                "[{}'s {} sears {} for {extra} {} damage]",
                attacker.name,
                enchantment_label(m),
                target.name,
                damage_type.display_name()
            ),
        );
        if matches!(m.duration, ModifierDuration::Charges { .. }) {
            pending_modifier_consumption
                .spent
                .push((attacker.entity, m.type_ex.clone()));
        }
    }
}

/// Roll the attacker's on-hit effects, from both the weapon definition
/// and any per-instance modifiers on the equipped weapon. Each entry is
/// rolled independently; rolled specs go through `apply_effects_lazy` so
/// a flaming weapon striking a freshly-spawned NPC (no `MagicEffects`
/// component yet) still ignites it.
fn apply_on_hit_effects(
    attacker: &CombatantSnapshot,
    target: &CombatantSnapshot,
    definitions: &OverworldObjectDefinitions,
    chat_log_query: &mut ScopedChatLogQuery,
    pending_modifier_consumption: &mut PendingModifierConsumption,
    target_magic: Option<&mut crate::magic::effects::MagicEffects>,
    commands: &mut Commands,
) {
    let caster = if attacker.is_player {
        attacker.player_id.map(PlayerId)
    } else if attacker.owner_player.is_some() {
        // Companion on-hit DoTs (e.g. a flaming summon) credit the owner too.
        attacker.owner_player
    } else {
        None
    };
    let mut rolled_specs: Vec<EffectSpec> = Vec::new();

    if let Some(on_hit_effects) = definitions
        .get(&attacker.definition_id)
        .and_then(|def| def.attack_profile.as_ref())
        .map(|profile| profile.on_hit_effects.as_slice())
    {
        for (i, on_hit) in on_hit_effects.iter().enumerate() {
            let salt = attacker.object_id.wrapping_add((i as u64) << 16);
            if !roll_chance(on_hit.chance, salt) {
                continue;
            }
            rolled_specs.push(EffectSpec {
                kind: on_hit.kind,
                magnitude: on_hit.magnitude,
                seconds: on_hit.seconds,
                secondary_magnitude: on_hit.secondary_magnitude,
            });
            push_chat_line_near(
                chat_log_query,
                target.space_id,
                target.position,
                &format!(
                    "[{} is afflicted by {}]",
                    target.name,
                    effect_kind_display_name(on_hit.kind)
                ),
            );
        }
    }

    // Per-instance `OnHit` modifiers (enchantments). Distinct salt offset
    // so they roll independently from the definition effects above. A
    // successful application spends one charge on charge-limited modifiers.
    for (i, m) in attacker.weapon_modifiers.iter().enumerate() {
        let ModifierEffect::OnHit { chance, spec } = &m.effect else {
            continue;
        };
        let salt = attacker.object_id.wrapping_add(0x00E0_0000 + i as u64);
        if !roll_chance(*chance, salt) {
            continue;
        }
        rolled_specs.push(*spec);
        push_chat_line_near(
            chat_log_query,
            target.space_id,
            target.position,
            &format!(
                "[{} is afflicted by {}]",
                target.name,
                effect_kind_display_name(spec.kind)
            ),
        );
        if matches!(m.duration, ModifierDuration::Charges { .. }) {
            pending_modifier_consumption
                .spent
                .push((attacker.entity, m.type_ex.clone()));
        }
    }

    if !rolled_specs.is_empty() {
        crate::magic::effects::apply_effects_lazy(
            target.entity,
            &rolled_specs,
            caster,
            target_magic,
            commands,
        );
    }
}

/// Player-facing name for a modifier in combat chat, falling back to a generic
/// word when the modifier carries no label.
fn enchantment_label(m: &ItemModifier) -> &str {
    if m.label.is_empty() {
        "enchantment"
    } else {
        &m.label
    }
}

/// Drain charge-consumption requests recorded by `resolve_battle_turn` and
/// decrement the matching `Charges` modifier on each attacker's equipped
/// weapon, removing it at zero. Deferred out of the battle loop because that
/// loop iterates a read-only combatant snapshot and cannot hold a mutable
/// `Inventory` borrow. Mutating `Inventory` here replicates via
/// `InventoryChanged`. Runs only when there is something to drain, so it never
/// spuriously dirties the component.
pub fn apply_pending_modifier_consumption(
    mut pending: ResMut<PendingModifierConsumption>,
    mut inventory_query: Query<&mut Inventory, With<Player>>,
) {
    if pending.spent.is_empty() {
        return;
    }
    for (entity, type_ex) in pending.spent.drain(..) {
        let Ok(mut inventory) = inventory_query.get_mut(entity) else {
            continue;
        };
        let Some(weapon) =
            inventory.equipment_item_mut(crate::world::object_definitions::EquipmentSlot::Weapon)
        else {
            continue;
        };
        weapon.modifiers.retain_mut(|m| {
            if m.type_ex != type_ex {
                return true;
            }
            match &mut m.duration {
                ModifierDuration::Charges { remaining } => {
                    *remaining = remaining.saturating_sub(1);
                    *remaining > 0
                }
                _ => true,
            }
        });
    }
}

/// Drain `PendingRetaliations`: for each player in Auto-Retaliate mode who was
/// attacked this battle tick and has no `CombatTarget`, lock one attacker
/// (picked at random when several attacked) as their target. Players with an
/// existing target — manual or from an earlier retaliation — are untouched, so
/// the player's own choice always wins. Runs after `resolve_battle_turn` and
/// before the projection collects events, so the inserted target replicates
/// via `CombatTargetChanged` the same frame. Next tick
/// `clear_invalid_combat_targets` re-validates it (death, space change,
/// `CombatLeash`), which re-arms auto-lock for the next attacker.
pub fn apply_auto_retaliation(
    mut pending: ResMut<crate::combat::resources::PendingRetaliations>,
    mut player_query: Query<
        (Has<CombatTarget>, &mut ChatLog),
        (With<Player>, With<crate::player::components::AutoRetaliate>),
    >,
    vitals_query: Query<&VitalStats>,
    mut commands: Commands,
) {
    if pending.items.is_empty() {
        return;
    }
    // Group by player, deduping attackers that committed several attacks
    // (e.g. a swing and a spell) in the same tick.
    let mut by_player: HashMap<Entity, Vec<(Entity, String)>> = HashMap::new();
    for hit in std::mem::take(&mut pending.items) {
        let attackers = by_player.entry(hit.player).or_default();
        if !attackers.iter().any(|(entity, _)| *entity == hit.attacker) {
            attackers.push((hit.attacker, hit.attacker_name));
        }
    }
    for (player, mut attackers) in by_player {
        // Not auto-retaliating (or despawned): the query filter rejects them.
        let Ok((has_target, mut chat_log)) = player_query.get_mut(player) else {
            continue;
        };
        if has_target {
            continue;
        }
        // An attacker may have died to the player's own swing this very tick.
        attackers.retain(|(attacker, _)| vitals_query.get(*attacker).is_ok_and(|v| v.health > 0.0));
        if attackers.is_empty() {
            continue;
        }
        let pick = if attackers.len() == 1 {
            0
        } else {
            (crate::combat::damage_expr::roll_die(attackers.len(), player.to_bits()) - 1) as usize
        };
        let (attacker, name) = &attackers[pick];
        commands
            .entity(player)
            .insert(CombatTarget { entity: *attacker });
        chat_log.push_narrator(format!("You turn to face {name}."));
    }
}

/// Drain `PendingNpcSummons`, spawning each boss's adds.
///
/// Separate from `resolve_battle_turn` because spawning needs
/// `ResMut<ObjectRegistry>` and the definitions, which that system (already at
/// the 16-param cap, and holding the registry immutably) cannot supply. The
/// summons are tagged `MonsterSide` with `owner_player: None` — the
/// monster-owned branch `Companion` was built for.
pub fn apply_pending_npc_summons(
    mut pending: ResMut<crate::combat::resources::PendingNpcSummons>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    companion_query: Query<(Entity, &Companion), With<crate::npc::components::Npc>>,
    mut commands: Commands,
) {
    if pending.requests.is_empty() {
        return;
    }
    for request in pending.requests.drain(..) {
        crate::game::systems::spawn_summoned_creature(
            &mut commands,
            &definitions,
            &mut object_registry,
            &companion_query,
            &request.spec,
            request.space_id,
            request.tile,
            request.caster,
            None,
            crate::npc::components::Faction::MonsterSide,
        );
    }
}

/// Apply a single NPC spell cast: emit damage events, apply buffs to target
/// and caster, restore caster HP/mana for self-casts, broadcast VFX and
/// chat. Mirrors the player cast handler's effect-application surface but
/// goes through the NPC-side primitives so player class/level/scroll gates
/// don't interfere.
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
        Query<&mut crate::player::components::Exertion, With<Player>>,
    )>,
    ui_events: &mut PendingGameUiEvents,
    pending_damage: &mut PendingDamageEvents,
    pending_summons: &mut crate::combat::resources::PendingNpcSummons,
    chat_log_query: &mut ScopedChatLogQuery,
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
        attacker.attributes,
        attacker.level,
        target.entity,
        &target.name,
        target.position,
    );

    for vfx in &outcome.vfx {
        ui_events.push_broadcast_near(attacker.space_id, attacker.position, vfx.clone());
    }
    for damage in &outcome.damage_events {
        pending_damage.push(damage.clone());
    }
    for msg in &outcome.chat_messages {
        push_chat_line_near(chat_log_query, attacker.space_id, attacker.position, msg);
    }

    // Resolve a tile-AoE splash against everyone standing in the blast. The
    // builder is world-free by design, so entity resolution happens here.
    // Only players take the splash: an NPC's AoE must not shred its own adds
    // (a boss that summons into its own fireball would be unplayable), and
    // player-side companions are spared for the same "no monster friendly
    // fire" reason.
    if let Some(splash) = &outcome.aoe_splash {
        let entities = combat_queries.p0();
        for (entity, _, _, space, position, _, _, _, _, player_identity, _, _, _, _, _) in
            entities.iter()
        {
            if player_identity.is_none() || space.space_id != attacker.space_id {
                continue;
            }
            if position.z != splash.center.z {
                continue;
            }
            let dx = (position.x - splash.center.x).abs();
            let dy = (position.y - splash.center.y).abs();
            if dx.max(dy) > splash.radius_tiles {
                continue;
            }
            pending_damage.push(DamageEvent {
                target: entity,
                amount: splash.amount,
                source: DamageSource::Npc {
                    entity: attacker.entity,
                },
                damage_type: splash.damage_type,
                vfx_override: splash.vfx_override.clone(),
                attacker: Some(attacker.entity),
            });
        }
    }

    // Adds. Deferred: spawning needs `ResMut<ObjectRegistry>` that
    // `resolve_battle_turn` (already at the system-param cap) doesn't hold.
    if let Some(plan) = &outcome.summon {
        pending_summons
            .requests
            .push(crate::combat::resources::NpcSummonRequest {
                caster: attacker.entity,
                space_id: attacker.space_id,
                tile: plan.tile,
                spec: plan.spec.clone(),
            });
    }

    // Mutate the attacker (self-cast heal + self-buffs) and the target
    // (target-buffs). For self-cast spells, attacker == target.
    // `apply_self_outcome` and `apply_target_buffs` handle the lazy-attach
    // path internally via `apply_effects_lazy`, so NPCs spawned without a
    // `MagicEffects` component still pick one up the first time a buff
    // lands.
    {
        let mut entities_query = combat_queries.p1();
        if matches!(target_kind, NpcSpellTargetKind::SelfCast) {
            if let Ok((mut vitals, mut magic)) = entities_query.get_mut(attacker.entity) {
                apply_self_outcome(
                    &outcome,
                    attacker.entity,
                    &mut vitals,
                    magic.as_deref_mut(),
                    commands,
                );
            }
        } else {
            // Apply target buffs first (separate query borrow).
            if !outcome.target_buffs.is_empty() {
                if let Ok((_, mut magic)) = entities_query.get_mut(target.entity) {
                    apply_target_buffs(&outcome, target.entity, magic.as_deref_mut(), commands);
                }
            }
            // Then apply self-buffs / clears on the caster.
            if !outcome.self_buffs.is_empty() || !outcome.self_clears.is_empty() {
                if let Ok((mut vitals, mut magic)) = entities_query.get_mut(attacker.entity) {
                    apply_self_outcome(
                        &outcome,
                        attacker.entity,
                        &mut vitals,
                        magic.as_deref_mut(),
                        commands,
                    );
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

/// Player chat-log query widened with position so combat lines can be scoped
/// to bystanders who can actually see the fight.
pub(crate) type ScopedChatLogQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static SpaceResident,
        &'static TilePosition,
        &'static mut ChatLog,
    ),
    With<Player>,
>;

/// Combat log lines reach the same audience as replicated entities: if you
/// can see the fight, you get the text. Matches the projection's
/// `INTEREST_RADIUS`, not the tighter `say` chat radius.
pub(crate) const COMBAT_LOG_RADIUS_TILES: i32 = crate::game::projection::INTEREST_RADIUS as i32;

/// Push a combat line into the chat log of every player in `space_id` within
/// [`COMBAT_LOG_RADIUS_TILES`] of `origin` (z-aware, so fights on distant
/// floors of the same space stay quiet too).
pub(crate) fn push_chat_line_near(
    chat_log_query: &mut ScopedChatLogQuery,
    space_id: crate::world::components::SpaceId,
    origin: TilePosition,
    message: &str,
) {
    for (resident, tile, mut chat_log) in chat_log_query.iter_mut() {
        if resident.space_id != space_id {
            continue;
        }
        if tile_distance_3d(origin, *tile) > COMBAT_LOG_RADIUS_TILES {
            continue;
        }
        chat_log.push_line(message.to_owned());
    }
}

pub(crate) fn is_target_in_range(
    attack_kind: AttackKind,
    attacker_position: &TilePosition,
    target_position: &TilePosition,
) -> bool {
    if attacker_position == target_position {
        return false;
    }
    let dx = (attacker_position.x - target_position.x).abs();
    let dy = (attacker_position.y - target_position.y).abs();
    let dz = (attacker_position.z - target_position.z).abs();
    let xy = dx.max(dy);
    match attack_kind {
        // Melee reaches one tile in XY and at most one half-block in Z. A
        // player on a half-block ledge is still in range of a goblin standing
        // next to it; a player on the floor above (dz=2) is not.
        AttackKind::Melee => xy <= 1 && dz <= 1,
        // Ranged counts Z as XY: dz=2 (one full floor) equals one tile of
        // horizontal distance. Verticality is part of the range budget.
        AttackKind::Ranged { range_tiles } => xy.max(dz) <= range_tiles,
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

/// Chebyshev distance treating each half-block of Z as one tile. Used by the
/// combat leash check (so an NPC chasing a player who climbs a ledge doesn't
/// instantly drop target) and by chat radius. See `tile_distance_3d` in
/// `world::components`.
pub(crate) fn chebyshev_distance(a: &TilePosition, b: &TilePosition) -> i32 {
    tile_distance_3d(*a, *b)
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
            owner_player: None,
            ranged_projectile_sprite: None,
            armor,
            block,
            dodge_bonus,
            block_chance_pct,
            has_shield,
            level,
            // Default to the creature ¾ track; tests that need a player/full
            // track override `attacker.bab_track` directly. At level 1 every
            // track yields bab 0, so the elevation/range tests are unaffected.
            bab_track: BabTrack::ThreeQuarter,
            class: None,
            sneaking: false,
            crit_threshold: 20,
            spellcasting: None,
            active_effect_kinds: HashSet::new(),
            weapon_modifiers: Vec::new(),
        }
    }

    #[test]
    fn clear_invalid_combat_targets_drops_dead_targets() {
        use crate::world::components::SpaceId;

        let mut app = App::new();
        app.add_systems(Update, clear_invalid_combat_targets);
        let space = SpaceId(0);

        let dead = app
            .world_mut()
            .spawn((
                SpaceResident { space_id: space },
                TilePosition::ground(5, 5),
                VitalStats::full(10.0, 0.0),
            ))
            .id();
        app.world_mut().get_mut::<VitalStats>(dead).unwrap().health = 0.0;
        let alive = app
            .world_mut()
            .spawn((
                SpaceResident { space_id: space },
                TilePosition::ground(6, 5),
                VitalStats::full(10.0, 0.0),
            ))
            .id();

        let attacker_on_dead = app
            .world_mut()
            .spawn((
                SpaceResident { space_id: space },
                TilePosition::ground(5, 6),
                CombatTarget { entity: dead },
            ))
            .id();
        let attacker_on_alive = app
            .world_mut()
            .spawn((
                SpaceResident { space_id: space },
                TilePosition::ground(6, 6),
                CombatTarget { entity: alive },
            ))
            .id();
        // A de-spatialized target (dead player past the death frame, spatial
        // components removed by handle_player_deaths).
        let despatialized = app.world_mut().spawn(VitalStats::full(10.0, 0.0)).id();
        let attacker_on_despatialized = app
            .world_mut()
            .spawn((
                SpaceResident { space_id: space },
                TilePosition::ground(4, 6),
                CombatTarget {
                    entity: despatialized,
                },
            ))
            .id();

        app.update();

        assert!(
            app.world().get::<CombatTarget>(attacker_on_dead).is_none(),
            "a target at 0 HP must be dropped"
        );
        assert!(
            app.world().get::<CombatTarget>(attacker_on_alive).is_some(),
            "a live same-space target must be kept"
        );
        assert!(
            app.world()
                .get::<CombatTarget>(attacker_on_despatialized)
                .is_none(),
            "a de-spatialized target must be dropped"
        );
    }

    #[test]
    fn combat_chat_lines_reach_only_nearby_same_space_players() {
        use crate::player::components::ChatLog;
        use crate::world::components::SpaceId;

        let mut app = App::new();
        app.add_systems(Update, |mut query: ScopedChatLogQuery| {
            push_chat_line_near(
                &mut query,
                SpaceId(0),
                TilePosition::ground(10, 10),
                "[test combat line]",
            );
        });

        let spawn = |app: &mut App, space: SpaceId, tile: TilePosition| {
            app.world_mut()
                .spawn((
                    crate::player::components::Player,
                    ChatLog::default(),
                    SpaceResident { space_id: space },
                    tile,
                ))
                .id()
        };
        let near = spawn(&mut app, SpaceId(0), TilePosition::ground(15, 12));
        let one_floor_up = spawn(&mut app, SpaceId(0), TilePosition { x: 10, y: 10, z: 2 });
        let far = spawn(&mut app, SpaceId(0), TilePosition::ground(80, 10));
        let far_z = spawn(
            &mut app,
            SpaceId(0),
            TilePosition {
                x: 10,
                y: 10,
                z: 40,
            },
        );
        let other_space = spawn(&mut app, SpaceId(1), TilePosition::ground(10, 10));

        app.update();

        let lines = |app: &App, entity: Entity| {
            app.world()
                .entity(entity)
                .get::<ChatLog>()
                .unwrap()
                .lines
                .clone()
        };
        let has_line = |app: &App, entity: Entity| {
            lines(app, entity)
                .iter()
                .any(|line| line == "[test combat line]")
        };
        assert!(has_line(&app, near));
        // One floor up is within the radius: nearby vertical neighbors still
        // hear the fight (only distant floors are pruned).
        assert!(has_line(&app, one_floor_up));
        assert!(!has_line(&app, far));
        assert!(!has_line(&app, far_z));
        assert!(!has_line(&app, other_space));
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
    fn dodge_dc_scales_with_level() {
        // L8 → (3·8)/4 = +6; AGI 14 → +2; +1 item → DC 19.
        let target = snapshot(10, 14, 8, true, 0, 0, 0, 1, false);
        assert_eq!(dodge_dc(&target), 19);
    }

    #[test]
    fn attack_roll_total_player_adds_full_bab() {
        // Player STR 14 → +2 mod, Full track (Fighter) at level 5 → bab 5.
        // Roll is d20 + 2 + 5, in [8, 27]. Melee, so elevation is irrelevant.
        let mut attacker = snapshot(14, 10, 5, true, 0, 0, 0, 0, false);
        attacker.bab_track = BabTrack::Full;
        let target = snapshot(10, 10, 1, false, 0, 0, 0, 0, false);
        for salt in 0..30 {
            let (_, total) = attack_roll_total(&attacker, &target, salt);
            assert!(
                (8..=27).contains(&total),
                "player attack {total} out of [8,27] (salt={salt})"
            );
        }
    }

    #[test]
    fn attack_roll_total_npc_adds_three_quarter_bab() {
        // NPC level 6, STR 12 → +1 mod, default ¾ track → bab_at(¾,6) = 4.
        // Roll is d20 + 1 + 4, in [6, 25] — the capped replacement for raw +6.
        let attacker = snapshot(12, 10, 6, false, 0, 0, 0, 0, false);
        let target = snapshot(10, 10, 1, true, 0, 0, 0, 0, false);
        for salt in 0..30 {
            let (_, total) = attack_roll_total(&attacker, &target, salt);
            assert!(
                (6..=25).contains(&total),
                "npc attack {total} out of [6,25] (salt={salt})"
            );
        }
    }

    #[test]
    fn ranged_attack_roll_adds_elevation_bonus() {
        // AGI 10 → +0 mod. Ranged shooter standing 2 half-blocks above target:
        // d20 + 0 + 2 (elevation) in [3, 22]. Player so no level bonus.
        let mut attacker = snapshot(10, 10, 1, true, 0, 0, 0, 0, false);
        attacker.attack_profile = AttackProfile {
            kind: AttackKind::Ranged { range_tiles: 5 },
            damage_type: DamageType::Pierce,
        };
        attacker.position = TilePosition::new(0, 0, 2);
        let target = snapshot(10, 10, 1, false, 0, 0, 0, 0, false);
        for salt in 0..30 {
            let (_, total) = attack_roll_total(&attacker, &target, salt);
            assert!(
                (3..=22).contains(&total),
                "ranged attack {total} out of [3,22] (salt={salt}) — elevation bonus +2"
            );
        }
    }

    #[test]
    fn ranged_attack_roll_subtracts_when_shooting_up() {
        // Shooter on ground, target two half-blocks up. Should subtract 2.
        // AGI 10 → +0 mod. d20 + 0 - 2 in [-1, 18].
        let mut attacker = snapshot(10, 10, 1, true, 0, 0, 0, 0, false);
        attacker.attack_profile = AttackProfile {
            kind: AttackKind::Ranged { range_tiles: 5 },
            damage_type: DamageType::Pierce,
        };
        attacker.position = TilePosition::new(0, 0, 0);
        let mut target = snapshot(10, 10, 1, false, 0, 0, 0, 0, false);
        target.position = TilePosition::new(0, 0, 2);
        for salt in 0..30 {
            let (_, total) = attack_roll_total(&attacker, &target, salt);
            assert!(
                (-1..=18).contains(&total),
                "ranged-upward attack {total} out of [-1, 18] (salt={salt})"
            );
        }
    }

    #[test]
    fn melee_attack_roll_ignores_elevation() {
        // Melee attacker 2 half-blocks above target — elevation must not
        // apply. STR 10 → +0 mod. d20 + 0 in [1, 20].
        let mut attacker = snapshot(10, 10, 1, true, 0, 0, 0, 0, false);
        attacker.position = TilePosition::new(0, 0, 2);
        let target = snapshot(10, 10, 1, false, 0, 0, 0, 0, false);
        for salt in 0..30 {
            let (_, total) = attack_roll_total(&attacker, &target, salt);
            assert!(
                (1..=20).contains(&total),
                "melee attack {total} out of [1, 20] (salt={salt}) — elevation must be ignored"
            );
        }
    }

    // ── apply_auto_retaliation ──────────────────────────────────────────────

    use crate::combat::resources::{PendingRetaliations, RetaliationHit};
    use crate::player::components::AutoRetaliate;

    fn retaliation_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<PendingRetaliations>();
        app.add_systems(Update, apply_auto_retaliation);
        app
    }

    fn spawn_attacker(app: &mut App, health: f32) -> Entity {
        app.world_mut()
            .spawn(VitalStats {
                health,
                max_health: 10.0,
                mana: 0.0,
                max_mana: 0.0,
            })
            .id()
    }

    fn push_hit(app: &mut App, player: Entity, attacker: Entity) {
        app.world_mut()
            .resource_mut::<PendingRetaliations>()
            .items
            .push(RetaliationHit {
                player,
                attacker,
                attacker_name: "Rat".to_owned(),
            });
    }

    #[test]
    fn auto_retaliation_locks_an_attacker() {
        let mut app = retaliation_app();
        let player = app
            .world_mut()
            .spawn((Player, AutoRetaliate, ChatLog::default()))
            .id();
        let attacker = spawn_attacker(&mut app, 10.0);
        push_hit(&mut app, player, attacker);
        app.update();

        let target = app.world().get::<CombatTarget>(player);
        assert_eq!(target.map(|t| t.entity), Some(attacker));
        let chat = app.world().get::<ChatLog>(player).unwrap();
        assert!(
            chat.lines.last().unwrap().contains("turn to face"),
            "expected narrator line, got {:?}",
            chat.lines.last()
        );
        assert!(
            app.world()
                .resource::<PendingRetaliations>()
                .items
                .is_empty(),
            "queue must be drained"
        );
    }

    #[test]
    fn auto_retaliation_never_overrides_an_existing_target() {
        let mut app = retaliation_app();
        let manual_target = spawn_attacker(&mut app, 10.0);
        let player = app
            .world_mut()
            .spawn((
                Player,
                AutoRetaliate,
                ChatLog::default(),
                CombatTarget {
                    entity: manual_target,
                },
            ))
            .id();
        let attacker = spawn_attacker(&mut app, 10.0);
        push_hit(&mut app, player, attacker);
        app.update();

        let target = app.world().get::<CombatTarget>(player).unwrap();
        assert_eq!(target.entity, manual_target);
    }

    #[test]
    fn auto_retaliation_requires_the_stance_marker() {
        let mut app = retaliation_app();
        let player = app.world_mut().spawn((Player, ChatLog::default())).id();
        let attacker = spawn_attacker(&mut app, 10.0);
        push_hit(&mut app, player, attacker);
        app.update();

        assert!(app.world().get::<CombatTarget>(player).is_none());
    }

    #[test]
    fn auto_retaliation_ignores_dead_attackers() {
        let mut app = retaliation_app();
        let player = app
            .world_mut()
            .spawn((Player, AutoRetaliate, ChatLog::default()))
            .id();
        let dead = spawn_attacker(&mut app, 0.0);
        push_hit(&mut app, player, dead);
        app.update();

        assert!(app.world().get::<CombatTarget>(player).is_none());
        assert!(app
            .world()
            .resource::<PendingRetaliations>()
            .items
            .is_empty());
    }

    #[test]
    fn auto_retaliation_picks_one_of_several_attackers() {
        let mut app = retaliation_app();
        let player = app
            .world_mut()
            .spawn((Player, AutoRetaliate, ChatLog::default()))
            .id();
        let a = spawn_attacker(&mut app, 10.0);
        let b = spawn_attacker(&mut app, 10.0);
        push_hit(&mut app, player, a);
        push_hit(&mut app, player, b);
        // Duplicate record of the same attacker must not skew or break the pick.
        push_hit(&mut app, player, a);
        app.update();

        let target = app.world().get::<CombatTarget>(player).unwrap().entity;
        assert!(target == a || target == b, "picked a queued attacker");
    }
}
