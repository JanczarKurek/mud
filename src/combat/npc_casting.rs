//! NPC spell-casting evaluation and application.
//!
//! Hooked into `combat::systems::resolve_battle_turn`: when an NPC's turn
//! comes up and it has a `SpellcastingProfile`, we walk the spell list in
//! declaration order and execute the first entry whose cooldown is ready and
//! whose `NpcSpellCondition`s all pass. A cast replaces the physical attack
//! for that turn — the NPC either casts OR melees/shoots, never both.
//!
//! Damage flows through the same `PendingDamageEvents` pipeline as every
//! other damage source. Buffs/debuffs land on the target's `MagicEffects`
//! exactly like `apply_buffs_target` in the player path. VFX broadcast over
//! `PendingGameUiEvents` so EmbeddedClient and TcpClient render identically.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::combat::damage::{DamageEvent, DamageSource};
use crate::game::resources::{GameUiEvent, VfxAnchor};
use crate::magic::effects::{apply_effects_lazy, MagicEffects};
use crate::magic::resources::{EffectKind, SpellDefinition};
use crate::npc::spellcasting::{NpcSpellCondition, NpcSpellEntry, NpcSpellTargetKind};
use crate::player::components::VitalStats;
use crate::world::components::{tile_distance_3d, SpaceId, TilePosition};

/// Read-only snapshot of an NPC caster + its target, supplied to
/// `pick_npc_spell`.
pub struct NpcCastContext<'a> {
    pub now_seconds: f32,
    pub attacker_position: TilePosition,
    pub attacker_health: f32,
    pub attacker_max_health: f32,
    pub attacker_active_effects: &'a HashSet<EffectKind>,
    pub target_position: TilePosition,
    pub target_health: f32,
    pub target_max_health: f32,
    pub target_active_effects: &'a HashSet<EffectKind>,
}

/// Walk `entries` in declaration order, returning the index of the first
/// spell whose cooldown is ready and whose conditions all pass. `None` =
/// fall back to physical attack.
pub fn pick_npc_spell(entries: &[NpcSpellEntry], ctx: &NpcCastContext) -> Option<usize> {
    for (idx, entry) in entries.iter().enumerate() {
        let elapsed = ctx.now_seconds - entry.last_cast_at;
        if elapsed < entry.cooldown_seconds {
            continue;
        }
        if !entry
            .conditions
            .iter()
            .all(|cond| evaluate_condition(*cond, ctx))
        {
            continue;
        }
        return Some(idx);
    }
    None
}

fn evaluate_condition(cond: NpcSpellCondition, ctx: &NpcCastContext) -> bool {
    match cond {
        NpcSpellCondition::TargetWithinRange(n) => {
            chebyshev_distance(ctx.attacker_position, ctx.target_position) <= n.max(0)
        }
        // The AI tick already gates `CombatTarget` on Bresenham LoS via
        // `HostileBehavior::requires_line_of_sight`; if the target is set,
        // the NPC saw it within the last ~1s. Treat as visible — a stricter
        // re-check from inside combat would need to rebuild `BlockerIndex`
        // and is overkill for the cadence.
        NpcSpellCondition::TargetVisible => true,
        NpcSpellCondition::TargetHpBelowFraction(f) => {
            if ctx.target_max_health <= 0.0 {
                return false;
            }
            ctx.target_health / ctx.target_max_health <= f
        }
        NpcSpellCondition::SelfHpBelowFraction(f) => {
            if ctx.attacker_max_health <= 0.0 {
                return false;
            }
            ctx.attacker_health / ctx.attacker_max_health <= f
        }
        NpcSpellCondition::TargetWithoutEffect(kind) => !ctx.target_active_effects.contains(&kind),
        NpcSpellCondition::SelfWithoutEffect(kind) => !ctx.attacker_active_effects.contains(&kind),
    }
}

/// Side-effect payload computed for a single NPC cast. The execution helper
/// returns this; the caller is responsible for applying mutations on the
/// right queries (PendingDamage, target's MagicEffects, attacker's
/// VitalStats, etc.). Decoupling like this keeps `resolve_battle_turn`'s
/// borrow patterns local instead of plumbing a dozen `&mut` everywhere.
#[derive(Default)]
pub struct NpcCastOutcome {
    /// Damage events to enqueue (single-target or AoE-resolved).
    pub damage_events: Vec<DamageEvent>,
    /// Spell-effect specs to apply on the target entity's `MagicEffects`.
    pub target_buffs: Vec<crate::magic::resources::EffectSpec>,
    /// Spell-effect specs to apply on the caster entity's `MagicEffects`.
    pub self_buffs: Vec<crate::magic::resources::EffectSpec>,
    /// Effect kinds to clear from the caster after `self_buffs` apply.
    pub self_clears: Vec<EffectKind>,
    /// HP to restore on the caster (self-heal spells).
    pub self_restore_health: f32,
    /// Mana to restore on the caster.
    pub self_restore_mana: f32,
    /// VFX broadcasts to push to `PendingGameUiEvents`.
    pub vfx: Vec<GameUiEvent>,
    /// Chat-log narration to broadcast.
    pub chat_messages: Vec<String>,
}

/// Build the cast payload for the spell at `spells[spell_idx]`. Returns
/// `None` when the entry references an unknown spell id.
#[allow(clippy::too_many_arguments)]
pub fn build_npc_cast_outcome(
    spell: &SpellDefinition,
    target_kind: NpcSpellTargetKind,
    attacker_entity: Entity,
    attacker_name: &str,
    attacker_space: SpaceId,
    attacker_tile: TilePosition,
    target_entity: Entity,
    target_name: &str,
    target_tile: TilePosition,
) -> NpcCastOutcome {
    let mut outcome = NpcCastOutcome::default();

    let damage_source = DamageSource::Npc {
        entity: attacker_entity,
    };
    let damage_type = spell.effects.effective_damage_type();

    // Cast-time VFX on the caster's tile.
    let cast_vfx_id = spell
        .effects
        .vfx_on_cast
        .clone()
        .unwrap_or_else(|| "cast_flash".to_owned());
    outcome.vfx.push(GameUiEvent::VfxSpawn {
        definition_id: cast_vfx_id,
        anchor: VfxAnchor::tile(attacker_space, attacker_tile),
    });

    let chat = match target_kind {
        NpcSpellTargetKind::SelfCast => {
            format!("[{attacker_name} casts {} on itself]", spell.name)
        }
        _ => format!("[{attacker_name} casts {} on {target_name}]", spell.name),
    };
    outcome.chat_messages.push(chat);

    match target_kind {
        NpcSpellTargetKind::SelfCast => {
            // Untargeted: damage/buffs apply to the caster. We only support
            // healing + self-buffs here; an NPC nuking itself would be a
            // YAML authoring bug.
            outcome.self_restore_health = spell.effects.restore_health;
            outcome.self_restore_mana = spell.effects.restore_mana;
            for spec in &spell.effects.buffs_self {
                outcome.self_buffs.push(*spec);
            }
            for kind in &spell.effects.clears_self {
                outcome.self_clears.push(*kind);
            }
        }
        NpcSpellTargetKind::Target => {
            if spell.effects.damage > 0.0 {
                outcome.damage_events.push(DamageEvent {
                    target: target_entity,
                    amount: spell.effects.damage,
                    source: damage_source,
                    damage_type,
                    vfx_override: spell.effects.vfx_on_target_hit.clone(),
                });
            }
            for spec in &spell.effects.buffs_target {
                outcome.target_buffs.push(*spec);
            }
            for spec in &spell.effects.buffs_self {
                outcome.self_buffs.push(*spec);
            }
            for kind in &spell.effects.clears_self {
                outcome.self_clears.push(*kind);
            }
        }
        NpcSpellTargetKind::TargetTile => {
            // Tile-target AoE — for NPC casts we keep the friendly-fire
            // surface intentionally narrow: only the actual `target_entity`
            // takes damage today. Fanning out to every entity in radius
            // would require a separate world query that's not worth wiring
            // in until enemy mages stand near each other in real content.
            // Per-tile VFX still play over the full radius so the spell
            // looks like an AoE on screen.
            if let Some(aoe) = spell.effects.aoe.as_ref() {
                let radius = aoe.radius_tiles.max(0);
                if let Some(tile_vfx_id) = aoe.vfx_on_tile.as_ref() {
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            let tile = TilePosition::new(
                                target_tile.x + dx,
                                target_tile.y + dy,
                                target_tile.z,
                            );
                            outcome.vfx.push(GameUiEvent::VfxSpawn {
                                definition_id: tile_vfx_id.clone(),
                                anchor: VfxAnchor::tile(attacker_space, tile),
                            });
                        }
                    }
                }
            }
            if spell.effects.damage > 0.0 {
                outcome.damage_events.push(DamageEvent {
                    target: target_entity,
                    amount: spell.effects.damage,
                    source: damage_source,
                    damage_type,
                    vfx_override: spell.effects.vfx_on_target_hit.clone(),
                });
            }
            for spec in &spell.effects.buffs_target {
                outcome.target_buffs.push(*spec);
            }
            for spec in &spell.effects.buffs_self {
                outcome.self_buffs.push(*spec);
            }
            for kind in &spell.effects.clears_self {
                outcome.self_clears.push(*kind);
            }
        }
    }

    outcome
}

/// Apply the parts of `NpcCastOutcome` that target the attacker itself
/// (restore HP/mana, self buffs/clears). Caller drains the other queues
/// (damage, target buffs, VFX) into their respective resources/queries.
///
/// `attacker_effects` is `Option<&mut MagicEffects>` so NPCs that spawn
/// without the component still receive self-buffs: `apply_effects_lazy`
/// inserts a fresh component via `Commands` on the next flush. Clears are
/// skipped when no component exists — there's nothing to remove.
pub fn apply_self_outcome(
    outcome: &NpcCastOutcome,
    attacker_entity: Entity,
    attacker_vitals: &mut VitalStats,
    mut attacker_effects: Option<&mut MagicEffects>,
    commands: &mut Commands,
) {
    attacker_vitals.health = (attacker_vitals.health + outcome.self_restore_health)
        .clamp(0.0, attacker_vitals.max_health);
    attacker_vitals.mana =
        (attacker_vitals.mana + outcome.self_restore_mana).clamp(0.0, attacker_vitals.max_mana);
    if let Some(effects) = attacker_effects.as_deref_mut() {
        for kind in &outcome.self_clears {
            effects.clear(*kind);
        }
    }
    apply_effects_lazy(
        attacker_entity,
        &outcome.self_buffs,
        None,
        attacker_effects,
        commands,
    );
}

/// Apply queued target buffs to the target's `MagicEffects`, lazily
/// attaching the component when missing.
pub fn apply_target_buffs(
    outcome: &NpcCastOutcome,
    target_entity: Entity,
    target_effects: Option<&mut MagicEffects>,
    commands: &mut Commands,
) {
    apply_effects_lazy(
        target_entity,
        &outcome.target_buffs,
        None,
        target_effects,
        commands,
    );
}

/// Build a `HashSet<EffectKind>` of *currently active* effect kinds.
pub fn active_effect_kinds(effects: Option<&MagicEffects>) -> HashSet<EffectKind> {
    let mut set = HashSet::new();
    let Some(effects) = effects else {
        return set;
    };
    for entry in &effects.active {
        set.insert(entry.kind);
    }
    set
}

fn chebyshev_distance(a: TilePosition, b: TilePosition) -> i32 {
    tile_distance_3d(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::npc::spellcasting::NpcSpellEntry;

    fn entry(
        spell_id: &str,
        cooldown: f32,
        last_cast_at: f32,
        target: NpcSpellTargetKind,
        conditions: Vec<NpcSpellCondition>,
    ) -> NpcSpellEntry {
        NpcSpellEntry {
            spell_id: spell_id.to_owned(),
            cooldown_seconds: cooldown,
            last_cast_at,
            target_kind: target,
            conditions,
        }
    }

    fn ctx<'a>(
        self_hp: f32,
        target_hp: f32,
        distance: i32,
        attacker_effects: &'a HashSet<EffectKind>,
        target_effects: &'a HashSet<EffectKind>,
    ) -> NpcCastContext<'a> {
        NpcCastContext {
            now_seconds: 100.0,
            attacker_position: TilePosition::new(0, 0, 0),
            attacker_health: self_hp,
            attacker_max_health: 100.0,
            attacker_active_effects: attacker_effects,
            target_position: TilePosition::new(distance, 0, 0),
            target_health: target_hp,
            target_max_health: 100.0,
            target_active_effects: target_effects,
        }
    }

    #[test]
    fn heal_chosen_first_when_self_hp_low() {
        let empty = HashSet::new();
        let spells = vec![
            entry(
                "goblin_heal",
                25.0,
                f32::NEG_INFINITY,
                NpcSpellTargetKind::SelfCast,
                vec![NpcSpellCondition::SelfHpBelowFraction(0.4)],
            ),
            entry(
                "magic_dart",
                3.0,
                f32::NEG_INFINITY,
                NpcSpellTargetKind::Target,
                vec![NpcSpellCondition::TargetWithinRange(7)],
            ),
        ];
        let c = ctx(30.0, 100.0, 5, &empty, &empty);
        assert_eq!(pick_npc_spell(&spells, &c), Some(0));
    }

    #[test]
    fn heal_skipped_when_self_hp_high_and_fallthrough_picks_filler() {
        let empty = HashSet::new();
        let spells = vec![
            entry(
                "goblin_heal",
                25.0,
                f32::NEG_INFINITY,
                NpcSpellTargetKind::SelfCast,
                vec![NpcSpellCondition::SelfHpBelowFraction(0.4)],
            ),
            entry(
                "magic_dart",
                3.0,
                f32::NEG_INFINITY,
                NpcSpellTargetKind::Target,
                vec![NpcSpellCondition::TargetWithinRange(7)],
            ),
        ];
        let c = ctx(90.0, 100.0, 5, &empty, &empty);
        assert_eq!(pick_npc_spell(&spells, &c), Some(1));
    }

    #[test]
    fn sleep_skipped_when_target_already_asleep() {
        let empty = HashSet::new();
        let mut target_effects = HashSet::new();
        target_effects.insert(EffectKind::Sleep);
        let spells = vec![
            entry(
                "sleep",
                18.0,
                f32::NEG_INFINITY,
                NpcSpellTargetKind::Target,
                vec![
                    NpcSpellCondition::TargetWithinRange(6),
                    NpcSpellCondition::TargetWithoutEffect(EffectKind::Sleep),
                ],
            ),
            entry(
                "magic_dart",
                3.0,
                f32::NEG_INFINITY,
                NpcSpellTargetKind::Target,
                vec![NpcSpellCondition::TargetWithinRange(7)],
            ),
        ];
        let c = ctx(90.0, 100.0, 4, &empty, &target_effects);
        assert_eq!(pick_npc_spell(&spells, &c), Some(1));
    }

    #[test]
    fn cooldown_blocks_selection() {
        let empty = HashSet::new();
        let spells = vec![entry(
            "magic_dart",
            5.0,
            99.0, // last cast 1s ago, cooldown 5s — not yet ready
            NpcSpellTargetKind::Target,
            vec![NpcSpellCondition::TargetWithinRange(7)],
        )];
        let c = ctx(90.0, 100.0, 5, &empty, &empty);
        assert_eq!(pick_npc_spell(&spells, &c), None);
    }

    #[test]
    fn target_out_of_range_skips_spell() {
        let empty = HashSet::new();
        let spells = vec![entry(
            "magic_dart",
            3.0,
            f32::NEG_INFINITY,
            NpcSpellTargetKind::Target,
            vec![NpcSpellCondition::TargetWithinRange(5)],
        )];
        let c = ctx(90.0, 100.0, 9, &empty, &empty);
        assert_eq!(pick_npc_spell(&spells, &c), None);
    }

    #[test]
    fn falls_through_to_none_when_no_spell_matches() {
        let empty = HashSet::new();
        let spells = vec![entry(
            "goblin_heal",
            25.0,
            f32::NEG_INFINITY,
            NpcSpellTargetKind::SelfCast,
            vec![NpcSpellCondition::SelfHpBelowFraction(0.4)],
        )];
        let c = ctx(90.0, 100.0, 5, &empty, &empty);
        assert_eq!(pick_npc_spell(&spells, &c), None);
    }
}
