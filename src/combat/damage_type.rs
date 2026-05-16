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
}
