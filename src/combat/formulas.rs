use crate::combat::components::AttackKind;
use crate::combat::damage_expr::DamageExpr;
use crate::player::classes::ability_mod;
use crate::player::components::AttributeSet;

/// Dodge DC = 10 + AGI_mod + sum(item.dodge_bonus). Armor and shield do NOT
/// contribute to the DC — they mitigate damage post-hit (see
/// `docs/progression.md` §7.2 refinement note).
pub fn dodge_dc(agi: i32, dodge_bonus: i32) -> i32 {
    10 + ability_mod(agi) + dodge_bonus
}

/// Probability (as a percentage 0-95) that a shield block triggers after a
/// confirmed hit: shield's raw `block_chance` + AGI_mod * 2, then clamped so
/// a hit always has at least a 5% chance to land its damage roll.
pub fn effective_block_chance_pct(raw_chance: i32, agi: i32) -> i32 {
    (raw_chance + ability_mod(agi) * 2).clamp(0, 95)
}

/// The flat to-hit modifier added to the d20 attack roll: ability_mod for the
/// weapon-relevant ability (STR for melee, AGI for ranged), plus the
/// combatant's level when they're an NPC. Players currently get no BAB — see
/// `docs/progression.md` §7.1 (BAB lands in a later progression batch).
pub fn attack_to_hit_bonus(
    kind: AttackKind,
    attrs: AttributeSet,
    is_player: bool,
    level: u32,
) -> i32 {
    let ability = match kind {
        AttackKind::Ranged { .. } => attrs.agility,
        AttackKind::Melee => attrs.strength,
    };
    let level_bonus = if is_player { 0 } else { level as i32 };
    ability_mod(ability) + level_bonus
}

/// Smallest and largest possible damage rolls for the given weapon expression
/// at the given attributes (excludes the to-hit roll and any post-hit
/// mitigation).
pub fn weapon_damage_range(expr: &DamageExpr, attrs: AttributeSet) -> (i32, i32) {
    (expr.min_damage(&attrs), expr.max_damage(&attrs))
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
    fn dodge_dc_baseline_ten_at_no_agi_no_items() {
        // AGI 10 → ability_mod 0, no dodge_bonus → DC 10.
        assert_eq!(dodge_dc(10, 0), 10);
    }

    #[test]
    fn dodge_dc_adds_agi_mod_and_item_bonus() {
        // AGI 14 → +2; +1 dodge_bonus from boots → DC 13.
        assert_eq!(dodge_dc(14, 1), 13);
    }

    #[test]
    fn dodge_dc_can_go_below_ten_with_agi_penalty() {
        // AGI 6 → -2; no items → DC 8.
        assert_eq!(dodge_dc(6, 0), 8);
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
    fn player_to_hit_skips_level_bonus() {
        // Melee uses STR. STR 14 → +2. Player, so level ignored.
        assert_eq!(
            attack_to_hit_bonus(AttackKind::Melee, attrs(14, 10), true, 5),
            2
        );
    }

    #[test]
    fn npc_to_hit_adds_level() {
        // Ranged uses AGI. AGI 12 → +1. NPC level 3 → +3 → total +4.
        assert_eq!(
            attack_to_hit_bonus(
                AttackKind::Ranged { range_tiles: 4 },
                attrs(10, 12),
                false,
                3
            ),
            4
        );
    }

    #[test]
    fn weapon_damage_range_for_default_melee_1d6_plus_str_over_5() {
        let expr = DamageExpr::melee_default();
        // STR 10 → STR/5 = 2. 1d6 → [1,6]. Range: [3, 8].
        assert_eq!(weapon_damage_range(&expr, attrs(10, 10)), (3, 8));
    }

    #[test]
    fn weapon_damage_range_handles_no_dice() {
        let expr = DamageExpr::parse("STR/2 + 4").unwrap();
        // No dice. STR 12 → 6, bonus 4 → both min and max are 10.
        assert_eq!(weapon_damage_range(&expr, attrs(12, 10)), (10, 10));
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
