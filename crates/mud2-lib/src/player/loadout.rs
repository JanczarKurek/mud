//! Data-driven loadouts.
//!
//! The inventory a character is granted lives in `assets/loadouts/*.yaml`,
//! not in code. Every file in the subdir is loaded into the [`Loadouts`]
//! resource keyed by file stem; the mandatory `starter` loadout is what a
//! brand-new character gets, other loadouts can be referenced by id (e.g.
//! from debug character presets). [`Loadout::apply_to`] seeds an
//! [`Inventory`] from a loadout. Loading mirrors
//! `SpellDefinitions::load_from_disk` — scan the `loadouts` asset subdir
//! (bundled + XDG overlay) via [`discover_yaml_assets`].
//!
//! Each item entry may carry per-instance `modifiers`, so a starter weapon can
//! ship pre-enchanted (the "effects applied on equipment" showcase) entirely
//! from YAML.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Deserialize;

use crate::assets::discover_yaml_assets;
use crate::combat::modifiers::ItemModifier;
use crate::player::components::{EquippedItem, Inventory, InventoryStack};
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::EquipmentSlot;

/// File stem of the loadout granted to every fresh character.
pub const STARTER_LOADOUT_ID: &str = "starter";

fn default_quantity() -> u32 {
    1
}

/// One item in a loadout: a type id plus optional quantity, per-instance
/// properties (e.g. a scroll's `spell_id`), and per-instance enchantments.
#[derive(Debug, Clone, Deserialize)]
pub struct LoadoutItem {
    pub type_id: String,
    /// Stack size for backpack items / ammo count. Defaults to 1.
    #[serde(default = "default_quantity")]
    pub quantity: u32,
    #[serde(default)]
    pub properties: ObjectProperties,
    /// Per-instance enchantments (see [`ItemModifier`]). Lets a loadout ship
    /// pre-buffed gear without code.
    #[serde(default)]
    pub modifiers: Vec<ItemModifier>,
}

/// An item that starts equipped in a specific slot. Same fields as
/// [`LoadoutItem`] plus the target `slot`. `quantity` is only meaningful for
/// [`EquipmentSlot::Ammo`] (it becomes `Inventory::ammo_quantity`).
#[derive(Debug, Clone, Deserialize)]
pub struct LoadoutEquipment {
    pub slot: EquipmentSlot,
    pub type_id: String,
    #[serde(default = "default_quantity")]
    pub quantity: u32,
    #[serde(default)]
    pub properties: ObjectProperties,
    #[serde(default)]
    pub modifiers: Vec<ItemModifier>,
}

/// A complete loadout: items to wear plus items to drop in the backpack.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Loadout {
    #[serde(default)]
    pub equipment: Vec<LoadoutEquipment>,
    #[serde(default)]
    pub backpack: Vec<LoadoutItem>,
}

/// All loadouts under the `loadouts` asset subdir, keyed by file stem.
#[derive(Resource, Debug, Clone, Default)]
pub struct Loadouts {
    by_id: BTreeMap<String, Loadout>,
}

impl Loadouts {
    /// Load every `loadouts/*.yaml`. Panics on a parse error or if no
    /// `starter.yaml` is present — a missing starter loadout would silently
    /// spawn empty-handed characters, which is worse than failing loudly at
    /// startup.
    pub fn load_from_disk() -> Self {
        let mut by_id = BTreeMap::new();
        for asset in discover_yaml_assets("loadouts", "loadout") {
            let loadout =
                serde_yaml::from_str::<Loadout>(&asset.contents).unwrap_or_else(|error| {
                    panic!("Failed to parse loadout {}: {error}", asset.path.display())
                });
            by_id.insert(asset.id, loadout);
        }
        assert!(
            by_id.contains_key(STARTER_LOADOUT_ID),
            "No '{STARTER_LOADOUT_ID}.yaml' found under any 'loadouts' asset directory; \
             fresh characters would spawn with no items"
        );
        Self { by_id }
    }

    pub fn get(&self, id: &str) -> Option<&Loadout> {
        self.by_id.get(id)
    }

    /// Cross-check every referenced object `type_id` and equipment slot.
    /// Panics on a typo or a slot mismatch — same posture as
    /// `RecipeDefinitions::validate_against`: a bad authoring file should stop
    /// the world at startup rather than seed broken inventories later.
    pub fn validate_against(
        &self,
        objects: &crate::world::object_definitions::OverworldObjectDefinitions,
    ) {
        for (id, loadout) in &self.by_id {
            for entry in &loadout.equipment {
                let def = objects.get(&entry.type_id).unwrap_or_else(|| {
                    panic!(
                        "loadout `{id}` references unknown object type_id `{}`",
                        entry.type_id
                    )
                });
                assert_eq!(
                    def.equipment_slot,
                    Some(entry.slot),
                    "loadout `{id}`: `{}` cannot be equipped in slot {:?}",
                    entry.type_id,
                    entry.slot
                );
            }
            for entry in &loadout.backpack {
                assert!(
                    objects.get(&entry.type_id).is_some(),
                    "loadout `{id}` references unknown object type_id `{}`",
                    entry.type_id
                );
            }
        }
    }

    /// The loadout granted to every fresh character. `load_from_disk`
    /// guarantees its presence.
    pub fn starter(&self) -> &Loadout {
        &self.by_id[STARTER_LOADOUT_ID]
    }
}

impl Loadout {
    /// Seed `inventory` from this loadout: equip each equipment entry into its
    /// slot (ammo gets its quantity), then fill free backpack slots with the
    /// backpack entries. Items that find no free backpack slot are warned and
    /// dropped rather than silently lost.
    pub fn apply_to(&self, inventory: &mut Inventory) {
        for entry in &self.equipment {
            let item = EquippedItem {
                type_id: entry.type_id.clone(),
                properties: entry.properties.clone(),
                modifiers: entry.modifiers.clone(),
            };
            if entry.slot == EquipmentSlot::Ammo {
                inventory.set_ammo(item, entry.quantity);
            } else {
                inventory.restore_equipment_item(entry.slot, item);
            }
        }

        for entry in &self.backpack {
            let Some(slot) = inventory
                .backpack_slots
                .iter_mut()
                .find(|slot| slot.is_none())
            else {
                warn!(
                    "starting loadout: no free backpack slot for '{}' — dropping it",
                    entry.type_id
                );
                continue;
            };
            *slot = Some(InventoryStack::with_modifiers(
                entry.type_id.clone(),
                entry.properties.clone(),
                entry.quantity,
                entry.modifiers.clone(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_starter_loadout_parses_and_seeds() {
        // The bundled `assets/loadouts/starter.yaml` must always load.
        let loadouts = Loadouts::load_from_disk();
        assert!(
            loadouts.get("no_such_loadout").is_none(),
            "unknown loadout ids must return None"
        );
        let mut inventory = Inventory::default();
        loadouts.starter().apply_to(&mut inventory);
        assert!(
            inventory
                .equipment_slots
                .iter()
                .any(|(_, item)| item.is_some()),
            "starter loadout should equip at least one item"
        );
    }

    #[test]
    fn authored_loadouts_validate_against_object_defs() {
        let loadouts = Loadouts::load_from_disk();
        let objects =
            crate::world::object_definitions::OverworldObjectDefinitions::load_from_disk();
        // Should not panic — every authored loadout must reference real items
        // in their declared equipment slots.
        loadouts.validate_against(&objects);
    }

    #[test]
    fn applies_equipment_ammo_and_backpack() {
        let yaml = r#"
equipment:
  - slot: weapon
    type_id: bow
  - slot: ammo
    type_id: arrow
    quantity: 20
backpack:
  - type_id: apple
    quantity: 3
  - type_id: pickaxe
"#;
        let loadout: Loadout = serde_yaml::from_str(yaml).unwrap();
        let mut inventory = Inventory::default();
        loadout.apply_to(&mut inventory);

        assert_eq!(
            inventory
                .equipment_item(EquipmentSlot::Weapon)
                .map(|i| i.type_id.as_str()),
            Some("bow")
        );
        assert_eq!(
            inventory
                .equipment_item(EquipmentSlot::Ammo)
                .map(|i| i.type_id.as_str()),
            Some("arrow")
        );
        assert_eq!(inventory.ammo_quantity, 20);

        let backpack: Vec<_> = inventory.backpack_slots.iter().flatten().collect();
        assert_eq!(backpack.len(), 2);
        assert_eq!(backpack[0].type_id, "apple");
        assert_eq!(backpack[0].quantity, 3);
        assert_eq!(backpack[1].type_id, "pickaxe");
        assert_eq!(backpack[1].quantity, 1, "quantity defaults to 1");
    }

    #[test]
    fn equipment_modifiers_round_trip_onto_item() {
        let yaml = r#"
equipment:
  - slot: weapon
    type_id: bronze_sword
    modifiers:
      - type_ex: flaming
        lvl: 1
        label: "Flaming (+1d6 fire)"
        effect:
          kind: bonus_damage
          dice: [1, 6]
          damage_type: fire
        duration:
          kind: permanent
"#;
        let loadout: Loadout = serde_yaml::from_str(yaml).unwrap();
        let mut inventory = Inventory::default();
        loadout.apply_to(&mut inventory);
        let weapon = inventory.equipment_item(EquipmentSlot::Weapon).unwrap();
        assert_eq!(weapon.modifiers.len(), 1);
        assert_eq!(weapon.modifiers[0].type_ex, "flaming");
    }
}
