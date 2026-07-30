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
use crate::player::components::{AttributeSet, VitalStats};
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
    /// Set for a tile-targeted cast that carries an `aoe` block and rolls
    /// damage. The builder is deliberately world-free, so it cannot resolve
    /// who is standing in the blast — it describes the splash and the caller
    /// (`execute_npc_spell_cast`) fans it out over the entities it can see.
    pub aoe_splash: Option<NpcAoeSplash>,
    /// Set when the spell carries a `summons_creature` block. Queued through
    /// `PendingNpcSummons` because spawning needs mutable registry access the
    /// battle-turn system doesn't hold.
    pub summon: Option<NpcSummonPlan>,
}

/// A tile-centred damage splash awaiting entity resolution by the caller.
pub struct NpcAoeSplash {
    pub center: TilePosition,
    pub radius_tiles: i32,
    pub amount: f32,
    pub damage_type: crate::combat::damage_type::DamageType,
    pub vfx_override: Option<String>,
}

/// Where and what an NPC cast wants to summon.
pub struct NpcSummonPlan {
    pub spec: crate::magic::resources::SummonSpec,
    pub tile: TilePosition,
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
    attacker_attributes: AttributeSet,
    attacker_level: u32,
    target_entity: Entity,
    target_name: &str,
    target_tile: TilePosition,
) -> NpcCastOutcome {
    let mut outcome = NpcCastOutcome::default();

    let damage_source = DamageSource::Npc {
        entity: attacker_entity,
    };
    let damage_type = spell.effects.effective_damage_type();
    // Roll once here against the caster's stats so projectile/AoE/direct paths
    // all use the same number for this cast.
    let damage = spell
        .effects
        .damage
        .roll(&attacker_attributes, attacker_level);

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
            // YAML authoring bug. Heal amounts and buff magnitudes resolve
            // against the caster's own attributes/level.
            outcome.self_restore_health = spell
                .effects
                .restore_health
                .resolve(&attacker_attributes, attacker_level);
            outcome.self_restore_mana = spell
                .effects
                .restore_mana
                .resolve(&attacker_attributes, attacker_level);
            for spec in &spell.effects.buffs_self {
                outcome
                    .self_buffs
                    .push(spec.resolve(&attacker_attributes, attacker_level));
            }
            for kind in &spell.effects.clears_self {
                outcome.self_clears.push(*kind);
            }
        }
        NpcSpellTargetKind::Target => {
            if damage > 0.0 {
                outcome.damage_events.push(DamageEvent {
                    target: target_entity,
                    amount: damage,
                    source: damage_source,
                    damage_type,
                    vfx_override: spell.effects.vfx_on_target_hit.clone(),
                });
            }
            for spec in &spell.effects.buffs_target {
                outcome
                    .target_buffs
                    .push(spec.resolve(&attacker_attributes, attacker_level));
            }
            for spec in &spell.effects.buffs_self {
                outcome
                    .self_buffs
                    .push(spec.resolve(&attacker_attributes, attacker_level));
            }
            for kind in &spell.effects.clears_self {
                outcome.self_clears.push(*kind);
            }
        }
        NpcSpellTargetKind::TargetTile => {
            // Tile-target AoE. Per-tile VFX play over the full radius here;
            // the damage splash is *described* rather than resolved, because
            // this builder has no world access. `execute_npc_spell_cast`
            // turns `aoe_splash` into one `DamageEvent` per victim standing
            // in the blast. A tile-targeted spell with no `aoe` block still
            // falls back to hitting only the primary target.
            let mut splashed = false;
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
                if damage > 0.0 {
                    outcome.aoe_splash = Some(NpcAoeSplash {
                        center: target_tile,
                        radius_tiles: radius,
                        amount: damage,
                        damage_type,
                        vfx_override: spell.effects.vfx_on_target_hit.clone(),
                    });
                    splashed = true;
                }
            }
            if damage > 0.0 && !splashed {
                outcome.damage_events.push(DamageEvent {
                    target: target_entity,
                    amount: damage,
                    source: damage_source,
                    damage_type,
                    vfx_override: spell.effects.vfx_on_target_hit.clone(),
                });
            }
            for spec in &spell.effects.buffs_target {
                outcome
                    .target_buffs
                    .push(spec.resolve(&attacker_attributes, attacker_level));
            }
            for spec in &spell.effects.buffs_self {
                outcome
                    .self_buffs
                    .push(spec.resolve(&attacker_attributes, attacker_level));
            }
            for kind in &spell.effects.clears_self {
                outcome.self_clears.push(*kind);
            }
        }
    }

    // Summons land on the cast tile — the caster's own tile for a self-cast,
    // otherwise the target tile, so a boss can drop adds either on itself or
    // on top of whoever it is fighting.
    if let Some(spec) = spell.effects.summons_creature.as_ref() {
        let tile = match target_kind {
            NpcSpellTargetKind::SelfCast => attacker_tile,
            _ => target_tile,
        };
        outcome.summon = Some(NpcSummonPlan {
            spec: spec.clone(),
            tile,
        });
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
    use crate::combat::damage_expr::DamageExpr;
    use crate::magic::resources::{
        AoeSpec, SpellDamage, SpellDefinition, SpellEffects, SpellTargeting, SummonSpec,
    };
    use crate::npc::spellcasting::NpcSpellEntry;

    fn spell_with(effects: SpellEffects) -> SpellDefinition {
        SpellDefinition {
            name: "Test Spell".to_owned(),
            incantation: "test".to_owned(),
            mana_cost: 0.0,
            targeting: SpellTargeting::TargetedTile,
            range_tiles: 6,
            class_access: Vec::new(),
            min_caster_level: 0,
            effects,
        }
    }

    /// Run the builder with fixed caster/target geometry.
    fn build(spell: &SpellDefinition, kind: NpcSpellTargetKind) -> NpcCastOutcome {
        build_npc_cast_outcome(
            spell,
            kind,
            Entity::PLACEHOLDER,
            "Boss",
            SpaceId(1),
            TilePosition::new(1, 1, 0),
            AttributeSet::default(),
            10,
            Entity::PLACEHOLDER,
            "Victim",
            TilePosition::new(5, 5, 0),
        )
    }

    #[test]
    fn tile_aoe_emits_a_splash_for_the_caller_to_resolve() {
        let spell = spell_with(SpellEffects {
            damage: SpellDamage(Some(DamageExpr::parse("4d6+10").unwrap())),
            aoe: Some(AoeSpec {
                radius_tiles: 3,
                vfx_on_tile: None,
                pattern: Default::default(),
            }),
            ..Default::default()
        });
        let outcome = build(&spell, NpcSpellTargetKind::TargetTile);

        // The splash replaces the single-target event: the caller fans it out
        // over everyone standing in the blast.
        assert!(outcome.damage_events.is_empty());
        let splash = outcome.aoe_splash.expect("tile AoE should emit a splash");
        assert_eq!(splash.radius_tiles, 3);
        assert_eq!(splash.center, TilePosition::new(5, 5, 0));
        assert!(splash.amount > 0.0);
    }

    #[test]
    fn tile_cast_without_an_aoe_block_still_hits_only_the_target() {
        let spell = spell_with(SpellEffects {
            damage: SpellDamage(Some(DamageExpr::parse("4d6+10").unwrap())),
            ..Default::default()
        });
        let outcome = build(&spell, NpcSpellTargetKind::TargetTile);

        assert!(outcome.aoe_splash.is_none());
        assert_eq!(outcome.damage_events.len(), 1);
    }

    #[test]
    fn single_target_cast_never_splashes() {
        let spell = spell_with(SpellEffects {
            damage: SpellDamage(Some(DamageExpr::parse("2d6").unwrap())),
            aoe: Some(AoeSpec {
                radius_tiles: 4,
                vfx_on_tile: None,
                pattern: Default::default(),
            }),
            ..Default::default()
        });
        let outcome = build(&spell, NpcSpellTargetKind::Target);

        assert!(outcome.aoe_splash.is_none());
        assert_eq!(outcome.damage_events.len(), 1);
    }

    #[test]
    fn summon_plan_lands_on_the_target_tile_and_on_self_for_selfcast() {
        let spell = spell_with(SpellEffects {
            summons_creature: Some(SummonSpec {
                type_id: "tallow_drip".to_owned(),
                lifetime_seconds: 45.0,
                count: 3,
                follow_close_tiles: 2,
            }),
            ..Default::default()
        });

        let at_target = build(&spell, NpcSpellTargetKind::TargetTile)
            .summon
            .expect("summons_creature should produce a plan");
        assert_eq!(at_target.tile, TilePosition::new(5, 5, 0));
        assert_eq!(at_target.spec.count, 3);
        assert_eq!(at_target.spec.type_id, "tallow_drip");

        let on_self = build(&spell, NpcSpellTargetKind::SelfCast)
            .summon
            .expect("self-cast summons should also produce a plan");
        assert_eq!(on_self.tile, TilePosition::new(1, 1, 0));
    }

    #[test]
    fn no_summon_plan_when_the_spell_does_not_summon() {
        let spell = spell_with(SpellEffects::default());
        assert!(build(&spell, NpcSpellTargetKind::TargetTile)
            .summon
            .is_none());
    }

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
