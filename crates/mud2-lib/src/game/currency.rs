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

/// Every coin tier, largest first — the order change is minted in.
const COIN_TIERS: [(&str, u32); 3] = [
    (GOLD_TYPE_ID, COPPER_PER_GOLD),
    (SILVER_TYPE_ID, COPPER_PER_SILVER),
    (COPPER_TYPE_ID, 1),
];

/// Total copper value of every coin stack in the player's backpack.
pub fn purse_total_copper(inventory: &crate::player::components::Inventory) -> u32 {
    inventory
        .backpack_slots
        .iter()
        .flatten()
        .map(|stack| {
            COIN_TIERS
                .iter()
                .find(|(type_id, _)| *type_id == stack.type_id)
                .map(|(_, worth)| stack.quantity.saturating_mul(*worth))
                .unwrap_or(0)
        })
        .sum()
}

/// Deduct `amount` copper from the purse, breaking large coins into change.
///
/// Implemented by re-minting: coins are fungible, so melting the whole purse
/// down and paying the remainder back in compact denominations is equivalent to
/// making change, and avoids a per-tier borrow/carry dance. The work happens on
/// a clone that is only committed if the change actually fits, so a full
/// backpack can never eat the player's money.
///
/// Returns false (leaving `inventory` untouched) if the purse is short or the
/// change won't fit.
pub fn spend_copper(
    inventory: &mut crate::player::components::Inventory,
    amount: u32,
    definitions: &crate::world::object_definitions::OverworldObjectDefinitions,
) -> bool {
    if amount == 0 {
        return true;
    }
    let total = purse_total_copper(inventory);
    if total < amount {
        return false;
    }

    let mut draft = inventory.clone();
    for slot in draft.backpack_slots.iter_mut() {
        let is_coin = slot
            .as_ref()
            .is_some_and(|stack| COIN_TIERS.iter().any(|(id, _)| *id == stack.type_id));
        if is_coin {
            *slot = None;
        }
    }
    if !deposit_copper(&mut draft, total - amount, definitions) {
        return false;
    }
    *inventory = draft;
    true
}

/// Add `amount` copper to the backpack as the most compact coin mix that fits.
/// Returns false if there aren't enough slots.
///
/// **Callers must pass a draft they are willing to throw away**: a failing
/// call leaves behind whatever coins it managed to place before running out of
/// room. `spend_copper` clones the inventory for exactly this reason, and the
/// shop-trade commit path works on its own snapshot.
pub(crate) fn deposit_copper(
    inventory: &mut crate::player::components::Inventory,
    amount: u32,
    definitions: &crate::world::object_definitions::OverworldObjectDefinitions,
) -> bool {
    let (gold, silver, copper) = split(amount);
    for (type_id, count) in [
        (GOLD_TYPE_ID, gold),
        (SILVER_TYPE_ID, silver),
        (COPPER_TYPE_ID, copper),
    ] {
        if count == 0 {
            continue;
        }
        let max_stack = definitions
            .get(type_id)
            .map(|def| def.max_stack_size.max(1))
            .unwrap_or(1);
        let mut remaining = count;
        // Top up partial stacks first, then claim empty slots.
        for slot in inventory.backpack_slots.iter_mut() {
            if remaining == 0 {
                break;
            }
            let Some(stack) = slot else { continue };
            if stack.type_id != type_id || stack.quantity >= max_stack {
                continue;
            }
            let room = max_stack - stack.quantity;
            let moved = room.min(remaining);
            stack.quantity += moved;
            remaining -= moved;
        }
        for slot in inventory.backpack_slots.iter_mut() {
            if remaining == 0 {
                break;
            }
            if slot.is_some() {
                continue;
            }
            let moved = max_stack.min(remaining);
            *slot = Some(crate::player::components::InventoryStack::item(
                type_id,
                crate::world::map_layout::ObjectProperties::new(),
                moved,
            ));
            remaining -= moved;
        }
        if remaining > 0 {
            return false;
        }
    }
    true
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

    use crate::player::components::{Inventory, InventoryStack};
    use crate::world::object_definitions::OverworldObjectDefinitions;

    /// Coin definitions with the authored `max_stack_size: 100`.
    fn coin_definitions() -> OverworldObjectDefinitions {
        OverworldObjectDefinitions::load_from_disk()
    }

    fn purse(stacks: &[(&str, u32)]) -> Inventory {
        let mut inventory = Inventory::default();
        for (i, (type_id, quantity)) in stacks.iter().enumerate() {
            inventory.backpack_slots[i] = Some(InventoryStack::item(
                *type_id,
                crate::world::map_layout::ObjectProperties::new(),
                *quantity,
            ));
        }
        inventory
    }

    #[test]
    fn purse_total_sums_every_tier_and_ignores_non_coins() {
        let inventory = purse(&[
            (GOLD_TYPE_ID, 1),
            (SILVER_TYPE_ID, 2),
            (COPPER_TYPE_ID, 3),
            ("iron_sword", 1),
        ]);
        assert_eq!(purse_total_copper(&inventory), 240 + 24 + 3);
    }

    #[test]
    fn spending_makes_change_from_a_larger_coin() {
        let definitions = coin_definitions();
        let mut inventory = purse(&[(GOLD_TYPE_ID, 1)]);
        // Pay 1s out of a gold piece: the purse must come back as change.
        assert!(spend_copper(
            &mut inventory,
            COPPER_PER_SILVER,
            &definitions
        ));
        assert_eq!(
            purse_total_copper(&inventory),
            COPPER_PER_GOLD - COPPER_PER_SILVER
        );
    }

    #[test]
    fn spending_exactly_empties_the_purse() {
        let definitions = coin_definitions();
        let mut inventory = purse(&[(SILVER_TYPE_ID, 1)]);
        assert!(spend_copper(
            &mut inventory,
            COPPER_PER_SILVER,
            &definitions
        ));
        assert_eq!(purse_total_copper(&inventory), 0);
    }

    #[test]
    fn spending_more_than_you_have_leaves_the_purse_untouched() {
        let definitions = coin_definitions();
        let mut inventory = purse(&[(SILVER_TYPE_ID, 1)]);
        assert!(!spend_copper(&mut inventory, COPPER_PER_GOLD, &definitions));
        assert_eq!(
            purse_total_copper(&inventory),
            COPPER_PER_SILVER,
            "a failed payment must not consume coins"
        );
    }

    #[test]
    fn spending_zero_is_a_no_op() {
        let definitions = coin_definitions();
        let mut inventory = purse(&[(COPPER_TYPE_ID, 5)]);
        assert!(spend_copper(&mut inventory, 0, &definitions));
        assert_eq!(purse_total_copper(&inventory), 5);
    }

    #[test]
    fn change_never_loses_value_across_many_amounts() {
        let definitions = coin_definitions();
        for fee in [1, 11, 12, 13, 239, 240, 241, 479] {
            let mut inventory = purse(&[(GOLD_TYPE_ID, 2)]);
            let before = purse_total_copper(&inventory);
            assert!(spend_copper(&mut inventory, fee, &definitions), "fee={fee}");
            assert_eq!(
                purse_total_copper(&inventory),
                before - fee,
                "value leaked paying {fee}"
            );
        }
    }
}
