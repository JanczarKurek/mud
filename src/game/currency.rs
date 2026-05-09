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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates() {
        assert_eq!(COPPER_PER_GOLD, 240);
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
