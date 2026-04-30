use bevy::prelude::*;

use crate::app::state::simulation_active;
use crate::player::components::InventoryStack;
use crate::world::components::{SpaceId, TilePosition};
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::{LootTableDef, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::spawn_overworld_object;

#[derive(Component, Clone, Copy, Debug)]
pub struct CorpseTtl {
    pub remaining_seconds: f32,
}

pub fn tick_corpse_ttl(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut CorpseTtl)>,
) {
    for (entity, mut ttl) in query.iter_mut() {
        ttl.remaining_seconds -= time.delta_secs();
        if ttl.remaining_seconds <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Roll items from a loot table. Returns `(type_id, quantity)` pairs.
fn roll_loot(table: &LootTableDef) -> Vec<(String, u32)> {
    let mut results = Vec::new();
    for drop in &table.drops {
        let roll = {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64)
                .unwrap_or(0);
            // Mix with type_id hash so simultaneous rolls differ
            let mixed = nanos
                ^ drop
                    .type_id
                    .bytes()
                    .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
            (mixed % 10_000) as f32 / 10_000.0
        };
        if roll < drop.probability {
            let qty = drop.quantity.roll();
            if qty > 0 {
                results.push((drop.type_id.clone(), qty));
            }
        }
    }
    results
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
        slots[i] = Some(InventoryStack {
            type_id,
            properties: ObjectProperties::new(),
            quantity: qty,
        });
    }

    let corpse_id = registry.allocate_runtime_id(&loot_table.corpse_type_id);
    let entity = spawn_overworld_object(
        commands,
        definitions,
        corpse_id,
        &loot_table.corpse_type_id,
        Some(slots),
        space_id,
        tile_position,
        None,
    );
    commands.entity(entity).insert(CorpseTtl {
        remaining_seconds: loot_table.corpse_despawn_seconds,
    });
}

pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, tick_corpse_ttl.run_if(simulation_active));
    }
}
