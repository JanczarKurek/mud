//! Turning a codex tier into the prose the Log window shows.
//!
//! Both ladders build their body by concatenating one block per tier reached,
//! separated by [`crate::log::BODY_DIVIDER`] so `populate_body_display`
//! renders horizontal rules between them for free. A tier-up therefore
//! *appends* — everything the player already knew stays put, in the order they
//! learned it.
//!
//! Only **durable** knowledge lives here. A dossier says who someone is, who
//! they answer to, and what's known about their past; it never says how they
//! feel about you right now or what crimes they've pinned on you — that is
//! live state and belongs in the Details popup (`npc::social_read`), which
//! re-rolls it every time.

use crate::log::BODY_DIVIDER;
use crate::world::object_definitions::{
    OverworldObjectDefinition, OverworldObjectDefinitions, QuantityDistribution,
};

/// Highest People tier. 1 = seen through, 2 = allegiances, 3 = background.
pub const PEOPLE_MAX_TIER: u8 = 3;
/// Highest Bestiary tier — 4 additionally requires the mastery kill count.
pub const BESTIARY_MAX_TIER: u8 = 4;

/// Joins tier blocks into a log body. Empty blocks are dropped so a definition
/// missing (say) `lore` doesn't leave a dangling divider.
fn join_blocks(blocks: Vec<String>) -> String {
    blocks
        .into_iter()
        .filter(|block| !block.trim().is_empty())
        .collect::<Vec<_>>()
        .join(&format!("\n{BODY_DIVIDER}\n"))
}

/// Body for a People dossier at `tier` (1..=[`PEOPLE_MAX_TIER`]).
pub fn compose_people_body(
    definitions: &OverworldObjectDefinitions,
    def: &OverworldObjectDefinition,
    tier: u8,
) -> String {
    let mut blocks = Vec::new();

    // Tier 1 — who they are.
    let mut identity = String::new();
    if let Some(occupation) = &def.occupation {
        identity.push_str(occupation);
        identity.push('\n');
    }
    identity.push_str(def.description_for_count(1));
    blocks.push(identity);

    // Tier 2 — who they answer to, and who answers to them.
    if tier >= 2 {
        blocks.push(compose_allegiances(definitions, def));
    }

    // Tier 3 — background.
    if tier >= 3 {
        let mut background = String::new();
        if let Some(lore) = &def.lore {
            background.push_str(lore);
        } else {
            background.push_str("You've learned nothing more of their past.");
        }
        blocks.push(background);
    }

    join_blocks(blocks)
}

/// The "answers to / protects / can absolve" block, plus who else stands with
/// them. Shared with the Details popup's `relationships` rows so the two views
/// never disagree about an NPC's social web.
fn compose_allegiances(
    definitions: &OverworldObjectDefinitions,
    def: &OverworldObjectDefinition,
) -> String {
    let relations = derive_relationships(definitions, def);
    if relations.is_empty() {
        return "They answer to no faction you can name.".to_owned();
    }
    relations
        .into_iter()
        .map(|(note, subject)| format!("{note} {subject}."))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Maximum relationship rows surfaced anywhere — a well-connected NPC in a
/// large town would otherwise bloat both the popup and the replicated body.
pub const MAX_RELATIONSHIPS: usize = 4;

/// Derives `(note, subject)` pairs describing an NPC's social ties, from the
/// faction fields alone — nothing here is separately authored.
pub fn derive_relationships(
    definitions: &OverworldObjectDefinitions,
    def: &OverworldObjectDefinition,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();

    for faction in &def.factions {
        out.push((
            "Answers to".to_owned(),
            definitions.faction_display_name(faction),
        ));
    }
    for faction in &def.protects_factions {
        out.push((
            "Protects".to_owned(),
            definitions.faction_display_name(faction),
        ));
    }
    if let Some(judge) = &def.judge {
        for faction in &judge.clears_factions {
            out.push((
                "Can absolve crimes against".to_owned(),
                definitions.faction_display_name(faction),
            ));
        }
    }

    // Who else wears the same colours. Sorted by id so the line is stable
    // across runs (the definition map is a `HashMap`).
    let mut peers: Vec<&str> = Vec::new();
    for id in definitions.ids() {
        let Some(other) = definitions.get(id) else {
            continue;
        };
        if std::ptr::eq(other, def) || other.name == def.name {
            continue;
        }
        if other
            .factions
            .iter()
            .any(|faction| def.factions.contains(faction))
        {
            peers.push(id);
        }
    }
    peers.sort_unstable();
    for id in peers {
        if let Some(other) = definitions.get(id) {
            out.push(("Stands with".to_owned(), other.name.clone()));
        }
    }

    out.truncate(MAX_RELATIONSHIPS);
    out
}

/// Body for a Bestiary entry at `tier` (1..=[`BESTIARY_MAX_TIER`]).
pub fn compose_bestiary_body(def: &OverworldObjectDefinition, tier: u8) -> String {
    let mut blocks = Vec::new();

    // Tier 1 — sighted.
    let mut sighted = String::new();
    if !def.tags.is_empty() {
        sighted.push_str(&capitalized_list(&def.tags));
        sighted.push('\n');
    }
    sighted.push_str(def.description_for_count(1));
    blocks.push(sighted);

    // Tier 2 — studied: how hard it is to put down.
    if tier >= 2 {
        let mut studied = vec![format!("Level {}", def.level.unwrap_or(1))];
        if let Some(band) = hp_band(def.hp.as_deref()) {
            studied.push(format!("Vitality: {band}"));
        }
        if def.armor > 0 {
            studied.push(format!("Armour {}", def.armor));
        }
        if def.block > 0 {
            studied.push(format!("Block {}", def.block));
        }
        blocks.push(studied.join(" · "));
    }

    // Tier 3 — analyzed: how it fights, and what sets it off.
    if tier >= 3 {
        let mut analyzed = Vec::new();
        let damage_type = def
            .attack_profile
            .as_ref()
            .and_then(|profile| profile.damage_type)
            .map(|kind| kind.display_name());
        match (&def.damage, damage_type) {
            (Some(dice), Some(kind)) => analyzed.push(format!("Strikes for {dice} {kind} damage.")),
            (Some(dice), None) => analyzed.push(format!("Strikes for {dice} damage.")),
            (None, Some(kind)) => analyzed.push(format!("Deals {kind} damage.")),
            (None, None) => {}
        }
        if def.dodge_bonus > 0 {
            analyzed.push(format!("Nimble (dodge +{}).", def.dodge_bonus));
        }
        if !def.hostile_towards.is_empty() {
            analyzed.push(format!("Hunts {}.", plain_list(&def.hostile_towards)));
        }
        if !def.flees_from.is_empty() {
            analyzed.push(format!("Flees from {}.", plain_list(&def.flees_from)));
        }
        if let Some(behavior) = &def.npc_behavior {
            analyzed.push(format!(
                "Notices intruders from about {} tiles.",
                behavior.detect_distance_tiles
            ));
        }
        if analyzed.is_empty() {
            analyzed.push("It fights with nothing you could name.".to_owned());
        }
        blocks.push(analyzed.join("\n"));
    }

    // Tier 4 — mastered: spoils and lore.
    if tier >= 4 {
        let mut mastered = Vec::new();
        let drops: Vec<String> = def
            .loot_table
            .as_ref()
            .map(|table| {
                table
                    .drops
                    .iter()
                    .map(|drop| {
                        format!(
                            "{} ({}{})",
                            drop.type_id.replace('_', " "),
                            odds_word(drop.probability),
                            quantity_note(&drop.quantity),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if drops.is_empty() {
            mastered.push("It carries nothing worth taking.".to_owned());
        } else {
            mastered.push(format!("Yields: {}.", drops.join(", ")));
        }
        if let Some(lore) = &def.lore {
            mastered.push(lore.clone());
        }
        blocks.push(mastered.join("\n"));
    }

    join_blocks(blocks)
}

/// Buckets a `hp:` dice expression into a worded band. Deliberately coarse:
/// the bestiary should tell you roughly how tough something is, never hand you
/// the exact formula the simulation rolls.
pub fn hp_band(expression: Option<&str>) -> Option<&'static str> {
    let flat: i32 = expression?
        .split(['+', '*'])
        .filter_map(|piece| piece.trim().parse::<i32>().ok())
        .sum();
    Some(match flat {
        ..=15 => "frail",
        16..=35 => "sturdy",
        36..=70 => "tough",
        71..=120 => "formidable",
        _ => "monstrous",
    })
}

/// Worded drop odds — the bestiary never prints raw probabilities.
fn odds_word(probability: f32) -> &'static str {
    if probability >= 0.99 {
        "always"
    } else if probability >= 0.5 {
        "often"
    } else if probability >= 0.15 {
        "sometimes"
    } else {
        "rarely"
    }
}

/// "" for a single item, " ×N" hint otherwise. Kept vague for ranges for the
/// same reason `hp_band` is coarse.
fn quantity_note(quantity: &QuantityDistribution) -> String {
    match quantity {
        QuantityDistribution::Fixed(1) => String::new(),
        QuantityDistribution::Fixed(n) => format!(", ×{n}"),
        _ => ", varying amounts".to_owned(),
    }
}

/// `["beast", "predator"]` → `"Beast · Predator"`.
fn capitalized_list(items: &[String]) -> String {
    items
        .iter()
        .map(|item| {
            let cleaned = item.replace('_', " ");
            let mut chars = cleaned.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// `["livestock", "townsfolk"]` → `"livestock and townsfolk"`.
fn plain_list(items: &[String]) -> String {
    let cleaned: Vec<String> = items.iter().map(|item| item.replace('_', " ")).collect();
    match cleaned.len() {
        0 => String::new(),
        1 => cleaned[0].clone(),
        _ => {
            let last = cleaned.last().cloned().unwrap_or_default();
            format!("{} and {last}", cleaned[..cleaned.len() - 1].join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wolf() -> OverworldObjectDefinitions {
        OverworldObjectDefinitions::load_from_disk()
    }

    #[test]
    fn hp_band_buckets_by_flat_total() {
        assert_eq!(hp_band(Some("1d4+6")), Some("frail"));
        // The wolf's own expression: 1d8+22+constitution*2.
        assert_eq!(hp_band(Some("1d8+22+constitution*2")), Some("sturdy"));
        assert_eq!(hp_band(Some("2d10+80")), Some("formidable"));
        assert_eq!(hp_band(None), None);
    }

    #[test]
    fn odds_words_cover_the_probability_range() {
        assert_eq!(odds_word(1.0), "always");
        assert_eq!(odds_word(0.6), "often");
        assert_eq!(odds_word(0.2), "sometimes");
        assert_eq!(odds_word(0.05), "rarely");
    }

    #[test]
    fn bestiary_body_grows_by_tier() {
        let definitions = wolf();
        let def = definitions.get("wolf").expect("wolf definition");

        let tier1 = compose_bestiary_body(def, 1);
        let tier2 = compose_bestiary_body(def, 2);
        let tier3 = compose_bestiary_body(def, 3);
        let tier4 = compose_bestiary_body(def, 4);

        // Each tier strictly extends the last.
        assert!(tier2.starts_with(&tier1), "tier 2 should extend tier 1");
        assert!(tier3.starts_with(&tier2), "tier 3 should extend tier 2");
        assert!(tier4.starts_with(&tier3), "tier 4 should extend tier 3");

        // Tier 1 identifies, and says nothing about combat.
        assert!(tier1.contains("Beast"));
        assert!(!tier1.contains("Level"));
        // Tier 2 sizes it up, without leaking the raw HP formula.
        assert!(tier2.contains("Level 3"));
        assert!(!tier2.contains("1d8+22"));
        // Tier 3 explains how it fights.
        assert!(tier3.contains("1d6+2"), "expected the damage dice: {tier3}");
        assert!(tier3.contains("livestock"));
        // Tier 4 lists the spoils.
        assert!(tier4.contains("Yields:"), "expected loot: {tier4}");

        // Dividers accumulate one per tier boundary.
        assert_eq!(tier1.matches(BODY_DIVIDER).count(), 0);
        assert_eq!(tier4.matches(BODY_DIVIDER).count(), 3);
    }

    #[test]
    fn people_body_reveals_allegiances_at_tier_2() {
        let definitions = wolf();
        let def = definitions
            .get("town_guard")
            .expect("town_guard definition");

        let tier1 = compose_people_body(&definitions, def, 1);
        let tier2 = compose_people_body(&definitions, def, 2);

        assert!(!tier1.contains("Answers to"));
        assert!(
            tier2.contains("Answers to") || tier2.contains("Protects"),
            "expected an allegiance line: {tier2}"
        );
        assert!(tier2.starts_with(&tier1));
    }

    #[test]
    fn relationships_are_capped() {
        let definitions = wolf();
        let def = definitions
            .get("town_guard")
            .expect("town_guard definition");
        assert!(derive_relationships(&definitions, def).len() <= MAX_RELATIONSHIPS);
    }

    #[test]
    fn plain_list_reads_naturally() {
        assert_eq!(plain_list(&["livestock".to_owned()]), "livestock");
        assert_eq!(
            plain_list(&["livestock".to_owned(), "town_folk".to_owned()]),
            "livestock and town folk"
        );
    }
}
