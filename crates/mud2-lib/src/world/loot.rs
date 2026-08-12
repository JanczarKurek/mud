use bevy::prelude::*;

use crate::player::components::InventoryStack;
use crate::world::components::{SpaceId, TilePosition};
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::{LootTableDef, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::spawn_overworld_object;
use crate::world::ttl::Ttl;

/// Per-drop salt: the drop's position in the table mixed with a hash of its
/// `type_id`. Both parts matter — the index separates two drops of the *same*
/// item, the hash separates different items at the same index across tables.
pub(crate) fn drop_salt(index: usize, type_id: &str) -> u64 {
    let hash = type_id
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    hash.wrapping_add((index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Roll items from a loot table. Returns `(type_id, quantity)` pairs.
fn roll_loot(table: &LootTableDef) -> Vec<(String, u32)> {
    let mut results = Vec::new();
    for (index, drop) in table.drops.iter().enumerate() {
        let salt = drop_salt(index, &drop.type_id);
        let roll = {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64)
                .unwrap_or(0);
            // Mix with the per-drop salt so simultaneous rolls differ.
            let mixed = nanos ^ salt;
            (mixed % 10_000) as f32 / 10_000.0
        };
        if roll < drop.probability {
            // Offset the quantity salt off the probability salt: reusing the
            // same value would tie "did it drop" to "how many", so a table
            // would only ever pay out its high quantities.
            let qty = drop.quantity.roll(salt ^ 0x5DEE_CE66_D1B2_745F);
            if qty > 0 {
                results.push((drop.type_id.clone(), qty));
            }
        }
    }
    results
}

/// Spawn a corpse container entity holding the player's full inventory at the
/// death tile. Always uses the `generic_corpse` definition; a longer
/// despawn TTL gives the player time to retrieve their gear after respawn.
/// Items past the corpse's container capacity are silently dropped — that
/// will happen rarely (corpse capacity defaults to 20, inventory backpack is
/// 16) but documenting it here so callers know.
pub fn spawn_corpse_for_player(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    registry: &mut ObjectRegistry,
    space_id: SpaceId,
    tile_position: TilePosition,
    dropped_items: Vec<InventoryStack>,
) {
    const CORPSE_TYPE_ID: &str = "generic_corpse";
    const CORPSE_DESPAWN_SECONDS: f32 = 300.0; // 5 minutes for retrieval

    let capacity = definitions
        .get(CORPSE_TYPE_ID)
        .and_then(|def| def.container_capacity)
        .unwrap_or(20);

    let mut slots: Vec<Option<InventoryStack>> = vec![None; capacity];
    for (i, stack) in dropped_items.into_iter().enumerate().take(capacity) {
        slots[i] = Some(stack);
    }

    let corpse_id = registry.allocate_runtime_id(CORPSE_TYPE_ID);
    let entity = spawn_overworld_object(
        commands,
        definitions,
        registry,
        corpse_id,
        CORPSE_TYPE_ID,
        Some(slots),
        space_id,
        tile_position,
        None,
    );
    commands.entity(entity).insert(Ttl {
        remaining_seconds: CORPSE_DESPAWN_SECONDS,
    });
}

/// Spawn a corpse container entity at the given position.
/// Rolls loot from the NPC's loot table and places the items inside.
pub fn spawn_corpse_for_npc(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    registry: &mut ObjectRegistry,
    loot_table: &LootTableDef,
    space_id: SpaceId,
    tile_position: TilePosition,
) {
    let rolled_items = roll_loot(loot_table);

    let capacity = definitions
        .get(&loot_table.corpse_type_id)
        .and_then(|def| def.container_capacity)
        .unwrap_or(20);

    let mut slots: Vec<Option<InventoryStack>> = vec![None; capacity];
    for (i, (type_id, qty)) in rolled_items.into_iter().enumerate().take(capacity) {
        slots[i] = Some(InventoryStack::item(type_id, ObjectProperties::new(), qty));
    }

    let corpse_id = registry.allocate_runtime_id(&loot_table.corpse_type_id);
    let entity = spawn_overworld_object(
        commands,
        definitions,
        registry,
        corpse_id,
        &loot_table.corpse_type_id,
        Some(slots),
        space_id,
        tile_position,
        None,
    );
    commands.entity(entity).insert(Ttl {
        remaining_seconds: loot_table.corpse_despawn_seconds,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::object_definitions::QuantityDistribution;

    /// Content guard: every `loot.drops[].type_id` and every `corpse_type_id`
    /// must name a definition that actually exists. A typo here is invisible
    /// at load time — the drop just silently never appears in a corpse.
    #[test]
    fn every_loot_drop_names_a_real_definition() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        let mut broken = Vec::new();
        let ids: Vec<String> = definitions.ids().map(str::to_owned).collect();
        for type_id in &ids {
            let Some(table) = definitions.get(type_id).and_then(|d| d.loot_table.as_ref()) else {
                continue;
            };
            if definitions.get(&table.corpse_type_id).is_none() {
                broken.push(format!(
                    "{type_id}: corpse_type_id `{}` does not exist",
                    table.corpse_type_id
                ));
            }
            for drop in &table.drops {
                if definitions.get(&drop.type_id).is_none() {
                    broken.push(format!("{type_id}: drop `{}` does not exist", drop.type_id));
                }
            }
        }
        assert!(
            broken.is_empty(),
            "dangling loot references:\n  {}",
            broken.join("\n  ")
        );
    }

    /// The bug this guards: `QuantityDistribution::roll` reads the wall clock,
    /// so without a per-drop salt every `uniform` drop resolved in the same
    /// call returns an identical number.
    #[test]
    fn same_range_drops_are_not_locked_together() {
        let dist = QuantityDistribution::Uniform(1, 20);
        let rolls: Vec<u32> = (0..8)
            .map(|i| dist.roll(drop_salt(i, "copper_coin")))
            .collect();
        assert!(
            rolls.iter().any(|q| *q != rolls[0]),
            "all eight rolls came back identical: {rolls:?}"
        );
        assert!(
            rolls.iter().all(|q| (1..=20).contains(q)),
            "out of range: {rolls:?}"
        );
    }

    #[test]
    fn drop_salt_varies_by_index_and_type_id() {
        assert_ne!(drop_salt(0, "apple"), drop_salt(1, "apple"));
        assert_ne!(drop_salt(0, "apple"), drop_salt(0, "bread_loaf"));
    }
}

#[cfg(test)]
mod corpse_spawn_tests {
    use super::*;
    use crate::world::components::Container;
    use crate::world::object_registry::ObjectRegistry;

    /// End-to-end on the real content: roll a shipped creature's table and
    /// place the results in a corpse container, the same way the death hook
    /// in `combat::damage` does. Guards the whole path — table parse, roll,
    /// corpse definition lookup, container sizing, item construction.
    #[test]
    fn shipped_tables_fill_a_real_corpse_container() {
        let mut app = crate::test_support::TestServerApp::new().build();
        // Loaded separately rather than borrowed off the app: the spawn call
        // below needs the world mutably at the same time, and the resource
        // isn't `Clone`.
        let definitions = OverworldObjectDefinitions::load_from_disk();

        // A guaranteed-coin table, so a single roll is deterministic enough to
        // assert on: the ogre's silver line is probability 1.0.
        let table = definitions
            .get("ogre_brute")
            .and_then(|d| d.loot_table.clone())
            .expect("ogre_brute ships a loot table");
        assert!(
            table
                .drops
                .iter()
                .any(|d| d.type_id == "silver_coin" && d.probability >= 1.0),
            "test assumes the ogre's silver line is guaranteed"
        );

        let mut registry = ObjectRegistry::default();
        let space_id = SpaceId(0);
        let tile = TilePosition { x: 5, y: 5, z: 0 };
        {
            let world = app.world_mut();
            let mut commands = world.commands();
            spawn_corpse_for_npc(
                &mut commands,
                &definitions,
                &mut registry,
                &table,
                space_id,
                tile,
            );
        }
        app.world_mut().flush();

        let mut query = app.world_mut().query::<(&Container, &Ttl)>();
        let (container, ttl) = query
            .iter(app.world())
            .next()
            .expect("a corpse container was spawned");
        assert_eq!(ttl.remaining_seconds, table.corpse_despawn_seconds);

        let contents: Vec<(&str, u32)> = container
            .slots
            .iter()
            .flatten()
            .map(|s| (s.type_id.as_str(), s.quantity))
            .collect();
        let silver = contents
            .iter()
            .find(|(id, _)| *id == "silver_coin")
            .expect("the guaranteed silver drop landed in the corpse");
        assert!(
            (10..=30).contains(&silver.1),
            "silver quantity {} outside the authored uniform(10, 30)",
            silver.1
        );
    }
}
