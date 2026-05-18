use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    #[default]
    Blunt,
    Cut,
    Pierce,
    Fire,
    Frost,
    Earth,
    Lightning,
    Poison,
    Acid,
    Death,
    Holy,
    Arcane,
}

impl DamageType {
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Blunt => "blunt",
            Self::Cut => "cut",
            Self::Pierce => "pierce",
            Self::Fire => "fire",
            Self::Frost => "frost",
            Self::Earth => "earth",
            Self::Lightning => "lightning",
            Self::Poison => "poison",
            Self::Acid => "acid",
            Self::Death => "death",
            Self::Holy => "holy",
            Self::Arcane => "arcane",
        }
    }

    /// VFX definition id the damage drainer spawns by default when a hit of
    /// this type lands. Matches the directory names under `assets/vfx/`.
    pub const fn default_hit_vfx_id(self) -> &'static str {
        match self {
            Self::Blunt => "blunt_hit",
            Self::Cut => "cut_hit",
            Self::Pierce => "pierce_hit",
            Self::Fire => "fire_hit",
            Self::Frost => "frost_hit",
            Self::Earth => "earth_hit",
            Self::Lightning => "lightning_hit",
            Self::Poison => "poison_hit",
            Self::Acid => "acid_hit",
            Self::Death => "death_hit",
            Self::Holy => "holy_hit",
            Self::Arcane => "arcane_hit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_variant_as_snake_case() {
        for (variant, name) in [
            (DamageType::Blunt, "blunt"),
            (DamageType::Cut, "cut"),
            (DamageType::Pierce, "pierce"),
            (DamageType::Fire, "fire"),
            (DamageType::Frost, "frost"),
            (DamageType::Earth, "earth"),
            (DamageType::Lightning, "lightning"),
            (DamageType::Poison, "poison"),
            (DamageType::Acid, "acid"),
            (DamageType::Death, "death"),
            (DamageType::Holy, "holy"),
            (DamageType::Arcane, "arcane"),
        ] {
            let yaml = serde_yaml::to_string(&variant).unwrap();
            assert_eq!(yaml.trim(), name);
            let parsed: DamageType = serde_yaml::from_str(name).unwrap();
            assert_eq!(parsed, variant);
            assert_eq!(variant.display_name(), name);
        }
    }

    #[test]
    fn default_is_blunt() {
        assert_eq!(DamageType::default(), DamageType::Blunt);
    }

    #[test]
    fn every_variant_has_a_non_empty_default_hit_vfx_id() {
        for variant in [
            DamageType::Blunt,
            DamageType::Cut,
            DamageType::Pierce,
            DamageType::Fire,
            DamageType::Frost,
            DamageType::Earth,
            DamageType::Lightning,
            DamageType::Poison,
            DamageType::Acid,
            DamageType::Death,
            DamageType::Holy,
            DamageType::Arcane,
        ] {
            let id = variant.default_hit_vfx_id();
            assert!(!id.is_empty(), "{variant:?} has empty vfx id");
            assert!(
                id.ends_with("_hit"),
                "{variant:?} -> {id} missing _hit suffix"
            );
        }
    }
}
