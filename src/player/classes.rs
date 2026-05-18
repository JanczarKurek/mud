//! Player classes (Fighter / Wizard / Cleric / Vagabond).
//!
//! See `docs/progression.md` §3 for design + per-class table values, and §7.4
//! for BAB / save progression formulas. The §7 combat-math rewrite that uses
//! BAB and saves in the to-hit / damage formulas is **out of scope** for this
//! batch — these helpers ship now so future work can plug straight in.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Which attribute drives a caster's mana / spell DC. `None` for non-casters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum CastingAttribute {
    /// Wizards key off Focus (= 3.5e INT).
    Focus,
    /// Clerics key off Willpower (= 3.5e WIS).
    Willpower,
}

/// BAB advancement track per `progression.md` §7.4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BabTrack {
    /// `+1 / level` (Fighter).
    Full,
    /// `+3 / 4 levels` (Cleric, Vagabond).
    ThreeQuarter,
    /// `+1 / 2 levels` (Wizard).
    Half,
}

/// Which saves a class has on the "good" track (others use "poor").
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GoodSaves {
    pub fortitude: bool,
    pub reflex: bool,
    pub will: bool,
}

impl GoodSaves {
    pub const FORT: Self = Self {
        fortitude: true,
        reflex: false,
        will: false,
    };
    pub const REF: Self = Self {
        fortitude: false,
        reflex: true,
        will: false,
    };
    pub const WILL: Self = Self {
        fortitude: false,
        reflex: false,
        will: true,
    };
    pub const FORT_WILL: Self = Self {
        fortitude: true,
        reflex: false,
        will: true,
    };
}

/// The four base classes shipping in v1.
#[derive(Component, Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Class {
    #[default]
    Fighter,
    Wizard,
    Cleric,
    Vagabond,
}

impl Class {
    pub const fn label(self) -> &'static str {
        match self {
            Class::Fighter => "Fighter",
            Class::Wizard => "Wizard",
            Class::Cleric => "Cleric",
            Class::Vagabond => "Vagabond",
        }
    }

    pub const ALL: [Class; 4] = [
        Class::Fighter,
        Class::Wizard,
        Class::Cleric,
        Class::Vagabond,
    ];

    /// Parse a `Class` from its `label()` form, case-insensitive. Used by the
    /// admin REPL (`Player.set_class("Vagabond")`).
    pub fn from_label(s: &str) -> Option<Class> {
        Class::ALL
            .iter()
            .copied()
            .find(|c| c.label().eq_ignore_ascii_case(s))
    }
}

/// Per-class fixed data. `[tunable]` per `progression.md` §10.
#[derive(Clone, Copy, Debug)]
pub struct ClassData {
    pub hit_die: u32,
    pub bab_track: BabTrack,
    pub good_saves: GoodSaves,
    pub skill_points_per_level: u32,
    pub mana_per_level: u32,
    pub casting_attribute: Option<CastingAttribute>,
}

pub const fn class_data(c: Class) -> ClassData {
    match c {
        Class::Fighter => ClassData {
            hit_die: 10,
            bab_track: BabTrack::Full,
            good_saves: GoodSaves::FORT,
            skill_points_per_level: 2,
            mana_per_level: 0,
            casting_attribute: None,
        },
        Class::Wizard => ClassData {
            hit_die: 4,
            bab_track: BabTrack::Half,
            good_saves: GoodSaves::WILL,
            skill_points_per_level: 2,
            mana_per_level: 10,
            casting_attribute: Some(CastingAttribute::Focus),
        },
        Class::Cleric => ClassData {
            hit_die: 8,
            bab_track: BabTrack::ThreeQuarter,
            good_saves: GoodSaves::FORT_WILL,
            skill_points_per_level: 2,
            mana_per_level: 8,
            casting_attribute: Some(CastingAttribute::Willpower),
        },
        Class::Vagabond => ClassData {
            hit_die: 6,
            bab_track: BabTrack::ThreeQuarter,
            good_saves: GoodSaves::REF,
            skill_points_per_level: 8,
            mana_per_level: 0,
            casting_attribute: None,
        },
    }
}

/// BAB at `level`, per `progression.md` §7.4.
pub fn bab_at(track: BabTrack, level: u32) -> i32 {
    let l = level as i32;
    match track {
        BabTrack::Full => l,
        BabTrack::ThreeQuarter => (3 * l) / 4,
        BabTrack::Half => l / 2,
    }
}

/// Good-save bonus at `level`: `2 + level / 2`.
pub fn good_save_at(level: u32) -> i32 {
    2 + (level as i32) / 2
}

/// Poor-save bonus at `level`: `level / 3`.
pub fn poor_save_at(level: u32) -> i32 {
    (level as i32) / 3
}

/// 3.5e ability modifier: `(score - 10) / 2`, rounded toward -∞.
pub fn ability_mod(score: i32) -> i32 {
    if score >= 10 {
        (score - 10) / 2
    } else {
        // Round toward -∞ for sub-10 scores: ((score - 10) - 1) / 2 in C-style
        // truncation gives the right answer.
        ((score - 10) - 1) / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_mod_anchors() {
        assert_eq!(ability_mod(10), 0);
        assert_eq!(ability_mod(12), 1);
        assert_eq!(ability_mod(14), 2);
        assert_eq!(ability_mod(8), -1);
        assert_eq!(ability_mod(6), -2);
        assert_eq!(ability_mod(1), -5);
        assert_eq!(ability_mod(20), 5);
    }

    #[test]
    fn bab_progressions() {
        assert_eq!(bab_at(BabTrack::Full, 1), 1);
        assert_eq!(bab_at(BabTrack::Full, 20), 20);
        assert_eq!(bab_at(BabTrack::ThreeQuarter, 1), 0);
        assert_eq!(bab_at(BabTrack::ThreeQuarter, 4), 3);
        assert_eq!(bab_at(BabTrack::ThreeQuarter, 20), 15);
        assert_eq!(bab_at(BabTrack::Half, 1), 0);
        assert_eq!(bab_at(BabTrack::Half, 2), 1);
        assert_eq!(bab_at(BabTrack::Half, 20), 10);
    }

    #[test]
    fn save_progressions() {
        assert_eq!(good_save_at(1), 2);
        assert_eq!(good_save_at(2), 3);
        assert_eq!(good_save_at(20), 12);
        assert_eq!(poor_save_at(1), 0);
        assert_eq!(poor_save_at(3), 1);
        assert_eq!(poor_save_at(20), 6);
    }

    #[test]
    fn class_data_anchors() {
        assert_eq!(class_data(Class::Fighter).hit_die, 10);
        assert_eq!(class_data(Class::Wizard).hit_die, 4);
        assert_eq!(class_data(Class::Cleric).hit_die, 8);
        assert_eq!(class_data(Class::Vagabond).hit_die, 6);
        assert_eq!(class_data(Class::Vagabond).skill_points_per_level, 8);
        assert!(class_data(Class::Wizard).casting_attribute.is_some());
        assert!(class_data(Class::Fighter).casting_attribute.is_none());
    }
}
