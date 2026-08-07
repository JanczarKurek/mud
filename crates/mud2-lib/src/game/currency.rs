//! Old-English £sd-style currency arithmetic.
//!
//! Three coin tiers — **copper**, **silver**, **gold** — with rates
//! `1 silver = 12 copper`, `1 gold = 20 silver` (so `1 gold = 240 copper`).
//! Coins are ordinary stack items in inventory; this module is the canonical
//! place to convert between mixed-coin tuples and a single integer "total
//! copper" amount used by vendor / loot math.

pub const COPPER_PER_SILVER: u32 = 12;
pub const SILVER_PER_GOLD: u32 = 20;
pub const COPPER_PER_GOLD: u32 = COPPER_PER_SILVER * SILVER_PER_GOLD;

pub const COPPER_TYPE_ID: &str = "copper_coin";
pub const SILVER_TYPE_ID: &str = "silver_coin";
pub const GOLD_TYPE_ID: &str = "gold_coin";

pub fn total_copper(copper: u32, silver: u32, gold: u32) -> u32 {
    copper
        .saturating_add(silver.saturating_mul(COPPER_PER_SILVER))
        .saturating_add(gold.saturating_mul(COPPER_PER_GOLD))
}

/// Split a copper-denominated amount into the most compact (gold, silver, copper) tuple.
pub fn split(total: u32) -> (u32, u32, u32) {
    let gold = total / COPPER_PER_GOLD;
    let remainder = total % COPPER_PER_GOLD;
    let silver = remainder / COPPER_PER_SILVER;
    let copper = remainder % COPPER_PER_SILVER;
    (gold, silver, copper)
}

/// Render a copper amount as the shortest `"3g 2s 4c"`-style string, omitting
/// zero tiers (`0` → `"0c"`).
pub fn format_compact(total: u32) -> String {
    let (g, s, c) = split(total);
    let mut out = String::new();
    if g > 0 {
        out.push_str(&format!("{}g ", g));
    }
    if s > 0 {
        out.push_str(&format!("{}s ", s));
    }
    if c > 0 || (g == 0 && s == 0) {
        out.push_str(&format!("{}c", c));
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates() {
        assert_eq!(COPPER_PER_GOLD, 240);
    }

    #[test]
    fn format_compact_examples() {
        assert_eq!(format_compact(0), "0c");
        assert_eq!(format_compact(4), "4c");
        assert_eq!(format_compact(12), "1s");
        assert_eq!(format_compact(28), "2s 4c");
        assert_eq!(format_compact(240), "1g");
        assert_eq!(format_compact(720), "3g");
        assert_eq!(format_compact(265), "1g 2s 1c");
    }

    #[test]
    fn total_copper_examples() {
        assert_eq!(total_copper(0, 0, 0), 0);
        assert_eq!(total_copper(1, 0, 0), 1);
        assert_eq!(total_copper(0, 1, 0), 12);
        assert_eq!(total_copper(0, 0, 1), 240);
        assert_eq!(total_copper(13, 1, 1), 265);
    }

    #[test]
    fn split_round_trips() {
        for total in [0, 1, 11, 12, 239, 240, 241, 999, 12345] {
            let (g, s, c) = split(total);
            assert_eq!(total_copper(c, s, g), total, "total={total}");
            assert!(c < COPPER_PER_SILVER, "copper not normalised at {total}");
            assert!(s < SILVER_PER_GOLD, "silver not normalised at {total}");
        }
    }
}
