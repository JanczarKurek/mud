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

/// One "an NPC committed an attack against a player" record from a battle tick.
/// Recorded regardless of the to-hit outcome (a dodged or blocked swing still
/// counts as being attacked), but only for committed attacks — an NPC merely
/// aggroed/chasing out of range produces nothing.
pub struct RetaliationHit {
    pub player: Entity,
    pub attacker: Entity,
    /// Display name of the attacker, for the narrator line.
    pub attacker_name: String,
}

/// Deferred auto-retaliate queue. `resolve_battle_turn` records each committed
/// NPC attack on a player here; `apply_auto_retaliation` drains it and, for
/// players in Auto-Retaliate mode with no current `CombatTarget`, locks one
/// attacker as their target. Mirrors the `PendingDamageEvents` /
/// `PendingNpcSummons` deferred-write pattern (the resolver is at Bevy's
/// 16-system-param cap and cannot take the extra queries itself).
#[derive(Resource, Default)]
pub struct PendingRetaliations {
    pub items: Vec<RetaliationHit>,
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
