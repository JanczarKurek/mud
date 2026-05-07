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

use crate::player::components::{DerivedStats, Player, RegenBuffs, RegenTickers, VitalStats};

/// Base health regen interval (seconds per HP) at constitution = 0.
/// Plug into the actual formula: `60 / (2 + constitution / 5)`.
fn health_interval_seconds(derived: &DerivedStats, multiplier: f32) -> f32 {
    let constitution = derived.attributes.constitution.max(0) as f32;
    let per_minute = 2.0 + constitution / 5.0;
    let effective = (per_minute * multiplier).max(0.001);
    60.0 / effective
}

fn mana_interval_seconds(derived: &DerivedStats, multiplier: f32) -> f32 {
    let willpower = derived.attributes.willpower.max(0) as f32;
    let per_minute = 2.0 + willpower / 5.0;
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
        ),
        With<Player>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for (mut vitals, mut tickers, derived, buffs) in query.iter_mut() {
        if vitals.health <= 0.0 {
            continue;
        }

        let multiplier = buffs.map_or(1.0, |b| if b.is_active() { b.multiplier } else { 1.0 });

        if vitals.health < vitals.max_health {
            tickers.health_remaining -= dt;
            while tickers.health_remaining <= 0.0 {
                vitals.health = (vitals.health + 1.0).min(vitals.max_health);
                tickers.health_remaining += health_interval_seconds(derived, multiplier);
                if vitals.health >= vitals.max_health {
                    tickers.health_remaining = health_interval_seconds(derived, multiplier);
                    break;
                }
            }
        } else {
            // Reset accumulator so the first tick after damage isn't instant.
            tickers.health_remaining = health_interval_seconds(derived, multiplier);
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
}
