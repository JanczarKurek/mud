use crate::combat::components::AttackKind;
use crate::combat::damage_expr::DamageExpr;
use crate::player::classes::{ability_mod, bab_at, weapon_focus_bonus, BabTrack, Class};
use crate::player::components::AttributeSet;

/// Dodge DC = 10 + (3·level)/4 + AGI_mod + sum(item.dodge_bonus). One
/// universal rule for players and creatures: the level term answers BAB so
/// same-level melee hit% stays in a ~60–80% band across L1–L20 instead of
/// both sides pinning at the nat-20/1 95% cap by mid-level. Armor and shield
/// do NOT contribute to the DC — they mitigate damage post-hit (see
/// `docs/progression.md` §7.2 refinement note).
pub fn dodge_dc(level: u32, agi: i32, dodge_bonus: i32) -> i32 {
    10 + (3 * level as i32) / 4 + ability_mod(agi) + dodge_bonus
}

/// Probability (as a percentage 0-95) that a shield block triggers after a
/// confirmed hit: shield's raw `block_chance` + AGI_mod * 2, then clamped so
/// a hit always has at least a 5% chance to land its damage roll.
pub fn effective_block_chance_pct(raw_chance: i32, agi: i32) -> i32 {
    (raw_chance + ability_mod(agi) * 2).clamp(0, 95)
}

/// The flat to-hit modifier added to the d20 attack roll: ability_mod for the
/// weapon-relevant ability (STR for melee, AGI for ranged) plus the combatant's
/// Base Attack Bonus for their level and BAB track (`progression.md` §7.1/§7.4),
/// plus the Fighter's melee-only **Weapon Focus** class feature (§3.1) when
/// `class` is `Some(Fighter)`.
///
/// This is symmetric for players and NPCs: a player's track comes from their
/// class (Fighter full, Cleric/Vagabond ¾, Wizard ½); a creature's comes from
/// its YAML `bab_track` (default ¾) and passes `class: None`. It replaces the
/// old raw `+level` NPC term, which scaled without bound and let mid-level
/// creatures auto-hit.
pub fn attack_to_hit_bonus(
    kind: AttackKind,
    attrs: AttributeSet,
    track: BabTrack,
    level: u32,
    class: Option<Class>,
) -> i32 {
    let ability = match kind {
        AttackKind::Ranged { .. } => attrs.agility,
        AttackKind::Melee => attrs.strength,
    };
    let focus = match (kind, class) {
        (AttackKind::Melee, Some(c)) => weapon_focus_bonus(c, level),
        _ => 0,
    };
    ability_mod(ability) + bab_at(track, level) + focus
}

/// Number of Backstab d6 dice a Vagabond rolls at `level` when striking a
/// target unaware of them (`progression.md` §3.4): 1d6 at level 1, +1d6 at
/// each of 4/8/12/16/20 → `1 + level/4` (6d6 at L20).
pub fn backstab_dice(level: u32) -> u32 {
    1 + level / 4
}

/// Flat backstab bonus for a non-Vagabond attacker striking from undetected
/// stealth (`docs/utility_systems.md` §3): any sneak opener stings a little,
/// but the dice belong to the class feature.
pub const BACKSTAB_FLAT_BONUS: i32 = 2;

/// Smallest and largest possible damage rolls for the given weapon expression
/// at the given attributes and wielder level (excludes the to-hit roll and any
/// post-hit mitigation). `level` only matters for expressions with a `level`
/// term.
pub fn weapon_damage_range(expr: &DamageExpr, attrs: AttributeSet, level: i32) -> (i32, i32) {
    (
        expr.min_damage(&attrs, level),
        expr.max_damage(&attrs, level),
    )
}

/// To-hit modifier from elevation difference, applied to **ranged physical**
/// attacks only (melee and spells unaffected). `z` is in half-block units;
/// `+ELEVATION_BONUS_PER_HALF_BLOCK` per half-block the attacker stands above
/// the target, clamped to `±ELEVATION_BONUS_CAP`. Shooting upward incurs a
/// matching penalty.
pub fn elevation_to_hit_mod(attacker_z: i32, target_z: i32) -> i32 {
    const ELEVATION_BONUS_PER_HALF_BLOCK: i32 = 1;
    const ELEVATION_BONUS_CAP: i32 = 3;
    let dz = attacker_z - target_z;
    (dz * ELEVATION_BONUS_PER_HALF_BLOCK).clamp(-ELEVATION_BONUS_CAP, ELEVATION_BONUS_CAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::damage_expr::DamageExpr;

    fn attrs(strength: i32, agility: i32) -> AttributeSet {
        AttributeSet {
            strength,
            agility,
            constitution: 10,
            willpower: 10,
            charisma: 10,
            focus: 10,
        }
    }

    #[test]
    fn dodge_dc_baseline_ten_at_level_one_no_agi_no_items() {
        // L1 → (3·1)/4 = 0; AGI 10 → ability_mod 0; no dodge_bonus → DC 10.
        assert_eq!(dodge_dc(1, 10, 0), 10);
    }

    #[test]
    fn dodge_dc_adds_agi_mod_and_item_bonus() {
        // L1 → +0; AGI 14 → +2; +1 dodge_bonus from boots → DC 13.
        assert_eq!(dodge_dc(1, 14, 1), 13);
    }

    #[test]
    fn dodge_dc_can_go_below_ten_with_agi_penalty() {
        // L1 → +0; AGI 6 → -2; no items → DC 8.
        assert_eq!(dodge_dc(1, 6, 0), 8);
    }

    #[test]
    fn dodge_dc_scales_three_quarters_per_level() {
        // The level term mirrors the default ¾ BAB track: L4 → +3, L8 → +6,
        // L20 → +15 (all at AGI 10, no items).
        assert_eq!(dodge_dc(4, 10, 0), 13);
        assert_eq!(dodge_dc(8, 10, 0), 16);
        assert_eq!(dodge_dc(20, 10, 0), 25);
    }

    #[test]
    fn block_chance_clamps_at_ninety_five() {
        // Raw 90 + AGI 20 (mod +5) * 2 = 100 → clamped to 95.
        assert_eq!(effective_block_chance_pct(90, 20), 95);
    }

    #[test]
    fn block_chance_floors_at_zero_under_agi_penalty() {
        // Raw 0, AGI 6 (mod -2) * 2 = -4 → clamped to 0.
        assert_eq!(effective_block_chance_pct(0, 6), 0);
    }

    #[test]
    fn block_chance_normal_case() {
        // Raw 25, AGI 12 (mod +1) * 2 = 27.
        assert_eq!(effective_block_chance_pct(25, 12), 27);
    }

    #[test]
    fn full_bab_melee_adds_full_level() {
        // Melee uses STR. STR 14 → +2. Full track at level 5 → bab 5. No class.
        assert_eq!(
            attack_to_hit_bonus(AttackKind::Melee, attrs(14, 10), BabTrack::Full, 5, None),
            7
        );
    }

    #[test]
    fn three_quarter_bab_ranged() {
        // Ranged uses AGI. AGI 12 → +1. ¾ track at level 3 → bab 2 → total +3.
        // (This is the default creature track, replacing the old raw +level.)
        assert_eq!(
            attack_to_hit_bonus(
                AttackKind::Ranged { range_tiles: 4 },
                attrs(10, 12),
                BabTrack::ThreeQuarter,
                3,
                None
            ),
            3
        );
    }

    #[test]
    fn half_bab_melee() {
        // Half track (Wizard) at level 8 → bab 4; STR 10 → +0 → total +4.
        // The Wizard class carries no Weapon Focus.
        assert_eq!(
            attack_to_hit_bonus(
                AttackKind::Melee,
                attrs(10, 10),
                BabTrack::Half,
                8,
                Some(Class::Wizard)
            ),
            4
        );
    }

    #[test]
    fn fighter_weapon_focus_applies_to_melee_only() {
        // Fighter L5: STR 14 → +2, full bab 5, Weapon Focus 1 + 5/5 = 2 → +9.
        assert_eq!(
            attack_to_hit_bonus(
                AttackKind::Melee,
                attrs(14, 10),
                BabTrack::Full,
                5,
                Some(Class::Fighter)
            ),
            9
        );
        // Same Fighter shooting a bow: no Weapon Focus (AGI 10 → +0, bab 5).
        assert_eq!(
            attack_to_hit_bonus(
                AttackKind::Ranged { range_tiles: 5 },
                attrs(14, 10),
                BabTrack::Full,
                5,
                Some(Class::Fighter)
            ),
            5
        );
    }

    #[test]
    fn backstab_dice_anchors() {
        // 1d6 at L1, +1d6 at each of 4/8/12/16/20.
        assert_eq!(backstab_dice(1), 1);
        assert_eq!(backstab_dice(3), 1);
        assert_eq!(backstab_dice(4), 2);
        assert_eq!(backstab_dice(8), 3);
        assert_eq!(backstab_dice(12), 4);
        assert_eq!(backstab_dice(16), 5);
        assert_eq!(backstab_dice(20), 6);
    }

    #[test]
    fn weapon_damage_range_for_default_melee_1d4_plus_str_mod() {
        let expr = DamageExpr::melee_default();
        // STR 10 → mod +0. 1d4 → [1,4]. Range: [1, 4].
        assert_eq!(weapon_damage_range(&expr, attrs(10, 10), 1), (1, 4));
        // STR 14 → mod +2. Range: [3, 6].
        assert_eq!(weapon_damage_range(&expr, attrs(14, 10), 1), (3, 6));
    }

    #[test]
    fn weapon_damage_range_handles_no_dice() {
        let expr = DamageExpr::parse("STR/2 + 4").unwrap();
        // No dice. STR 12 → 6, bonus 4 → both min and max are 10.
        assert_eq!(weapon_damage_range(&expr, attrs(12, 10), 1), (10, 10));
    }

    #[test]
    fn elevation_mod_zero_at_same_z() {
        assert_eq!(elevation_to_hit_mod(2, 2), 0);
    }

    #[test]
    fn elevation_mod_positive_when_above_target() {
        // 2 half-blocks above (one full floor) → +2.
        assert_eq!(elevation_to_hit_mod(2, 0), 2);
        // 3 half-blocks above → +3 (one short of the cap).
        assert_eq!(elevation_to_hit_mod(3, 0), 3);
    }

    #[test]
    fn elevation_mod_negative_when_below_target() {
        // Shooting up one half-block → -1.
        assert_eq!(elevation_to_hit_mod(0, 1), -1);
        // Two full floors below → -3 cap.
        assert_eq!(elevation_to_hit_mod(0, 5), -3);
    }

    #[test]
    fn elevation_mod_caps_at_plus_minus_three() {
        // 10 half-blocks up — still +3 (cap).
        assert_eq!(elevation_to_hit_mod(10, 0), 3);
        // 10 half-blocks down — still -3 (cap).
        assert_eq!(elevation_to_hit_mod(0, 10), -3);
    }
}
