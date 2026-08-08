//! Slow stat-driven HP/MP regeneration plus food/drink rate buffs.
//!
//! Two server-side systems run every Update:
//! - `tick_regen_buffs` decays the active `RegenBuffs.remaining_seconds`.
//! - `tick_vital_regen` decrements per-player accumulators and adds 1 HP / 1 MP
//!   each time the corresponding interval elapses.
//!
//! Both must be gated by `simulation_active` (per the project-wide rule for
//! server-side simulation systems). They mutate authoritative components only;
//! the resulting HP/MP changes replicate to the client via the existing
//! `PlayerVitalsChanged` diff in `compute_events_for_peer`.

use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::npc::components::LastDamagedAt;
use crate::player::components::{
    DerivedStats, Exertion, Player, RegenBuffs, RegenTickers, VitalStats,
};
use crate::player::exertion::exertion_regen_multiplier;
use crate::player::skills::{Skill, SkillSheet};

/// Per-rank regen speed-up from Endurance (`utility_systems.md` §6.1). Rank 0 →
/// ×1.0; each rank adds 4%, so a maxed Endurance (~13 ranks at L10) → ~×1.52.
const ENDURANCE_REGEN_PER_RANK: f32 = 0.04;

/// HP-regen rate multiplier while the player counts as in combat. `[tunable]`
/// — CON 10 goes from the 15 s/HP base to 10 s/HP.
const IN_COMBAT_HP_REGEN_MULTIPLIER: f32 = 1.5;

/// HP-regen rate multiplier while out of combat. `[tunable]` — CON 10 → 5 s/HP
/// (12 HP/min), so downtime between fights stays short without touching the
/// balance model's stand-and-trade duels (which ignore mid-fight regen; see
/// `tools/balance/README.md`).
const OUT_OF_COMBAT_HP_REGEN_MULTIPLIER: f32 = 3.0;

/// How long after the last hit taken a player still counts as in combat.
const COMBAT_RECENCY_WINDOW_SECONDS: f32 = 8.0;

/// Combat-state HP multiplier: a player is in combat while they hold an attack
/// lock (`CombatTarget`) or took a hit within the recency window. Applies to
/// health only — mana regen is tuned for sustained casting and unaffected.
fn combat_hp_regen_multiplier(
    has_combat_target: bool,
    last_damaged_at: Option<&LastDamagedAt>,
    now: f32,
) -> f32 {
    let hurt_recently = last_damaged_at.is_some_and(|t| now - t.0 <= COMBAT_RECENCY_WINDOW_SECONDS);
    if has_combat_target || hurt_recently {
        IN_COMBAT_HP_REGEN_MULTIPLIER
    } else {
        OUT_OF_COMBAT_HP_REGEN_MULTIPLIER
    }
}

/// Regen-rate multiplier from the player's Endurance rank.
fn endurance_regen_multiplier(sheet: Option<&SkillSheet>) -> f32 {
    let rank = sheet.map_or(0, |s| s.rank(Skill::Endurance)) as f32;
    1.0 + rank * ENDURANCE_REGEN_PER_RANK
}

/// Base health regen interval (seconds per HP) at constitution = 0.
/// Plug into the actual formula: `60 / (2 + constitution / 5)`.
fn health_interval_seconds(derived: &DerivedStats, multiplier: f32) -> f32 {
    let constitution = derived.attributes.constitution.max(0) as f32;
    let per_minute = 2.0 + constitution / 5.0;
    let effective = (per_minute * multiplier).max(0.001);
    60.0 / effective
}

/// Mana regen interval (seconds per MP): `60 / (2 + willpower + focus/2)`
/// per minute. `[tunable]` — the target metric is *sustained casting*: a
/// dedicated caster (WIL 16 / FOC 16 → ~26 MP/min ≈ 2.3 s/MP) keeps a 4-mana
/// cantrip flowing every ~10 s and affords a 12-mana nuke every ~28 s,
/// instead of the old ~7/min trickle that made every fight a one-shot mana
/// bar (`docs/balance/report.md` §2.5).
fn mana_interval_seconds(derived: &DerivedStats, multiplier: f32) -> f32 {
    let willpower = derived.attributes.willpower.max(0) as f32;
    let focus = derived.attributes.focus.max(0) as f32;
    let per_minute = 2.0 + willpower + focus / 2.0;
    let effective = (per_minute * multiplier).max(0.001);
    60.0 / effective
}

/// Decrement active food/drink buff timers. When the buff expires, snap the
/// multiplier back to 1.0 so it stops affecting `tick_vital_regen`. The
/// resulting state change replicates to the client via the projection diff
/// for `regen_buff` (see `compute_events_for_peer`).
pub fn tick_regen_buffs(time: Res<Time>, mut query: Query<&mut RegenBuffs, With<Player>>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for mut buffs in query.iter_mut() {
        if buffs.remaining_seconds <= 0.0 {
            continue;
        }
        buffs.remaining_seconds -= dt;
        if buffs.remaining_seconds <= 0.0 {
            buffs.remaining_seconds = 0.0;
            buffs.multiplier = 1.0;
        }
    }
}

/// Tick HP/MP regen accumulators. While `RegenBuffs::is_active()` the rate is
/// multiplied by `buffs.multiplier`. Skip ticking entirely for dead players
/// (`health <= 0`) — death/respawn is owned by `handle_player_deaths`.
pub fn tick_vital_regen(
    time: Res<Time>,
    mut query: Query<
        (
            &mut VitalStats,
            &mut RegenTickers,
            &DerivedStats,
            Option<&RegenBuffs>,
            Option<&SkillSheet>,
            Option<&Exertion>,
            Has<CombatTarget>,
            Option<&LastDamagedAt>,
        ),
        With<Player>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let now = time.elapsed_secs();

    for (mut vitals, mut tickers, derived, buffs, sheet, exertion, in_combat, last_damaged) in
        query.iter_mut()
    {
        if vitals.health <= 0.0 {
            continue;
        }

        // Compose the three regen modifiers into one rate scalar: food/drink
        // buff × Endurance speed-up × exertion penalty. Floored so regen never
        // fully stops.
        let food = buffs.map_or(1.0, |b| if b.is_active() { b.multiplier } else { 1.0 });
        let endurance = endurance_regen_multiplier(sheet);
        let fatigue = exertion_regen_multiplier(exertion);
        let multiplier = (food * endurance * fatigue).max(0.05);
        // Health additionally scales with combat state; mana keeps the base
        // composed multiplier so sustained-casting tuning is untouched.
        let hp_multiplier = multiplier * combat_hp_regen_multiplier(in_combat, last_damaged, now);

        if vitals.health < vitals.max_health {
            tickers.health_remaining -= dt;
            while tickers.health_remaining <= 0.0 {
                vitals.health = (vitals.health + 1.0).min(vitals.max_health);
                tickers.health_remaining += health_interval_seconds(derived, hp_multiplier);
                if vitals.health >= vitals.max_health {
                    tickers.health_remaining = health_interval_seconds(derived, hp_multiplier);
                    break;
                }
            }
        } else {
            // Reset accumulator so the first tick after damage isn't instant.
            tickers.health_remaining = health_interval_seconds(derived, hp_multiplier);
        }

        if vitals.max_mana > 0.0 && vitals.mana < vitals.max_mana {
            tickers.mana_remaining -= dt;
            while tickers.mana_remaining <= 0.0 {
                vitals.mana = (vitals.mana + 1.0).min(vitals.max_mana);
                tickers.mana_remaining += mana_interval_seconds(derived, multiplier);
                if vitals.mana >= vitals.max_mana {
                    tickers.mana_remaining = mana_interval_seconds(derived, multiplier);
                    break;
                }
            }
        } else {
            tickers.mana_remaining = mana_interval_seconds(derived, multiplier);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::{AttributeSet, BaseStats};

    fn derived_with(con: i32, will: i32) -> DerivedStats {
        let base = BaseStats {
            attributes: AttributeSet::new(10, 10, con, will, 10, 10),
            ..BaseStats::default()
        };
        DerivedStats::from_base(&base)
    }

    #[test]
    fn buff_extends_remaining_seconds() {
        // Re-eating a 60s food while 30s remain should yield 90s remaining.
        let mut buffs = RegenBuffs {
            multiplier: 2.0,
            remaining_seconds: 30.0,
        };
        let new_duration: f32 = 60.0;
        let new_multiplier: f32 = 2.0;

        buffs.remaining_seconds += new_duration;
        buffs.multiplier = buffs.multiplier.max(new_multiplier);

        assert!((buffs.remaining_seconds - 90.0).abs() < f32::EPSILON);
        assert!((buffs.multiplier - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn weaker_buff_does_not_reduce_active_multiplier() {
        // While a 3x buff is active, eating a 2x food extends time but
        // doesn't dilute the multiplier.
        let mut buffs = RegenBuffs {
            multiplier: 3.0,
            remaining_seconds: 20.0,
        };
        let new_duration: f32 = 60.0;
        let new_multiplier: f32 = 2.0;

        buffs.remaining_seconds += new_duration;
        buffs.multiplier = buffs.multiplier.max(new_multiplier);

        assert!((buffs.remaining_seconds - 80.0).abs() < f32::EPSILON);
        assert!((buffs.multiplier - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn baseline_health_interval_at_con_10_is_15_seconds() {
        let derived = derived_with(10, 10);
        let interval = health_interval_seconds(&derived, 1.0);
        // 2 + 10/5 = 4 per minute → 60/4 = 15s per HP.
        assert!((interval - 15.0).abs() < 0.01, "interval was {interval}");
    }

    #[test]
    fn buff_multiplier_halves_interval() {
        let derived = derived_with(10, 10);
        let baseline = health_interval_seconds(&derived, 1.0);
        let buffed = health_interval_seconds(&derived, 2.0);
        assert!((buffed - baseline / 2.0).abs() < 0.01);
    }

    #[test]
    fn higher_constitution_speeds_regen() {
        let baseline = health_interval_seconds(&derived_with(10, 10), 1.0);
        let stronger = health_interval_seconds(&derived_with(20, 10), 1.0);
        assert!(
            stronger < baseline,
            "expected con=20 ({stronger}) to be faster than con=10 ({baseline})"
        );
    }

    #[test]
    fn willpower_drives_mana_not_health() {
        let derived = derived_with(10, 30);
        let h = health_interval_seconds(&derived, 1.0);
        let m = mana_interval_seconds(&derived, 1.0);
        assert!(m < h);
    }

    #[test]
    fn endurance_speeds_regen() {
        use crate::player::skills::{Skill, SkillSheet};
        let mut sheet = SkillSheet::default();
        assert!((endurance_regen_multiplier(Some(&sheet)) - 1.0).abs() < f32::EPSILON);
        sheet.set_rank(Skill::Endurance, 10);
        let m10 = endurance_regen_multiplier(Some(&sheet));
        assert!(m10 > 1.0);
        // A higher multiplier shortens the regen interval (faster regen).
        let derived = derived_with(10, 10);
        assert!(health_interval_seconds(&derived, m10) < health_interval_seconds(&derived, 1.0));
    }

    #[test]
    fn combat_state_selects_hp_multiplier() {
        // Out of combat: no target, never damaged.
        assert_eq!(
            combat_hp_regen_multiplier(false, None, 100.0),
            OUT_OF_COMBAT_HP_REGEN_MULTIPLIER
        );
        // Holding an attack lock counts as in combat regardless of damage.
        assert_eq!(
            combat_hp_regen_multiplier(true, None, 100.0),
            IN_COMBAT_HP_REGEN_MULTIPLIER
        );
        // Hit inside the recency window: in combat.
        let recent = LastDamagedAt(95.0);
        assert_eq!(
            combat_hp_regen_multiplier(false, Some(&recent), 100.0),
            IN_COMBAT_HP_REGEN_MULTIPLIER
        );
        // Stale hit: back to the out-of-combat rate.
        let stale = LastDamagedAt(100.0 - COMBAT_RECENCY_WINDOW_SECONDS - 0.1);
        assert_eq!(
            combat_hp_regen_multiplier(false, Some(&stale), 100.0),
            OUT_OF_COMBAT_HP_REGEN_MULTIPLIER
        );
    }

    #[test]
    fn combat_multipliers_shorten_the_base_interval() {
        let derived = derived_with(10, 10);
        let base = health_interval_seconds(&derived, 1.0);
        let in_combat = health_interval_seconds(&derived, IN_COMBAT_HP_REGEN_MULTIPLIER);
        let out_of_combat = health_interval_seconds(&derived, OUT_OF_COMBAT_HP_REGEN_MULTIPLIER);
        // CON 10 anchors: 15 s/HP base → 10 s in combat, 5 s out of combat.
        assert!((base - 15.0).abs() < 0.01, "base was {base}");
        assert!((in_combat - 10.0).abs() < 0.01, "in_combat was {in_combat}");
        assert!(
            (out_of_combat - 5.0).abs() < 0.01,
            "out_of_combat was {out_of_combat}"
        );
    }

    #[test]
    fn composed_multiplier_respects_floor() {
        use crate::player::components::{Exertion, EXERTION_BASE_MAX};
        // Worst case: no food, no endurance, a full exertion meter. The fatigue
        // penalty bottoms at EXERTION_REGEN_FLOOR (0.5), so the composed rate is
        // still well above the 0.05 floor — regen slows but never stalls.
        let exhausted = Exertion {
            current: EXERTION_BASE_MAX,
            max: EXERTION_BASE_MAX,
        };
        let fatigue = crate::player::exertion::exertion_regen_multiplier(Some(&exhausted));
        let composed = (1.0_f32 * 1.0 * fatigue).max(0.05);
        assert!((0.05..1.0).contains(&composed));
    }
}
