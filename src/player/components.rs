use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::damage_expr::DamageExpr;
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::{EquipmentSlot, OverworldObjectDefinitions};

#[derive(Component, Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WeaponDamage(pub DamageExpr);

impl Default for WeaponDamage {
    fn default() -> Self {
        Self(DamageExpr::melee_default())
    }
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct DefenseStats {
    pub armor: i32,
    pub block: i32,
    pub dodge_bonus: i32,
    /// Percentage 0-100. For players this comes from the equipped shield's
    /// definition; for NPCs it comes from the creature's own definition.
    pub block_chance: i32,
}

#[derive(Component)]
pub struct Player;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct PlayerId(pub u64);

#[derive(Component, Clone, Debug, Eq, PartialEq)]
pub struct PlayerIdentity {
    pub id: PlayerId,
    /// Human-readable name shown in chat lines and other UI. Sourced from the
    /// account DB's `character_name` (falling back to `username`) at login. For
    /// fresh test spawns the `new` constructor generates a `Player#<id>`
    /// placeholder so unit tests don't need to plumb a real name.
    pub display_name: String,
    /// Where this character respawns after death. `None` means "use map
    /// center" — fresh characters start with no explicit home, then can set
    /// one with the `SetHome` command. Restored from the account DB on login.
    pub home_position: Option<(
        crate::world::components::SpaceId,
        crate::world::components::TilePosition,
    )>,
}

impl PlayerIdentity {
    pub fn new(id: PlayerId) -> Self {
        Self {
            id,
            display_name: format!("Player#{}", id.0),
            home_position: None,
        }
    }

    pub fn with_display_name(id: PlayerId, display_name: String) -> Self {
        Self {
            id,
            display_name,
            home_position: None,
        }
    }
}

/// A self-describing stack of identical items. Carries the canonical type
/// (matching a directory under `assets/overworld_objects/`) plus any
/// per-instance `properties` (e.g. a scroll's `spell_id`). Notably *no* runtime
/// `object_id` — runtime ids are allocated only when the stack leaves the
/// inventory and becomes a world entity, so saves stay portable across map
/// edits.
///
/// `contained_slots` holds the contents of a *container item* (a pouch) while
/// it lives in inventory. `None` for non-container items. The vector length
/// matches the definition's `container_capacity`. Nested storable containers
/// are forbidden at placement time (`accepts_storable_containers: false` on
/// the pouch base), so contents are themselves never pouches — depth bounded.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct InventoryStack {
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    pub quantity: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contained_slots: Option<Vec<Option<InventoryStack>>>,
}

/// Key under `ObjectProperties` (string-keyed map) where a per-instance
/// `charges_remaining: u32` value lives for items whose definition declares
/// `max_charges`. Stored as a stringified decimal because `ObjectProperties` is
/// `HashMap<String, String>` (already serialised, replicated, and persisted).
pub const CHARGES_KEY: &str = "charges_remaining";

impl InventoryStack {
    /// Construct a non-container stack with empty `contained_slots`. Use this
    /// at every site that doesn't specifically intend to populate pouch
    /// contents.
    pub fn item(type_id: impl Into<String>, properties: ObjectProperties, quantity: u32) -> Self {
        Self {
            type_id: type_id.into(),
            properties,
            quantity,
            contained_slots: None,
        }
    }

    /// Read this stack's `charges_remaining` property, parsing it as `u32`.
    /// Returns `None` when the key is missing or unparseable — callers should
    /// fall back to the definition's `max_charges` for that case (legacy stacks
    /// spawned before the field was authored).
    pub fn charges_remaining(&self) -> Option<u32> {
        self.properties
            .get(CHARGES_KEY)
            .and_then(|s| s.parse().ok())
    }

    pub fn set_charges_remaining(&mut self, charges: u32) {
        self.properties
            .insert(CHARGES_KEY.to_string(), charges.to_string());
    }
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
        Some(InventoryStack::item(
            item.type_id.clone(),
            item.properties.clone(),
            self.ammo_quantity,
        ))
    }

    pub fn set_ammo(&mut self, item: EquippedItem, quantity: u32) {
        self.restore_equipment_item(EquipmentSlot::Ammo, item);
        self.ammo_quantity = quantity;
    }

    /// Total carried weight across backpack slots, equipment slots, ammo
    /// counter, and any nested pouch contents. Items missing from
    /// `definitions` are treated as weightless (e.g. legacy saves with
    /// renamed types). The recursion bottoms out via the
    /// `accepts_storable_containers: false` rule on pouches.
    pub fn total_weight(&self, definitions: &OverworldObjectDefinitions) -> f32 {
        let mut total = 0.0;
        for stack in self.backpack_slots.iter().flatten() {
            total += stack_weight(stack, definitions);
        }
        for (slot, equipped) in &self.equipment_slots {
            let Some(item) = equipped else {
                continue;
            };
            let per_item = definitions.get(&item.type_id).map_or(0.0, |d| d.weight);
            let count = if *slot == EquipmentSlot::Ammo {
                self.ammo_quantity.max(1) as f32
            } else {
                1.0
            };
            total += per_item * count;
        }
        total
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

/// Weight (in kg) of a single inventory stack including nested pouch
/// contents. The pouch itself counts at its own weight (per definition); the
/// nested item weights are added on top.
pub fn stack_weight(stack: &InventoryStack, definitions: &OverworldObjectDefinitions) -> f32 {
    let per = definitions.get(&stack.type_id).map_or(0.0, |d| d.weight);
    let mut total = per * stack.quantity as f32;
    if let Some(inner) = &stack.contained_slots {
        for inner_stack in inner.iter().flatten() {
            total += stack_weight(inner_stack, definitions);
        }
    }
    total
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
                "[Narrator]: Press T to chat with players nearby.".to_owned(),
            ],
            max_lines: 64,
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

/// Point-buy rules used by character creation:
/// 6 attributes start at 10, pool of 12 points to distribute, each attribute
/// clamped to [8, 18]. Cost = 1 point per +1 above 10 (negatives refund).
/// Total spend must equal exactly the budget.
pub const POINT_BUY_BUDGET: i32 = 12;
pub const ATTR_FLOOR: i32 = 8;
pub const ATTR_CEILING: i32 = 18;
pub const ATTR_BASELINE: i32 = 10;

/// Validates an `AttributeSet` against the character-creation point-buy rules.
/// Shared between the client UI (to enable/disable the Create button) and the
/// server (to reject crafted `CreateCharacter` messages).
pub fn validate_point_buy(attrs: &AttributeSet) -> Result<(), String> {
    let values = [
        ("strength", attrs.strength),
        ("agility", attrs.agility),
        ("constitution", attrs.constitution),
        ("willpower", attrs.willpower),
        ("charisma", attrs.charisma),
        ("focus", attrs.focus),
    ];
    let mut total = 0;
    for (name, value) in values {
        if value < ATTR_FLOOR || value > ATTR_CEILING {
            return Err(format!(
                "{name} must be between {ATTR_FLOOR} and {ATTR_CEILING}"
            ));
        }
        total += value - ATTR_BASELINE;
    }
    if total != POINT_BUY_BUDGET {
        return Err(format!(
            "must spend exactly {POINT_BUY_BUDGET} points (currently {total})"
        ));
    }
    Ok(())
}

/// Names of the six attributes on `AttributeSet`, used by string-keyed admin
/// tooling (`Player.set_attribute("agility", 18)`) and by snapshot rendering.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AttributeKind {
    Strength,
    Agility,
    Constitution,
    Willpower,
    Charisma,
    Focus,
}

impl AttributeKind {
    pub const ALL: [AttributeKind; 6] = [
        AttributeKind::Strength,
        AttributeKind::Agility,
        AttributeKind::Constitution,
        AttributeKind::Willpower,
        AttributeKind::Charisma,
        AttributeKind::Focus,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            AttributeKind::Strength => "strength",
            AttributeKind::Agility => "agility",
            AttributeKind::Constitution => "constitution",
            AttributeKind::Willpower => "willpower",
            AttributeKind::Charisma => "charisma",
            AttributeKind::Focus => "focus",
        }
    }

    /// Parse from full name or short alias (`str`/`agi`/`con`/`wil`/`cha`/`foc`).
    /// Also accepts D&D-style synonyms (`dex` → agility, `wis` → willpower,
    /// `int` → focus) so admins from a D&D background don't trip up.
    pub fn from_label(s: &str) -> Option<AttributeKind> {
        match s.to_ascii_lowercase().as_str() {
            "strength" | "str" => Some(Self::Strength),
            "agility" | "agi" | "dex" => Some(Self::Agility),
            "constitution" | "con" => Some(Self::Constitution),
            "willpower" | "wil" | "wis" => Some(Self::Willpower),
            "charisma" | "cha" => Some(Self::Charisma),
            "focus" | "foc" | "int" => Some(Self::Focus),
            _ => None,
        }
    }

    pub fn read(self, attrs: &AttributeSet) -> i32 {
        match self {
            Self::Strength => attrs.strength,
            Self::Agility => attrs.agility,
            Self::Constitution => attrs.constitution,
            Self::Willpower => attrs.willpower,
            Self::Charisma => attrs.charisma,
            Self::Focus => attrs.focus,
        }
    }

    pub fn write(self, attrs: &mut AttributeSet, value: i32) {
        match self {
            Self::Strength => attrs.strength = value,
            Self::Agility => attrs.agility = value,
            Self::Constitution => attrs.constitution = value,
            Self::Willpower => attrs.willpower = value,
            Self::Charisma => attrs.charisma = value,
            Self::Focus => attrs.focus = value,
        }
    }
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

/// Soft and hard carry-weight caps in kg, derived from `BaseStats`. The soft
/// cap triggers the `Encumbered` slow-walk state; the hard cap is the
/// authoritative reject threshold (pickup fails above this). Recomputed by
/// `refresh_derived_player_stats` whenever stats change.
#[derive(Component, Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MaxCarryWeight {
    pub soft_cap: f32,
    pub hard_cap: f32,
}

impl MaxCarryWeight {
    pub fn from_strength(strength: i32) -> Self {
        // [tunable] mirrors progression.md §10. STR 10 → 40 kg soft / 60 kg hard.
        let soft = (20.0 + strength.max(0) as f32 * 2.0).max(5.0);
        let hard = soft * 1.5;
        Self {
            soft_cap: soft,
            hard_cap: hard,
        }
    }
}

/// Cached total weight (kg) of the player's carried inventory + equipment.
/// Server-authoritative; replicated to the client via
/// `GameEvent::PlayerCarryWeightChanged`.
#[derive(Component, Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CurrentCarryWeight(pub f32);

/// Marker: total carry weight currently exceeds `MaxCarryWeight::soft_cap`.
/// Drives the slow-walk movement penalty and the HUD encumbrance icon.
#[derive(Component, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Encumbered;

impl Default for DerivedStats {
    fn default() -> Self {
        let base = BaseStats::default();
        Self::from_base(&base)
    }
}

impl DerivedStats {
    pub fn from_base(base: &BaseStats) -> Self {
        Self::from_base_with_class(base, crate::player::classes::Class::Fighter, 1)
    }

    /// Class- and level-aware derivation. At `level = 1` this returns the same
    /// numbers as the legacy `from_base` for any class — level scaling only
    /// kicks in from level 2 onward, matching `progression.md` §4.3.
    pub fn from_base_with_class(
        base: &BaseStats,
        class: crate::player::classes::Class,
        level: u32,
    ) -> Self {
        use crate::player::classes::{ability_mod, class_data, CastingAttribute};

        let attributes = base.attributes.clamped_min(1);
        let con_mod = ability_mod(attributes.constitution);

        let level_above_1 = level.saturating_sub(1) as i32;
        let class = class_data(class);

        // Average HP per level: floor(HD / 2) + 1 + CON_mod, min 1.
        let hp_per_level = ((class.hit_die as i32) / 2 + 1 + con_mod).max(1);
        let level_hp = level_above_1 * hp_per_level;

        // Mana per level: class table + casting attribute modifier, min 0.
        let cast_mod = match class.casting_attribute {
            Some(CastingAttribute::Focus) => ability_mod(attributes.focus),
            Some(CastingAttribute::Willpower) => ability_mod(attributes.willpower),
            None => 0,
        };
        let mana_per_level = ((class.mana_per_level as i32) + cast_mod).max(0);
        let level_mana = level_above_1 * mana_per_level;

        let max_health = (35
            + attributes.constitution * 6
            + attributes.strength * 2
            + base.max_health
            + level_hp)
            .max(1);
        let max_mana =
            (10 + attributes.willpower * 6 + attributes.focus * 3 + base.max_mana + level_mana)
                .max(0);
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

/// Per-player accumulators that drive slow HP/MP regeneration. One unit is
/// added to `health` / `mana` each time the corresponding `*_remaining`
/// counter ticks below zero; the counter is then reset based on the player's
/// stats (constitution drives HP, willpower drives MP) and any active
/// `RegenBuffs` multiplier. Not persisted — regen state always starts fresh
/// on login because the timing is sub-second.
#[derive(Component, Clone, Copy, Debug)]
pub struct RegenTickers {
    pub health_remaining: f32,
    pub mana_remaining: f32,
}

impl Default for RegenTickers {
    fn default() -> Self {
        Self {
            health_remaining: 0.0,
            mana_remaining: 0.0,
        }
    }
}

/// Active regen-rate multiplier from food/drink. Inert when
/// `remaining_seconds <= 0` or `multiplier <= 1.0`. Re-eating extends the
/// duration: `remaining_seconds += new.duration`, and `multiplier` snaps to
/// the strongest of the two so a stronger buff isn't diluted by a weaker
/// follow-up. Not persisted across sessions — buffs reset on disconnect.
#[derive(Component, Clone, Copy, Debug)]
pub struct RegenBuffs {
    pub multiplier: f32,
    pub remaining_seconds: f32,
}

impl Default for RegenBuffs {
    fn default() -> Self {
        Self {
            multiplier: 1.0,
            remaining_seconds: 0.0,
        }
    }
}

impl RegenBuffs {
    pub fn is_active(&self) -> bool {
        self.remaining_seconds > 0.0 && self.multiplier > 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charges_remaining_round_trips_through_properties() {
        let mut stack = InventoryStack::item("wand_of_sparks", ObjectProperties::new(), 1);
        assert_eq!(stack.charges_remaining(), None);

        stack.set_charges_remaining(30);
        assert_eq!(stack.charges_remaining(), Some(30));
        assert_eq!(
            stack.properties.get(CHARGES_KEY).map(String::as_str),
            Some("30")
        );

        stack.set_charges_remaining(0);
        assert_eq!(stack.charges_remaining(), Some(0));
    }

    #[test]
    fn charges_remaining_returns_none_for_unparseable_value() {
        let mut stack = InventoryStack::item("wand_of_sparks", ObjectProperties::new(), 1);
        stack
            .properties
            .insert(CHARGES_KEY.to_string(), "garbage".to_string());
        assert_eq!(stack.charges_remaining(), None);
    }
}
