use bevy::prelude::*;

#[derive(Component)]
pub struct Player;

#[derive(Clone, Copy, Debug, Default)]
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

#[derive(Component)]
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

#[derive(Component)]
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

#[derive(Component)]
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

#[derive(Component)]
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
