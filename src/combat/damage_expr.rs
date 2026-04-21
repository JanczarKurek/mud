use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::player::components::AttributeSet;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatTerm {
    pub kind: AttributeKind,
    pub multiplier: i32,
    pub divisor: i32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DamageExpr {
    pub dice: Option<(u32, u32)>,
    pub stats: Vec<StatTerm>,
    pub bonus: i32,
}

impl Default for DamageExpr {
    fn default() -> Self {
        Self::melee_default()
    }
}

impl DamageExpr {
    pub fn melee_default() -> Self {
        Self {
            dice: Some((1, 6)),
            stats: vec![StatTerm {
                kind: AttributeKind::Strength,
                multiplier: 1,
                divisor: 5,
            }],
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

            let Some(kind) = AttributeKind::parse(stat_part) else {
                return Err(format!("unrecognized term '{term}' in '{raw}'"));
            };
            stats.push(StatTerm {
                kind,
                multiplier,
                divisor,
            });
        }

        Ok(Self { dice, stats, bonus })
    }

    pub fn roll(&self, attrs: &AttributeSet) -> i32 {
        let dice_total = match self.dice {
            Some((count, sides)) if count > 0 && sides > 0 => {
                let mut total = 0i32;
                for i in 0..count {
                    total = total.saturating_add(roll_die(sides as usize, i as u64));
                }
                total
            }
            _ => 0,
        };
        let stat_total: i32 = self
            .stats
            .iter()
            .map(|term| {
                let raw = term.kind.value_of(attrs).saturating_mul(term.multiplier);
                if term.divisor == 0 {
                    0
                } else {
                    raw / term.divisor
                }
            })
            .sum();
        dice_total
            .saturating_add(stat_total)
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
    fn parses_melee_default() {
        let expr = DamageExpr::parse("1d6+strength/5").unwrap();
        assert_eq!(expr.dice, Some((1, 6)));
        assert_eq!(expr.stats.len(), 1);
        assert_eq!(expr.stats[0].kind, AttributeKind::Strength);
        assert_eq!(expr.stats[0].divisor, 5);
        assert_eq!(expr.stats[0].multiplier, 1);
        assert_eq!(expr.bonus, 0);
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
        let value = expr.roll(&attrs());
        assert!(value >= 1 + 10 && value <= 6 + 10);
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
        assert_eq!(expr.roll(&attrs), 50 + 12 * 5);
    }

    #[test]
    fn roll_honors_divisor() {
        let expr = DamageExpr {
            dice: None,
            stats: vec![StatTerm {
                kind: AttributeKind::Strength,
                multiplier: 1,
                divisor: 5,
            }],
            bonus: 0,
        };
        assert_eq!(expr.roll(&attrs()), 2);
    }
}
