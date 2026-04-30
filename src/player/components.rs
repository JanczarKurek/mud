use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::damage_expr::DamageExpr;
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::EquipmentSlot;

#[derive(Component, Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WeaponDamage(pub DamageExpr);

impl Default for WeaponDamage {
    fn default() -> Self {
        Self(DamageExpr::melee_default())
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct PlayerId(pub u64);

#[derive(Component, Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayerIdentity {
    pub id: PlayerId,
}

/// A self-describing stack of identical items. Carries the canonical type
/// (matching a directory under `assets/overworld_objects/`) plus any
/// per-instance `properties` (e.g. a scroll's `spell_id`). Notably *no* runtime
/// `object_id` — runtime ids are allocated only when the stack leaves the
/// inventory and becomes a world entity, so saves stay portable across map
/// edits.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct InventoryStack {
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    pub quantity: u32,
}

/// A single item occupying an equipment slot. Same shape as the descriptive
/// half of `InventoryStack` (without `quantity`, since most equipment slots
/// are 1-of-a-kind — ammo's quantity rides on `Inventory::ammo_quantity`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct EquippedItem {
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
}

impl EquippedItem {
    pub fn new(type_id: impl Into<String>) -> Self {
        Self {
            type_id: type_id.into(),
            properties: ObjectProperties::new(),
        }
    }
}

#[derive(Component, Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Inventory {
    pub backpack_slots: Vec<Option<InventoryStack>>,
    pub equipment_slots: Vec<(EquipmentSlot, Option<EquippedItem>)>,
    /// Quantity of the stack currently occupying `EquipmentSlot::Ammo`.
    #[serde(default)]
    pub ammo_quantity: u32,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            backpack_slots: vec![None; 16],
            equipment_slots: EquipmentSlot::ALL
                .into_iter()
                .map(|slot| (slot, None))
                .collect(),
            ammo_quantity: 0,
        }
    }
}

impl Inventory {
    pub fn equipment_item(&self, slot: EquipmentSlot) -> Option<&EquippedItem> {
        self.equipment_slots
            .iter()
            .find_map(|(equipment_slot, item)| {
                if *equipment_slot == slot {
                    item.as_ref()
                } else {
                    None
                }
            })
    }

    pub fn take_equipment_item(&mut self, slot: EquipmentSlot) -> Option<EquippedItem> {
        self.equipment_slots
            .iter_mut()
            .find_map(|(equipment_slot, item)| {
                if *equipment_slot == slot {
                    item.take()
                } else {
                    None
                }
            })
    }

    pub fn place_equipment_item(&mut self, slot: EquipmentSlot, item: EquippedItem) -> bool {
        for (equipment_slot, slot_item) in &mut self.equipment_slots {
            if *equipment_slot != slot {
                continue;
            }

            if slot_item.is_some() {
                return false;
            }

            *slot_item = Some(item);
            return true;
        }

        false
    }

    pub fn restore_equipment_item(&mut self, slot: EquipmentSlot, item: EquippedItem) {
        for (equipment_slot, slot_item) in &mut self.equipment_slots {
            if *equipment_slot == slot {
                *slot_item = Some(item);
                return;
            }
        }
    }

    /// Ensure every `EquipmentSlot` variant is represented. Called after loading
    /// saves from before a new slot was added so older saves gain the new slot
    /// as empty instead of iteration silently skipping it.
    pub fn ensure_slots(&mut self) {
        for slot in EquipmentSlot::ALL {
            let present = self
                .equipment_slots
                .iter()
                .any(|(existing, _)| *existing == slot);
            if !present {
                self.equipment_slots.push((slot, None));
            }
        }
        if self.equipment_item(EquipmentSlot::Ammo).is_none() {
            self.ammo_quantity = 0;
        }
    }

    pub fn ammo_stack(&self) -> Option<InventoryStack> {
        let item = self.equipment_item(EquipmentSlot::Ammo)?;
        Some(InventoryStack {
            type_id: item.type_id.clone(),
            properties: item.properties.clone(),
            quantity: self.ammo_quantity,
        })
    }

    pub fn set_ammo(&mut self, item: EquippedItem, quantity: u32) {
        self.restore_equipment_item(EquipmentSlot::Ammo, item);
        self.ammo_quantity = quantity;
    }

    /// Decrement the ammo stack by one. Reports whether the slot is now empty
    /// (so the caller can update UI / chat). No `object_id` to free — runtime
    /// ids only exist for items actually in the world.
    pub fn consume_one_ammo(&mut self) -> AmmoConsumption {
        if self.equipment_item(EquipmentSlot::Ammo).is_none() || self.ammo_quantity == 0 {
            return AmmoConsumption::None;
        }
        self.ammo_quantity -= 1;
        if self.ammo_quantity == 0 {
            self.take_equipment_item(EquipmentSlot::Ammo);
            return AmmoConsumption::Emptied;
        }
        AmmoConsumption::Decremented
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmmoConsumption {
    None,
    Decremented,
    Emptied,
}

#[derive(Component, Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ChatLog {
    pub lines: Vec<String>,
    pub max_lines: usize,
}

impl Default for ChatLog {
    fn default() -> Self {
        Self {
            lines: vec![
                "[Narrator]: Right-click an item to inspect it.".to_owned(),
                "[Narrator]: Right-click a nearby barrel to open it.".to_owned(),
            ],
            max_lines: 8,
        }
    }
}

impl ChatLog {
    pub fn push_line(&mut self, message: impl Into<String>) {
        self.lines.push(message.into());
        if self.lines.len() > self.max_lines {
            let overflow = self.lines.len() - self.max_lines;
            self.lines.drain(0..overflow);
        }
    }

    pub fn push_narrator(&mut self, message: impl Into<String>) {
        self.push_line(format!("[Narrator]: {}", message.into()));
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttributeSet {
    pub strength: i32,
    pub agility: i32,
    pub constitution: i32,
    pub willpower: i32,
    pub charisma: i32,
    pub focus: i32,
}

impl AttributeSet {
    pub const fn new(
        strength: i32,
        agility: i32,
        constitution: i32,
        willpower: i32,
        charisma: i32,
        focus: i32,
    ) -> Self {
        Self {
            strength,
            agility,
            constitution,
            willpower,
            charisma,
            focus,
        }
    }

    pub fn add_assign(&mut self, other: Self) {
        self.strength += other.strength;
        self.agility += other.agility;
        self.constitution += other.constitution;
        self.willpower += other.willpower;
        self.charisma += other.charisma;
        self.focus += other.focus;
    }

    pub fn clamped_min(self, minimum: i32) -> Self {
        Self {
            strength: self.strength.max(minimum),
            agility: self.agility.max(minimum),
            constitution: self.constitution.max(minimum),
            willpower: self.willpower.max(minimum),
            charisma: self.charisma.max(minimum),
            focus: self.focus.max(minimum),
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct VitalStats {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
}

impl VitalStats {
    pub const fn full(max_health: f32, max_mana: f32) -> Self {
        Self {
            health: max_health,
            max_health,
            mana: max_mana,
            max_mana,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct BaseStats {
    pub attributes: AttributeSet,
    pub max_health: i32,
    pub max_mana: i32,
    pub storage_slots: i32,
}

impl Default for BaseStats {
    fn default() -> Self {
        Self {
            attributes: AttributeSet::new(10, 10, 10, 10, 10, 10),
            max_health: 0,
            max_mana: 0,
            storage_slots: 8,
        }
    }
}

impl BaseStats {
    pub fn npc_default() -> Self {
        Self {
            attributes: AttributeSet::new(9, 9, 9, 8, 7, 8),
            max_health: 0,
            max_mana: 0,
            storage_slots: 0,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct DerivedStats {
    #[allow(dead_code)]
    pub attributes: AttributeSet,
    pub max_health: i32,
    pub max_mana: i32,
    pub storage_slots: usize,
}

impl Default for DerivedStats {
    fn default() -> Self {
        let base = BaseStats::default();
        Self::from_base(&base)
    }
}

impl DerivedStats {
    pub fn from_base(base: &BaseStats) -> Self {
        let attributes = base.attributes.clamped_min(1);
        let max_health =
            (35 + attributes.constitution * 6 + attributes.strength * 2 + base.max_health).max(1);
        let max_mana =
            (10 + attributes.willpower * 6 + attributes.focus * 3 + base.max_mana).max(0);
        let storage_slots = (base.storage_slots - 2 + attributes.strength / 4).max(0) as usize;

        Self {
            attributes,
            max_health,
            max_mana,
            storage_slots,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Deserialize, Serialize)]
pub struct MovementCooldown {
    pub remaining_seconds: f32,
    pub step_interval_seconds: f32,
}

impl Default for MovementCooldown {
    fn default() -> Self {
        Self {
            remaining_seconds: 0.0,
            step_interval_seconds: 0.18,
        }
    }
}
