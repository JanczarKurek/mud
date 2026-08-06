//! Unified navigation for [`ItemSlotRef`] — the one place that knows how to
//! reach the `Option<InventoryStack>` behind a backpack slot, a world
//! container slot, or a pouch sub-slot.
//!
//! `Equipment` is deliberately *not* resolved here: equipment slots store an
//! `EquippedItem` plus `ammo_quantity` bookkeeping rather than an
//! `InventoryStack`, so callers keep their own equipment branch and these
//! helpers return `None` for it.

use bevy::prelude::*;

use crate::game::commands::ItemSlotRef;
use crate::game::helpers::WorldObjectQuery;
use crate::game::resources::InventoryState;
use crate::player::components::InventoryStack;
use crate::world::components::Container;

/// Resolve a world container's `object_id` to its entity.
pub(crate) fn find_container_entity(
    object_id: u64,
    object_query: &WorldObjectQuery,
) -> Option<Entity> {
    object_query
        .iter()
        .find_map(|(entity, _, _, object)| (object.object_id == object_id).then_some(entity))
}

/// Read-only access to the option-slot behind a player-side stack slot
/// (`Backpack` / `PouchInBackpack`). `Equipment` / `Container` → `None`.
pub(crate) fn player_stack_slot(
    inventory_state: &InventoryState,
    slot_ref: ItemSlotRef,
) -> Option<&Option<InventoryStack>> {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => inventory_state.backpack_slots.get(slot_index),
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => inventory_state
            .backpack_slots
            .get(backpack_slot)?
            .as_ref()?
            .contained_slots
            .as_ref()?
            .get(sub_slot),
        ItemSlotRef::Equipment(_) | ItemSlotRef::Container { .. } => None,
    }
}

/// Mutable access to the option-slot behind a player-side stack slot
/// (`Backpack` / `PouchInBackpack`). `Equipment` / `Container` → `None`.
pub(crate) fn player_stack_slot_mut(
    inventory_state: &mut InventoryState,
    slot_ref: ItemSlotRef,
) -> Option<&mut Option<InventoryStack>> {
    match slot_ref {
        ItemSlotRef::Backpack(slot_index) => inventory_state.backpack_slots.get_mut(slot_index),
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => inventory_state
            .backpack_slots
            .get_mut(backpack_slot)?
            .as_mut()?
            .contained_slots
            .as_mut()?
            .get_mut(sub_slot),
        ItemSlotRef::Equipment(_) | ItemSlotRef::Container { .. } => None,
    }
}

/// Mutable access to the option-slot behind any stack-backed slot ref —
/// `Backpack`, `PouchInBackpack`, or a world `Container`. `None` for
/// `Equipment` (see module docs) and for stale refs (despawned container,
/// out-of-range index).
pub(crate) fn stack_slot_mut<'a>(
    inventory_state: &'a mut InventoryState,
    container_query: &'a mut Query<&mut Container>,
    object_query: &WorldObjectQuery,
    slot_ref: ItemSlotRef,
) -> Option<&'a mut Option<InventoryStack>> {
    match slot_ref {
        ItemSlotRef::Container {
            object_id,
            slot_index,
        } => {
            let entity = find_container_entity(object_id, object_query)?;
            container_query
                .get_mut(entity)
                .ok()?
                .into_inner()
                .slots
                .get_mut(slot_index)
        }
        _ => player_stack_slot_mut(inventory_state, slot_ref),
    }
}

/// Take up to `amount` off the stack in a slot, clearing the slot when it
/// reaches zero. No-op when `slot` is empty.
pub(crate) fn reduce_option_slot(slot: &mut Option<InventoryStack>, amount: u32) {
    if let Some(stack) = slot {
        if stack.quantity <= amount {
            *slot = None;
        } else {
            stack.quantity -= amount;
        }
    }
}
