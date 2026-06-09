//! Per-instance item modifiers (enchantments).
//!
//! An [`ItemModifier`] is attached to a *specific* item instance (stored in
//! `InventoryStack::modifiers` / `EquippedItem::modifiers`), not to the shared
//! object definition. Modifiers grant on-hit effects, on-hit bonus elemental
//! damage, or a flat stat bonus to the wielder, for a fixed time, a fixed
//! number of successful applications, or permanently.
//!
//! ## Anti-stacking (`type_ex` / `lvl`)
//! To stop permanent enchants from stacking without bound, every modifier
//! carries an exclusivity group key (`type_ex`) and a rank (`lvl`). Within one
//! item, at most one modifier per `type_ex` survives: a stronger `lvl`
//! overrides a weaker one, a weaker one is rejected, and an equal one refreshes
//! the duration. [`apply_modifier`] is the single decision point — every code
//! path that grants a modifier (spells, item use) routes through it.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::damage_expr::roll_die;
use crate::combat::damage_type::DamageType;
use crate::magic::resources::EffectSpec;
use crate::player::components::{AttributeSet, Inventory, Player};

/// A single per-instance modifier on an item.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct ItemModifier {
    /// Exclusivity group. Anti-stacking is scoped per item per `type_ex`.
    pub type_ex: String,
    /// Strength rank within the `type_ex` group. Higher overrides lower.
    pub lvl: i32,
    pub effect: ModifierEffect,
    pub duration: ModifierDuration,
    /// Player-facing label for tooltips / chat (e.g. "Flaming (+1d6 fire)").
    #[serde(default)]
    pub label: String,
}

impl ItemModifier {
    /// One player-facing line for the inspect chat / tooltips:
    /// `"{headline} — {duration}"`. For [`ModifierEffect::OnHit`] the mechanical
    /// [`ModifierEffect::summary`] is always surfaced (a flavor `label` rarely
    /// carries the DoT numbers a player wants), shown in parens after the label.
    /// Other effects prefer the authored `label` when present (it already states
    /// the mechanic), falling back to the summary.
    pub fn inspect_line(&self) -> String {
        let duration = self.duration.describe();
        let headline = match &self.effect {
            ModifierEffect::OnHit { .. } => {
                let mech = self.effect.summary();
                if self.label.is_empty() {
                    mech
                } else {
                    format!("{} ({mech})", self.label)
                }
            }
            _ => {
                if self.label.is_empty() {
                    self.effect.summary()
                } else {
                    self.label.clone()
                }
            }
        };
        format!("{headline} — {duration}")
    }
}

/// What a modifier does.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModifierEffect {
    /// Extra damage on every hit, applied as its own `DamageEvent` so the
    /// element shows its own number and hit VFX. `dice: Some((1, 6))` = +1d6.
    BonusDamage {
        #[serde(default)]
        dice: Option<(u32, u32)>,
        #[serde(default)]
        bonus: i32,
        damage_type: DamageType,
    },
    /// Chance to apply a magical effect to the struck target on hit.
    OnHit {
        /// Probability in `[0, 1]`, rolled per hit.
        chance: f32,
        spec: EffectSpec,
    },
    /// Flat bonus to the wielder's derived attributes / defense while the item
    /// is equipped.
    WielderStats {
        #[serde(default)]
        attributes: AttributeSet,
        #[serde(default)]
        armor: i32,
        #[serde(default)]
        dodge_bonus: i32,
    },
}

/// Format an `f32` as a compact number: no trailing `.0` for whole values
/// (`3.0 → "3"`), one decimal otherwise (`2.5 → "2.5"`).
fn compact_f32(value: f32) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value:.1}")
    }
}

impl ModifierEffect {
    /// One-line mechanical summary of what this effect does, used by the inspect
    /// chat line and the item-details popup. For [`ModifierEffect::OnHit`] with a
    /// DoT spec (poison / burning / chill) the per-second damage and approximate
    /// total are included; non-DoT on-hit effects show just the applied duration.
    pub fn summary(&self) -> String {
        match self {
            ModifierEffect::BonusDamage {
                dice,
                bonus,
                damage_type,
            } => {
                let dice_str = match dice {
                    Some((count, sides)) => format!("{count}d{sides}"),
                    None => String::new(),
                };
                let bonus_str = if *bonus != 0 {
                    format!("{bonus:+}")
                } else {
                    String::new()
                };
                format!("+{dice_str}{bonus_str} {} dmg", damage_type.display_name())
            }
            ModifierEffect::OnHit { chance, spec } => {
                let pct = (chance * 100.0).round() as i32;
                let kind_name = spec.kind.display_name();
                // A DoT spec deals `magnitude` damage per 1s tick (see
                // `magic::effects::DOT_TICK_INTERVAL_SECONDS`), so the total over
                // its lifetime is ~ magnitude * seconds.
                if crate::magic::effects::dot_damage_type(spec.kind).is_some() {
                    let total = (spec.magnitude * spec.seconds).round() as i32;
                    format!(
                        "{pct}% chance: {kind_name} {}/s for {}s (~{total} dmg)",
                        compact_f32(spec.magnitude),
                        compact_f32(spec.seconds),
                    )
                } else {
                    format!(
                        "{pct}% chance: {kind_name} for {}s",
                        compact_f32(spec.seconds)
                    )
                }
            }
            ModifierEffect::WielderStats {
                attributes,
                armor,
                dodge_bonus,
            } => {
                let parts: Vec<String> = [
                    ("STR", attributes.strength),
                    ("AGI", attributes.agility),
                    ("CON", attributes.constitution),
                    ("WIL", attributes.willpower),
                    ("CHA", attributes.charisma),
                    ("FOC", attributes.focus),
                    ("Armor", *armor),
                    ("Dodge", *dodge_bonus),
                ]
                .into_iter()
                .filter(|(_, v)| *v != 0)
                .map(|(name, v)| format!("{v:+} {name}"))
                .collect();
                if parts.is_empty() {
                    "stat bonus".to_owned()
                } else {
                    parts.join(", ")
                }
            }
        }
    }
}

/// How long a modifier lasts.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModifierDuration {
    Permanent,
    /// Wall-clock seconds remaining. Ticked (in whole-second steps) only while
    /// the item is equipped — see [`tick_item_modifiers`].
    Timed {
        remaining_seconds: f32,
    },
    /// Remaining successful applications. Decremented on each application.
    Charges {
        remaining: u32,
    },
}

impl ModifierDuration {
    /// Short player-facing description for tooltips.
    pub fn describe(&self) -> String {
        match self {
            ModifierDuration::Permanent => "permanent".to_owned(),
            ModifierDuration::Timed { remaining_seconds } => {
                format!("{}s", remaining_seconds.max(0.0).ceil() as i64)
            }
            ModifierDuration::Charges { remaining } => format!("{remaining} hits"),
        }
    }
}

/// Outcome of [`apply_modifier`], so callers can emit the right chat line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    /// No modifier of this `type_ex` existed; the incoming one was added.
    Added,
    /// A weaker modifier of this `type_ex` was replaced by the stronger one.
    Upgraded,
    /// An equal-`lvl` modifier already existed; its duration was refreshed.
    Refreshed,
    /// A stronger modifier of this `type_ex` already existed; incoming dropped.
    Rejected,
}

/// Apply `incoming` to `mods`, honoring the per-item `type_ex` / `lvl` rule.
pub fn apply_modifier(mods: &mut Vec<ItemModifier>, incoming: ItemModifier) -> ApplyOutcome {
    if let Some(existing) = mods.iter_mut().find(|m| m.type_ex == incoming.type_ex) {
        if incoming.lvl > existing.lvl {
            *existing = incoming;
            ApplyOutcome::Upgraded
        } else if incoming.lvl == existing.lvl {
            existing.duration = refresh_duration(existing.duration, incoming.duration);
            ApplyOutcome::Refreshed
        } else {
            ApplyOutcome::Rejected
        }
    } else {
        mods.push(incoming);
        ApplyOutcome::Added
    }
}

/// Equal-`lvl` recast: keep the more generous remaining duration. Permanent
/// always wins; otherwise take the larger remaining seconds / charges.
fn refresh_duration(existing: ModifierDuration, incoming: ModifierDuration) -> ModifierDuration {
    use ModifierDuration::*;
    match (existing, incoming) {
        (Permanent, _) | (_, Permanent) => Permanent,
        (
            Timed {
                remaining_seconds: a,
            },
            Timed {
                remaining_seconds: b,
            },
        ) => Timed {
            remaining_seconds: a.max(b),
        },
        (Charges { remaining: a }, Charges { remaining: b }) => Charges {
            remaining: a.max(b),
        },
        // Mixed kinds at equal lvl (authoring should avoid this): prefer the
        // incoming kind's value.
        (_, other) => other,
    }
}

/// Roll a [`ModifierEffect::BonusDamage`] payload. Mirrors `DamageExpr::roll`
/// minus stat terms — bonus damage has a fixed element and no attribute
/// scaling. `salt` varies the per-die jitter so two procs in one tick differ.
pub fn roll_bonus_damage(dice: Option<(u32, u32)>, bonus: i32, salt: u64) -> i32 {
    let dice_total = match dice {
        Some((count, sides)) if count > 0 && sides > 0 => {
            let mut total = 0i32;
            for i in 0..count {
                total = total.saturating_add(roll_die(sides as usize, salt.wrapping_add(i as u64)));
            }
            total
        }
        _ => 0,
    };
    dice_total.saturating_add(bonus)
}

/// Drives whole-second decrements of [`ModifierDuration::Timed`] modifiers.
/// Mirrors `BattleTurnTimer`: counting in 1s steps keeps the stored value
/// stable between ticks so the `Inventory` diff (and thus `InventoryChanged`
/// network traffic) fires at most once per second per player, matching the
/// existing magic-effect / regen-buff cadence.
#[derive(Resource)]
pub struct ItemModifierTickTimer {
    pub remaining_seconds: f32,
}

impl Default for ItemModifierTickTimer {
    fn default() -> Self {
        Self {
            remaining_seconds: 1.0,
        }
    }
}

/// Decrement equipped-item `Timed` modifiers by one second on each tick and
/// drop the expired ones. Equipped-only: a temporary enchant should not burn
/// its timer while the item sits dormant in the backpack. Removal mutates
/// `Inventory`, which replicates via `InventoryChanged` and is re-folded by
/// `refresh_derived_player_stats` (dropping any expired wielder-stat bonus).
pub fn tick_item_modifiers(
    time: Res<Time>,
    mut timer: ResMut<ItemModifierTickTimer>,
    mut query: Query<&mut Inventory, With<Player>>,
) {
    timer.remaining_seconds -= time.delta_secs();
    if timer.remaining_seconds > 0.0 {
        return;
    }
    // Re-arm; clamp so a single very long frame can't bank multiple ticks.
    timer.remaining_seconds = 1.0;

    for mut inventory in &mut query {
        // Only take a mutable (change-detecting) borrow when there is actually
        // a timed modifier to decrement — otherwise `DerefMut` would dirty the
        // component every tick and spam `InventoryChanged`.
        let has_timed = inventory.equipment_slots.iter().any(|(_, equipped)| {
            equipped.as_ref().is_some_and(|item| {
                item.modifiers
                    .iter()
                    .any(|m| matches!(m.duration, ModifierDuration::Timed { .. }))
            })
        });
        if !has_timed {
            continue;
        }
        for (_slot, equipped) in inventory.equipment_slots.iter_mut() {
            let Some(item) = equipped else {
                continue;
            };
            item.modifiers.retain_mut(|m| match &mut m.duration {
                ModifierDuration::Timed { remaining_seconds } => {
                    *remaining_seconds -= 1.0;
                    *remaining_seconds > 0.0
                }
                _ => true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timed(type_ex: &str, lvl: i32, seconds: f32) -> ItemModifier {
        ItemModifier {
            type_ex: type_ex.to_owned(),
            lvl,
            effect: ModifierEffect::BonusDamage {
                dice: Some((1, 6)),
                bonus: 0,
                damage_type: DamageType::Fire,
            },
            duration: ModifierDuration::Timed {
                remaining_seconds: seconds,
            },
            label: String::new(),
        }
    }

    #[test]
    fn adds_into_empty() {
        let mut mods = Vec::new();
        assert_eq!(
            apply_modifier(&mut mods, timed("a", 1, 10.0)),
            ApplyOutcome::Added
        );
        assert_eq!(mods.len(), 1);
    }

    #[test]
    fn weaker_is_rejected_and_leaves_existing_intact() {
        let mut mods = vec![timed("a", 2, 30.0)];
        let before = mods.clone();
        assert_eq!(
            apply_modifier(&mut mods, timed("a", 1, 99.0)),
            ApplyOutcome::Rejected
        );
        assert_eq!(mods, before, "rejecting a +1 must not touch the +2");
    }

    #[test]
    fn stronger_upgrades_and_replaces_effect() {
        let mut mods = vec![timed("a", 1, 30.0)];
        let mut stronger = timed("a", 3, 5.0);
        stronger.effect = ModifierEffect::WielderStats {
            attributes: AttributeSet::new(1, 0, 0, 0, 0, 0),
            armor: 0,
            dodge_bonus: 0,
        };
        assert_eq!(
            apply_modifier(&mut mods, stronger.clone()),
            ApplyOutcome::Upgraded
        );
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].lvl, 3);
        assert_eq!(mods[0].effect, stronger.effect);
    }

    #[test]
    fn equal_refreshes_to_longer_duration_without_stacking() {
        let mut mods = vec![timed("a", 1, 5.0)];
        assert_eq!(
            apply_modifier(&mut mods, timed("a", 1, 12.0)),
            ApplyOutcome::Refreshed
        );
        assert_eq!(mods.len(), 1, "equal lvl must not add a second entry");
        assert_eq!(
            mods[0].duration,
            ModifierDuration::Timed {
                remaining_seconds: 12.0
            }
        );
        // Refreshing with a shorter duration keeps the longer remaining.
        assert_eq!(
            apply_modifier(&mut mods, timed("a", 1, 3.0)),
            ApplyOutcome::Refreshed
        );
        assert_eq!(
            mods[0].duration,
            ModifierDuration::Timed {
                remaining_seconds: 12.0
            }
        );
    }

    #[test]
    fn distinct_type_ex_coexist() {
        let mut mods = Vec::new();
        apply_modifier(&mut mods, timed("fire", 1, 10.0));
        apply_modifier(&mut mods, timed("might", 1, 10.0));
        assert_eq!(mods.len(), 2);
    }

    #[test]
    fn refresh_keeps_permanent() {
        let mut perm = timed("a", 1, 0.0);
        perm.duration = ModifierDuration::Permanent;
        let mut mods = vec![perm];
        apply_modifier(&mut mods, timed("a", 1, 50.0));
        assert_eq!(mods[0].duration, ModifierDuration::Permanent);
    }

    #[test]
    fn roll_bonus_damage_within_dice_range() {
        for salt in 0..50 {
            let v = roll_bonus_damage(Some((1, 6)), 0, salt);
            assert!((1..=6).contains(&v), "got {v}");
        }
    }

    #[test]
    fn roll_bonus_damage_flat_bonus_only() {
        assert_eq!(roll_bonus_damage(None, 2, 7), 2);
        assert_eq!(roll_bonus_damage(None, 0, 7), 0);
    }

    #[test]
    fn serde_round_trips_and_defaults_label() {
        let yaml = r#"
type_ex: weapon_coating
lvl: 1
effect:
  kind: on_hit
  chance: 1.0
  spec:
    kind: poisoned
    magnitude: 3.0
    seconds: 6.0
duration:
  kind: charges
  remaining: 20
"#;
        let m: ItemModifier = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(m.type_ex, "weapon_coating");
        assert!(m.label.is_empty(), "label defaults to empty");
        assert_eq!(m.duration, ModifierDuration::Charges { remaining: 20 });
        // Round-trip.
        let s = serde_yaml::to_string(&m).unwrap();
        let back: ItemModifier = serde_yaml::from_str(&s).unwrap();
        assert_eq!(m, back);
    }

    use crate::magic::resources::{EffectKind, EffectSpec};

    fn poison_spec(magnitude: f32, seconds: f32) -> EffectSpec {
        EffectSpec {
            kind: EffectKind::Poisoned,
            magnitude,
            seconds,
            secondary_magnitude: None,
        }
    }

    #[test]
    fn bonus_damage_summary_formats_dice_and_bonus() {
        let with_bonus = ModifierEffect::BonusDamage {
            dice: Some((1, 6)),
            bonus: 2,
            damage_type: DamageType::Fire,
        };
        assert_eq!(with_bonus.summary(), "+1d6+2 fire dmg");
        let no_bonus = ModifierEffect::BonusDamage {
            dice: Some((1, 6)),
            bonus: 0,
            damage_type: DamageType::Fire,
        };
        assert_eq!(no_bonus.summary(), "+1d6 fire dmg");
    }

    #[test]
    fn dot_on_hit_summary_includes_dps_and_total() {
        let effect = ModifierEffect::OnHit {
            chance: 0.3,
            spec: poison_spec(3.0, 6.0),
        };
        assert_eq!(effect.summary(), "30% chance: poison 3/s for 6s (~18 dmg)");
    }

    #[test]
    fn non_dot_on_hit_summary_shows_duration_only() {
        let effect = ModifierEffect::OnHit {
            chance: 0.5,
            spec: EffectSpec {
                kind: EffectKind::Slow,
                magnitude: 2.0,
                seconds: 4.0,
                secondary_magnitude: None,
            },
        };
        assert_eq!(effect.summary(), "50% chance: slow for 4s");
    }

    #[test]
    fn wielder_stats_summary_lists_nonzero_terms() {
        let effect = ModifierEffect::WielderStats {
            attributes: AttributeSet::new(2, 0, 0, 0, 0, 0),
            armor: 1,
            dodge_bonus: -1,
        };
        assert_eq!(effect.summary(), "+2 STR, +1 Armor, -1 Dodge");
    }

    #[test]
    fn inspect_line_prefers_label_for_bonus_damage() {
        let mut m = timed("flaming", 1, 0.0);
        m.duration = ModifierDuration::Permanent;
        m.label = "Flaming (+1d6 fire)".to_owned();
        assert_eq!(m.inspect_line(), "Flaming (+1d6 fire) — permanent");
    }

    #[test]
    fn inspect_line_falls_back_to_summary_without_label() {
        // `timed` builds a 1d6 fire BonusDamage with an empty label.
        let m = timed("fire", 1, 12.0);
        assert_eq!(m.inspect_line(), "+1d6 fire dmg — 12s");
    }

    #[test]
    fn inspect_line_onhit_surfaces_mechanics_even_with_label() {
        let m = ItemModifier {
            type_ex: "venom".to_owned(),
            lvl: 1,
            effect: ModifierEffect::OnHit {
                chance: 0.3,
                spec: poison_spec(3.0, 6.0),
            },
            duration: ModifierDuration::Charges { remaining: 18 },
            label: "Venom coating".to_owned(),
        };
        assert_eq!(
            m.inspect_line(),
            "Venom coating (30% chance: poison 3/s for 6s (~18 dmg)) — 18 hits"
        );
    }
}
