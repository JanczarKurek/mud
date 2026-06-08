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
