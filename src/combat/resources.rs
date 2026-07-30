use bevy::prelude::*;

#[derive(Resource)]
pub struct BattleTurnTimer {
    pub remaining_seconds: f32,
    pub interval_seconds: f32,
}

impl Default for BattleTurnTimer {
    fn default() -> Self {
        Self {
            remaining_seconds: 1.0,
            interval_seconds: 1.0,
        }
    }
}

/// Deferred charge-consumption queue for item modifiers. `resolve_battle_turn`
/// builds combatant snapshots read-only, so it cannot mutate the attacker's
/// `Inventory` in place; instead it records `(attacker_entity, type_ex)` for
/// each `Charges` modifier that successfully applied this turn, and
/// `apply_pending_modifier_consumption` drains the queue afterward. Mirrors the
/// `PendingDamageEvents` deferred-write pattern.
#[derive(Resource, Default)]
pub struct PendingModifierConsumption {
    pub spent: Vec<(Entity, String)>,
}

/// One creature-summon request produced by an NPC spell cast.
pub struct NpcSummonRequest {
    /// The casting NPC — becomes `Companion.owner`, so its summons despawn
    /// together when it recasts.
    pub caster: Entity,
    pub space_id: crate::world::components::SpaceId,
    /// Tile the summons appear on (the cast target tile, or the caster's own
    /// tile for a self-cast).
    pub tile: crate::world::components::TilePosition,
    pub spec: crate::magic::resources::SummonSpec,
}

/// Deferred summon queue for NPC casts.
///
/// `resolve_battle_turn` already sits at Bevy's 16-system-param cap and holds
/// `ObjectRegistry` immutably, but spawning needs `ResMut<ObjectRegistry>` plus
/// the definitions and companion queries. So the cast path records the request
/// here and `apply_pending_npc_summons` drains it, mirroring the
/// `PendingDamageEvents` / `PendingModifierConsumption` deferred-write pattern.
#[derive(Resource, Default)]
pub struct PendingNpcSummons {
    pub requests: Vec<NpcSummonRequest>,
}
