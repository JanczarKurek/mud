use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::player::classes::ability_mod;
use crate::player::components::AttributeSet;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum AttributeKind {
    Strength,
    Agility,
    Constitution,
    Willpower,
    Charisma,
    Focus,
}

impl AttributeKind {
    fn value_of(self, attrs: &AttributeSet) -> i32 {
        match self {
            AttributeKind::Strength => attrs.strength,
            AttributeKind::Agility => attrs.agility,
            AttributeKind::Constitution => attrs.constitution,
            AttributeKind::Willpower => attrs.willpower,
            AttributeKind::Charisma => attrs.charisma,
            AttributeKind::Focus => attrs.focus,
        }
    }

    fn parse(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "strength" | "str" => Some(Self::Strength),
            "agility" | "agi" => Some(Self::Agility),
            "constitution" | "con" => Some(Self::Constitution),
            "willpower" | "wil" => Some(Self::Willpower),
            "charisma" | "cha" => Some(Self::Charisma),
            "focus" | "foc" => Some(Self::Focus),
            _ => None,
        }
    }
}

/// How a stat term reads its attribute: `Raw` uses the full score (`strength`
/// at STR 16 adds 16), `Mod` uses the d20 ability modifier (`str_mod` at
/// STR 16 adds +3, matching to-hit math). Raw stays the default so existing
/// expressions — creature `hp:` like `2d20+80+constitution*6` in particular —
/// keep their meaning.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StatTermMode {
    #[default]
    Raw,
    Mod,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct StatTerm {
    pub kind: AttributeKind,
    pub multiplier: i32,
    pub divisor: i32,
    /// `#[serde(default)]` so exprs serialized before this field existed load
    /// as `Raw`.
    #[serde(default)]
    pub mode: StatTermMode,
}

/// A term that scales with the caster/wielder's level, e.g. `level`, `level/2`,
/// `level*3`. Spells key damage off caster level with it, and every real
/// weapon carries a `level/2` skill-growth term (tools and fists don't).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct LevelTerm {
    pub multiplier: i32,
    pub divisor: i32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct DamageExpr {
    pub dice: Option<(u32, u32)>,
    pub stats: Vec<StatTerm>,
    /// Level-scaling terms (usually empty). `#[serde(default)]` so existing
    /// serialized exprs without this field still load.
    #[serde(default)]
    pub level: Vec<LevelTerm>,
    pub bonus: i32,
}

impl Default for DamageExpr {
    fn default() -> Self {
        Self::melee_default()
    }
}

impl DamageExpr {
    /// Unarmed / no-`damage:`-field fallback: `1d4 + str_mod`. Fists are the
    /// damage floor — every real weapon carries larger dice.
    pub fn melee_default() -> Self {
        Self {
            dice: Some((1, 4)),
            stats: vec![StatTerm {
                kind: AttributeKind::Strength,
                multiplier: 1,
                divisor: 1,
                mode: StatTermMode::Mod,
            }],
            level: Vec::new(),
            bonus: 0,
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("empty damage expression".to_owned());
        }

        let mut dice: Option<(u32, u32)> = None;
        let mut stats: Vec<StatTerm> = Vec::new();
        let mut level: Vec<LevelTerm> = Vec::new();
        let mut bonus: i32 = 0;

        for raw_term in trimmed.split('+') {
            let term = raw_term.trim();
            if term.is_empty() {
                return Err(format!("empty term in '{raw}'"));
            }
            if let Some((count_s, sides_s)) = split_once_lower(term, 'd') {
                if count_s.chars().all(|c| c.is_ascii_digit())
                    && sides_s.chars().all(|c| c.is_ascii_digit())
                    && !count_s.is_empty()
                    && !sides_s.is_empty()
                {
                    if dice.is_some() {
                        return Err(format!("multiple dice terms in '{raw}'"));
                    }
                    let count: u32 = count_s
                        .parse()
                        .map_err(|e| format!("bad dice count '{count_s}': {e}"))?;
                    let sides: u32 = sides_s
                        .parse()
                        .map_err(|e| format!("bad dice sides '{sides_s}': {e}"))?;
                    if count == 0 || sides == 0 {
                        return Err(format!("dice must be non-zero in '{raw}'"));
                    }
                    dice = Some((count, sides));
                    continue;
                }
            }

            if let Ok(value) = term.parse::<i32>() {
                bonus = bonus.saturating_add(value);
                continue;
            }

            let (stat_part, multiplier, divisor) = if let Some((lhs, rhs)) = term.split_once('*') {
                let mul: i32 = rhs
                    .trim()
                    .parse()
                    .map_err(|e| format!("bad multiplier '{rhs}': {e}"))?;
                (lhs.trim(), mul, 1)
            } else if let Some((lhs, rhs)) = term.split_once('/') {
                let div: i32 = rhs
                    .trim()
                    .parse()
                    .map_err(|e| format!("bad divisor '{rhs}': {e}"))?;
                if div == 0 {
                    return Err(format!("zero divisor in '{raw}'"));
                }
                (lhs.trim(), 1, div)
            } else {
                (term, 1, 1)
            };

            if stat_part.eq_ignore_ascii_case("level") || stat_part.eq_ignore_ascii_case("lvl") {
                level.push(LevelTerm {
                    multiplier,
                    divisor,
                });
                continue;
            }

            // A `_mod` suffix (e.g. `str_mod`, `focus_mod`) switches the term
            // to ability-modifier mode.
            let lower = stat_part.to_ascii_lowercase();
            let (attr_token, mode) = match lower.strip_suffix("_mod") {
                Some(base) => (base, StatTermMode::Mod),
                None => (lower.as_str(), StatTermMode::Raw),
            };
            let Some(kind) = AttributeKind::parse(attr_token) else {
                return Err(format!("unrecognized term '{term}' in '{raw}'"));
            };
            stats.push(StatTerm {
                kind,
                multiplier,
                divisor,
                mode,
            });
        }

        Ok(Self {
            dice,
            stats,
            level,
            bonus,
        })
    }

    /// Smallest possible damage roll for the given attributes + level (every
    /// die shows 1, stat/level terms applied at floor).
    pub fn min_damage(&self, attrs: &AttributeSet, level: i32) -> i32 {
        let dice_total = match self.dice {
            Some((count, _)) => count as i32,
            None => 0,
        };
        dice_total
            .saturating_add(self.stat_total(attrs))
            .saturating_add(self.level_total(level))
            .saturating_add(self.bonus)
    }

    /// Largest possible damage roll for the given attributes + level (every die
    /// at max face).
    pub fn max_damage(&self, attrs: &AttributeSet, level: i32) -> i32 {
        let dice_total = match self.dice {
            Some((count, sides)) => (count as i32).saturating_mul(sides as i32),
            None => 0,
        };
        dice_total
            .saturating_add(self.stat_total(attrs))
            .saturating_add(self.level_total(level))
            .saturating_add(self.bonus)
    }

    fn stat_total(&self, attrs: &AttributeSet) -> i32 {
        self.stats
            .iter()
            .map(|term| {
                let base = match term.mode {
                    StatTermMode::Raw => term.kind.value_of(attrs),
                    StatTermMode::Mod => ability_mod(term.kind.value_of(attrs)),
                };
                let raw = base.saturating_mul(term.multiplier);
                if term.divisor == 0 {
                    0
                } else {
                    raw / term.divisor
                }
            })
            .sum()
    }

    fn level_total(&self, level: i32) -> i32 {
        self.level
            .iter()
            .map(|term| {
                let raw = level.saturating_mul(term.multiplier);
                if term.divisor == 0 {
                    0
                } else {
                    raw / term.divisor
                }
            })
            .sum()
    }

    /// Roll this expression for a combatant with the given attributes and
    /// `level`. `level` only matters when the expression has `level` terms (it
    /// is ignored by every weapon/HP expression today).
    pub fn roll(&self, attrs: &AttributeSet, level: i32) -> i32 {
        self.roll_salted(attrs, level, 0)
    }

    /// Like [`roll`], but with an extra salt folded into every die. Two rolls
    /// of the same expression in the same nanosecond tick share the underlying
    /// time source, so callers that need independent back-to-back rolls (e.g.
    /// the critical-hit double roll) must pass distinct salts.
    pub fn roll_salted(&self, attrs: &AttributeSet, level: i32, salt: u64) -> i32 {
        let dice_total = match self.dice {
            Some((count, sides)) if count > 0 && sides > 0 => {
                let mut total = 0i32;
                for i in 0..count {
                    total = total.saturating_add(roll_die(sides as usize, salt + i as u64));
                }
                total
            }
            _ => 0,
        };
        dice_total
            .saturating_add(self.stat_total(attrs))
            .saturating_add(self.level_total(level))
            .saturating_add(self.bonus)
    }
}

fn split_once_lower(s: &str, sep: char) -> Option<(String, String)> {
    let lower = s.to_ascii_lowercase();
    let (lhs, rhs) = lower.split_once(sep)?;
    Some((lhs.to_owned(), rhs.to_owned()))
}

pub fn roll_die(sides: usize, salt: u64) -> i32 {
    if sides == 0 {
        return 0;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos.wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    ((mixed as usize % sides) + 1) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::components::AttributeSet;

    fn attrs() -> AttributeSet {
        AttributeSet::new(10, 12, 10, 10, 10, 10)
    }

    #[test]
    fn parses_legacy_raw_divisor_expression() {
        let expr = DamageExpr::parse("1d6+strength/5").unwrap();
        assert_eq!(expr.dice, Some((1, 6)));
        assert_eq!(expr.stats.len(), 1);
        assert_eq!(expr.stats[0].kind, AttributeKind::Strength);
        assert_eq!(expr.stats[0].divisor, 5);
        assert_eq!(expr.stats[0].multiplier, 1);
        assert_eq!(expr.bonus, 0);
    }

    #[test]
    fn melee_default_is_unarmed_floor() {
        // 1d4 + str_mod: parse form and constructor agree.
        let expr = DamageExpr::melee_default();
        assert_eq!(expr, DamageExpr::parse("1d4+str_mod").unwrap());
    }

    #[test]
    fn parses_bow_damage() {
        let expr = DamageExpr::parse("1d6+strength").unwrap();
        assert_eq!(expr.dice, Some((1, 6)));
        assert_eq!(expr.stats[0].multiplier, 1);
        assert_eq!(expr.stats[0].divisor, 1);
    }

    #[test]
    fn parses_crossbow_damage() {
        let expr = DamageExpr::parse("2d4+agility").unwrap();
        assert_eq!(expr.dice, Some((2, 4)));
        assert_eq!(expr.stats[0].kind, AttributeKind::Agility);
    }

    #[test]
    fn parses_multiplier_and_bonus() {
        let expr = DamageExpr::parse("1d4+agility*2+3").unwrap();
        assert_eq!(expr.dice, Some((1, 4)));
        assert_eq!(expr.stats[0].multiplier, 2);
        assert_eq!(expr.bonus, 3);
    }

    #[test]
    fn rejects_unknown_stat() {
        assert!(DamageExpr::parse("1d6+luck").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(DamageExpr::parse("").is_err());
        assert!(DamageExpr::parse("1d6++").is_err());
    }

    #[test]
    fn roll_is_positive_for_strength_term() {
        let expr = DamageExpr::parse("1d6+strength").unwrap();
        let value = expr.roll(&attrs(), 1);
        assert!((1 + 10..=6 + 10).contains(&value));
    }

    #[test]
    fn parses_and_rolls_level_term() {
        // A caster-level-scaling spell expression: 1d8 + focus/2 + level/2.
        let expr = DamageExpr::parse("1d8+focus/2+level/2").unwrap();
        assert_eq!(expr.dice, Some((1, 8)));
        assert_eq!(expr.stats[0].kind, AttributeKind::Focus);
        assert_eq!(expr.stats[0].divisor, 2);
        assert_eq!(expr.level.len(), 1);
        assert_eq!(expr.level[0].divisor, 2);
        // FOC 18 -> +9, level 10 -> +5, 1d8 -> [1,8]: roll in [15, 22].
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 18);
        let value = expr.roll(&attrs, 10);
        assert!((1 + 9 + 5..=8 + 9 + 5).contains(&value), "{value}");
    }

    #[test]
    fn level_term_ignored_without_it() {
        // No level term -> the `level` argument has no effect. Use a dice-free
        // expression so the roll is deterministic.
        let expr = DamageExpr::parse("agility+2").unwrap();
        assert_eq!(expr.roll(&attrs(), 1), 12 + 2);
        assert_eq!(expr.roll(&attrs(), 1), expr.roll(&attrs(), 99));
    }

    #[test]
    fn parses_hp_style_expression_without_dice() {
        let expr = DamageExpr::parse("50+constitution*5").unwrap();
        assert_eq!(expr.dice, None);
        assert_eq!(expr.bonus, 50);
        assert_eq!(expr.stats.len(), 1);
        assert_eq!(expr.stats[0].kind, AttributeKind::Constitution);
        assert_eq!(expr.stats[0].multiplier, 5);
        assert_eq!(expr.stats[0].divisor, 1);
        let attrs = AttributeSet::new(10, 10, 12, 10, 10, 10);
        assert_eq!(expr.roll(&attrs, 1), 50 + 12 * 5);
    }

    #[test]
    fn parses_mod_terms_short_and_long() {
        // `str_mod` at STR 16 -> +3 (the d20 ability modifier, not the raw 16).
        let expr = DamageExpr::parse("1d8+str_mod").unwrap();
        assert_eq!(expr.stats[0].kind, AttributeKind::Strength);
        assert_eq!(expr.stats[0].mode, StatTermMode::Mod);
        let attrs = AttributeSet::new(16, 10, 10, 10, 10, 10);
        assert_eq!(expr.min_damage(&attrs, 1), 1 + 3);
        assert_eq!(expr.max_damage(&attrs, 1), 8 + 3);

        let long = DamageExpr::parse("focus_mod*2").unwrap();
        assert_eq!(long.stats[0].kind, AttributeKind::Focus);
        assert_eq!(long.stats[0].mode, StatTermMode::Mod);
        // FOC 14 -> mod +2, *2 -> +4.
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 14);
        assert_eq!(long.roll(&attrs, 1), 4);
    }

    #[test]
    fn mod_term_is_negative_below_ten() {
        // STR 7 -> mod -2 (rounded toward -inf). No dice: deterministic.
        let expr = DamageExpr::parse("str_mod+5").unwrap();
        let attrs = AttributeSet::new(7, 10, 10, 10, 10, 10);
        assert_eq!(expr.roll(&attrs, 1), -2 + 5);
    }

    #[test]
    fn raw_terms_unchanged_by_mode_addition() {
        // The cyclops-style HP expression must keep its raw-score meaning.
        let expr = DamageExpr::parse("2d20+80+constitution*6").unwrap();
        assert_eq!(expr.stats[0].mode, StatTermMode::Raw);
        let attrs = AttributeSet::new(10, 10, 14, 10, 10, 10);
        assert_eq!(expr.min_damage(&attrs, 1), 2 + 80 + 14 * 6);
        assert_eq!(expr.max_damage(&attrs, 1), 40 + 80 + 14 * 6);
    }

    #[test]
    fn stat_term_serde_defaults_to_raw() {
        // Exprs serialized before `mode` existed must deserialize as Raw.
        let json = r#"{"kind":"Strength","multiplier":1,"divisor":5}"#;
        let term: StatTerm = serde_json::from_str(json).unwrap();
        assert_eq!(term.mode, StatTermMode::Raw);
    }

    #[test]
    fn roll_honors_divisor() {
        let expr = DamageExpr {
            dice: None,
            stats: vec![StatTerm {
                kind: AttributeKind::Strength,
                multiplier: 1,
                divisor: 5,
                mode: StatTermMode::Raw,
            }],
            level: Vec::new(),
            bonus: 0,
        };
        assert_eq!(expr.roll(&attrs(), 1), 2);
    }
}
