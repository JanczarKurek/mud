use std::collections::HashMap;

use bevy::prelude::*;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::assets::discover_yaml_assets;
use crate::combat::damage_expr::DamageExpr;
use crate::combat::damage_type::DamageType;
use crate::player::classes::Class;
use crate::player::components::AttributeSet;

/// A spell's damage, as a roll expression keyed off the caster's attributes and
/// level (e.g. `"3d6 + focus/2 + level/2"`). Deserializes from either a YAML
/// **number** (flat damage — back-compat with the old `damage: 18.0` form, where
/// `0`/absent means "no damage") or a **string** `DamageExpr`. This lets each
/// spell scale however its author wants, mirroring weapon `damage:` expressions.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpellDamage(pub Option<DamageExpr>);

impl SpellDamage {
    /// True if this spell can deal damage (it has a damage expression).
    pub fn deals_damage(&self) -> bool {
        self.0.is_some()
    }

    /// Roll the damage for a caster with the given attributes and level.
    /// Returns `0.0` when the spell deals no damage.
    pub fn roll(&self, attrs: &AttributeSet, caster_level: u32) -> f32 {
        match &self.0 {
            Some(expr) => expr.roll(attrs, caster_level as i32).max(0) as f32,
            None => 0.0,
        }
    }

    /// Maximum possible damage at the given attributes/level — for UI tooltips.
    pub fn max(&self, attrs: &AttributeSet, caster_level: u32) -> f32 {
        match &self.0 {
            Some(expr) => expr.max_damage(attrs, caster_level as i32).max(0) as f32,
            None => 0.0,
        }
    }

    /// A tooltip-friendly damage figure evaluated at a neutral baseline
    /// (all-10 attributes, level 1). Returns a range like `"8–12"` for dice
    /// expressions, a single number otherwise, and `None` for non-damaging
    /// spells. Actual in-game damage scales with the caster's stats and level.
    pub fn tooltip_value(&self) -> Option<String> {
        let expr = self.0.as_ref()?;
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 10);
        let lo = expr.min_damage(&attrs, 1).max(0);
        let hi = expr.max_damage(&attrs, 1).max(0);
        Some(if lo == hi {
            format!("{lo}")
        } else {
            format!("{lo}\u{2013}{hi}")
        })
    }
}

fn flat_damage_expr(value: f64) -> Option<DamageExpr> {
    let n = value.round() as i32;
    if n <= 0 {
        None
    } else {
        Some(DamageExpr {
            dice: None,
            stats: Vec::new(),
            level: Vec::new(),
            bonus: n,
        })
    }
}

impl<'de> Deserialize<'de> for SpellDamage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DamageVisitor;
        impl<'de> Visitor<'de> for DamageVisitor {
            type Value = SpellDamage;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a damage number or a damage-expression string")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(SpellDamage(flat_damage_expr(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(SpellDamage(flat_damage_expr(v as f64)))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(SpellDamage(flat_damage_expr(v as f64)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let expr = DamageExpr::parse(v).map_err(E::custom)?;
                Ok(SpellDamage(Some(expr)))
            }
            // `null` / absent -> no damage (symmetric with serializing `None`).
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(SpellDamage(None))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(SpellDamage(None))
            }
            // The full `DamageExpr` struct form — keeps deserialize symmetric
            // with the derived struct `Serialize` so a spell round-trips.
            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let expr = DamageExpr::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(SpellDamage(Some(expr)))
            }
        }
        deserializer.deserialize_any(DamageVisitor)
    }
}

/// A YAML-authorable amount that is either a **flat number** (kept as f32 so
/// fractional multipliers like haste `0.7` survive) or a **roll expression**
/// keyed off the caster's attributes/level (e.g. `"2d8+wil_mod*2+level"`).
/// Used for heal/restore amounts and effect magnitudes; unlike `SpellDamage`,
/// a flat value is preserved verbatim rather than rounded/dropped.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ScalableAmount {
    Flat(f32),
    Expr(DamageExpr),
}

impl Default for ScalableAmount {
    fn default() -> Self {
        ScalableAmount::Flat(0.0)
    }
}

impl ScalableAmount {
    /// Evaluate for a caster: flat values pass through, expressions roll.
    pub fn resolve(&self, attrs: &AttributeSet, caster_level: u32) -> f32 {
        match self {
            ScalableAmount::Flat(v) => *v,
            ScalableAmount::Expr(expr) => expr.roll(attrs, caster_level as i32).max(0) as f32,
        }
    }

    /// True when this amount can never contribute anything (flat ≤ 0).
    /// Expressions count as non-zero without evaluating.
    pub fn is_zero(&self) -> bool {
        matches!(self, ScalableAmount::Flat(v) if *v <= 0.0)
    }

    /// Tooltip figure at the neutral baseline (all-10 attrs, level 1):
    /// a range like `"9–17"` for dice expressions, a plain number for flat
    /// values, `None` when zero.
    pub fn tooltip_value(&self) -> Option<String> {
        match self {
            ScalableAmount::Flat(v) if *v > 0.0 => Some(format!("{v:.0}")),
            ScalableAmount::Flat(_) => None,
            ScalableAmount::Expr(expr) => {
                let attrs = AttributeSet::new(10, 10, 10, 10, 10, 10);
                let lo = expr.min_damage(&attrs, 1).max(0);
                let hi = expr.max_damage(&attrs, 1).max(0);
                Some(if lo == hi {
                    format!("{lo}")
                } else {
                    format!("{lo}\u{2013}{hi}")
                })
            }
        }
    }
}

impl<'de> Deserialize<'de> for ScalableAmount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AmountVisitor;
        impl<'de> Visitor<'de> for AmountVisitor {
            type Value = ScalableAmount;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a number or a roll-expression string")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(ScalableAmount::Flat(v as f32))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(ScalableAmount::Flat(v as f32))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(ScalableAmount::Flat(v as f32))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let expr = DamageExpr::parse(v).map_err(E::custom)?;
                Ok(ScalableAmount::Expr(expr))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(ScalableAmount::Flat(0.0))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(ScalableAmount::Flat(0.0))
            }
            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let expr = DamageExpr::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(ScalableAmount::Expr(expr))
            }
        }
        deserializer.deserialize_any(AmountVisitor)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpellDefinition {
    pub name: String,
    pub incantation: String,
    pub mana_cost: f32,
    pub targeting: SpellTargeting,
    #[serde(default)]
    pub range_tiles: i32,
    /// Classes permitted to cast this spell directly (via a memorized-spell
    /// path that does not exist yet — Phase E). Empty = any class. Scrolls
    /// bypass this gate; see `check_caster_eligibility` in `game::systems`.
    #[serde(default)]
    pub class_access: Vec<Class>,
    /// Minimum caster level. `0` = anyone. Enforced on every cast path,
    /// including scrolls.
    #[serde(default)]
    pub min_caster_level: u32,
    #[serde(default)]
    pub effects: SpellEffects,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpellEffects {
    #[serde(default)]
    pub damage: SpellDamage,
    /// Damage type for damaging spells. Defaults to `Arcane` when omitted
    /// — see `effective_damage_type`.
    #[serde(default)]
    pub damage_type: Option<DamageType>,
    /// HP restored on the caster/target. A flat number (back-compat) or a
    /// roll expression keyed off the caster (e.g. `"2d8+wil_mod*2+level"`).
    #[serde(default)]
    pub restore_health: ScalableAmount,
    /// Mana restored — same forms as `restore_health`.
    #[serde(default)]
    pub restore_mana: ScalableAmount,
    /// Timed buffs applied to the caster. Magnitudes may be expressions;
    /// resolved to plain `EffectSpec`s at cast time.
    #[serde(default)]
    pub buffs_self: Vec<ScalableEffectSpec>,
    /// Timed debuffs applied to the targeted NPC. Ignored for untargeted casts.
    /// Magnitudes may be expressions; resolved at cast time.
    #[serde(default)]
    pub buffs_target: Vec<ScalableEffectSpec>,
    /// Effect kinds to remove from the caster after other effects apply.
    /// Drives Cleric "Restore" clearing Slow/Sleep on self.
    #[serde(default)]
    pub clears_self: Vec<EffectKind>,
    /// Spawn a transient world object at the cast location.
    #[serde(default)]
    pub spawns_object: Option<SpawnObjectSpec>,
    /// Summon a timed friendly creature (a companion) that fights on the
    /// caster's side. Its kills credit the caster. Only meaningful for
    /// `targeted_tile` spells (the creature appears at the target tile).
    #[serde(default)]
    pub summons_creature: Option<SummonSpec>,
    /// Deal `damage` to every entity within `aoe.radius_tiles` Chebyshev
    /// distance of the target tile. Only meaningful for tile-target spells.
    #[serde(default)]
    pub aoe: Option<AoeSpec>,
    /// When set, the spell fires a flying missile and its damage/AoE only
    /// resolves when the missile lands (`travel = distance / speed`). For
    /// entity-target spells the missile homes — it hits the locked target
    /// wherever it moved. `None` keeps the instantaneous behavior. See
    /// `tick_scheduled_impacts` in `combat::scheduled`.
    #[serde(default)]
    pub projectile: Option<ProjectileSpec>,
    /// VFX definition id played on the caster at cast time. `None` falls back
    /// to `"cast_flash"` in the trigger code.
    #[serde(default)]
    pub vfx_on_cast: Option<String>,
    /// VFX definition id played on the target on a targeted hit. `None` falls
    /// back to `"hit_flash"` for damaging spells; healing spells should
    /// override with `"heal_sparkle"`.
    #[serde(default)]
    pub vfx_on_target_hit: Option<String>,
    /// Enchant the targeted item (any of the caster's inventory/equipment
    /// slots) with this modifier. Only meaningful for `targeted_item` spells.
    /// Routed through `combat::modifiers::apply_modifier`, so the per-item
    /// TYPE_EX/LVL anti-stack rule applies.
    #[serde(default)]
    pub enchant_item: Option<crate::combat::modifiers::ItemModifier>,
}

impl SpellEffects {
    /// Resolve the damage type, defaulting to `Arcane` when unspecified.
    /// Only meaningful for `damage > 0` spells.
    pub fn effective_damage_type(&self) -> DamageType {
        self.damage_type.unwrap_or(DamageType::Arcane)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct EffectSpec {
    pub kind: EffectKind,
    pub magnitude: f32,
    pub seconds: f32,
    /// Optional second parameter. Currently only `Chill` reads it, as the
    /// slow multiplier paired with the DOT magnitude.
    #[serde(default)]
    pub secondary_magnitude: Option<f32>,
}

/// YAML-side effect spec whose `magnitude` may scale with the caster
/// (`"1+foc_mod/2+level/4"` burning ticks). Resolved to a plain, `Copy`,
/// persistence-safe [`EffectSpec`] at cast time — `EffectSpec` itself is
/// embedded in persisted `ItemModifier`s and trap defs, so it must not grow
/// an expression. A bare-number `magnitude` keeps every existing YAML loading.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct ScalableEffectSpec {
    pub kind: EffectKind,
    pub magnitude: ScalableAmount,
    pub seconds: f32,
    /// See [`EffectSpec::secondary_magnitude`] — stays flat (it's a
    /// multiplier, not a scaling amount).
    #[serde(default)]
    pub secondary_magnitude: Option<f32>,
}

impl ScalableEffectSpec {
    /// Evaluate the magnitude for this caster and produce the runtime spec.
    pub fn resolve(&self, attrs: &AttributeSet, caster_level: u32) -> EffectSpec {
        EffectSpec {
            kind: self.kind,
            magnitude: self.magnitude.resolve(attrs, caster_level),
            seconds: self.seconds,
            secondary_magnitude: self.secondary_magnitude,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpawnObjectSpec {
    pub type_id: String,
    pub lifetime_seconds: f32,
    /// How many tiles to spawn and where, relative to the cast target tile.
    #[serde(default)]
    pub pattern: SpawnPattern,
    /// When true, every spawned object inherits a `HazardOwner(caster_id)`
    /// component so damage and DoTs it produces credit the caster via
    /// `DamageSource::OwnedByPlayer`.
    #[serde(default)]
    pub attribute_to_caster: bool,
}

/// A timed friendly creature summoned by a spell. The creature is realized as a
/// full hostile NPC (so it acquires and fights enemies through the normal AI),
/// then tagged `Faction::PlayerSide` + `Companion` so it fights *for* the caster
/// and its kills are credited to them. It despawns when its `lifetime_seconds`
/// `Ttl` expires (or when the caster dies). One companion per owner: re-casting
/// replaces the previous summon.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SummonSpec {
    /// Overworld-object definition id of the creature to summon.
    pub type_id: String,
    /// Seconds the summon lives before its `Ttl` despawns it.
    pub lifetime_seconds: f32,
    /// How many to summon at the target tile. Defaults to 1.
    #[serde(default = "default_summon_count")]
    pub count: u32,
    /// When no enemy is visible, the companion follows its owner until within
    /// this many tiles. Defaults to 2.
    #[serde(default = "default_follow_close_tiles")]
    pub follow_close_tiles: i32,
}

fn default_summon_count() -> u32 {
    1
}

fn default_follow_close_tiles() -> i32 {
    2
}

/// Tile pattern for `SpawnObjectSpec`. `Single` is the default and matches
/// pre-existing behavior (one entity at the target tile).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpawnPattern {
    #[default]
    Single,
    /// Three tiles in a straight line perpendicular to the caster→target
    /// axis, centered on the target tile.
    #[serde(rename = "perpendicular_line_3")]
    PerpendicularLine3,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AoeSpec {
    /// Chebyshev radius around the target tile. `0` hits only the target tile.
    pub radius_tiles: i32,
    /// VFX definition id (under `assets/vfx/`) played once on **every** tile
    /// in the AoE — not just on entities hit. Use for explosion-style spells
    /// where the floor itself should flash. `None` skips the per-tile VFX
    /// (only hit entities get `vfx_on_target_hit`).
    #[serde(default)]
    pub vfx_on_tile: Option<String>,
    /// How the AoE footprint resolves in space and time. Defaults to `Instant`
    /// (the whole radius at once, matching the original behavior). `Spread`
    /// blooms ring-by-ring from the center; `Spiral` hits one tile at a time
    /// in an outward spiral. See `combat::scheduled::tick_scheduled_impacts`.
    #[serde(default)]
    pub pattern: AoePattern,
}

/// Spatial/temporal shape of an AoE blast. Internal tagging (`kind:`) matches
/// the project convention for data-carrying YAML enums. Damage on each tile is
/// scheduled at its delay and resolved planar at the target floor.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AoePattern {
    /// Whole radius resolves simultaneously (delay 0 for every tile).
    #[default]
    Instant,
    /// Rings bloom outward: the tile on Chebyshev ring `r` fires at
    /// `r * ring_delay_seconds` (ring 0 = center at delay 0).
    Spread { ring_delay_seconds: f32 },
    /// One tile at a time along an outward square spiral: the `i`-th
    /// spiral tile fires at `i * step_delay_seconds`. `clockwise` flips the
    /// spiral handedness.
    Spiral {
        step_delay_seconds: f32,
        #[serde(default)]
        clockwise: bool,
    },
}

/// Flying-missile parameters for a spell. The cast itself is immediate (mana,
/// cast VFX, narrator) but the damage/AoE is deferred until the missile lands.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct ProjectileSpec {
    /// Overworld-object definition id whose sprite renders the flying missile
    /// (resolved via `OverworldObjectDefinitions`, like `arrow`/`bolt`).
    pub sprite: String,
    /// Travel speed in tiles per second. Flight time = `distance / speed`,
    /// floored by a small minimum so adjacent casts still show a brief flight.
    #[serde(default = "default_projectile_speed")]
    pub speed_tiles_per_second: f32,
}

fn default_projectile_speed() -> f32 {
    10.0
}

/// Kinds of timed magical effects tracked by `MagicEffects`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Player's personal light halo grows. Magnitude = tile radius.
    Glimmer,
    /// Player moves faster. Magnitude = step interval multiplier (e.g. 0.7).
    Haste,
    /// Caster gains tracked AC bonus. Magnitude = AC bonus (combat math reads
    /// this once §7 lands — no-op today).
    Shield,
    /// Caster gains tracked to-hit bonus. Magnitude = to-hit bonus (no-op
    /// today, hooked for Phase B combat math).
    Bless,
    /// Target NPC's roaming step interval lengthens. Magnitude = multiplier
    /// (e.g. 2.0 doubles the interval = half speed).
    Slow,
    /// Target NPC is asleep — its AI tick is skipped. Magnitude unused
    /// (presence is what matters). Cleared on damage by `resolve_battle_turn`.
    Sleep,
    /// Target cannot move or cast spells. Magnitude unused. Unlike Sleep,
    /// damage does *not* clear Paralyze — it only expires on its timer.
    Paralyze,
    /// DOT (cold damage) plus slow movement. Magnitude = damage per tick
    /// (1s cadence); `secondary_magnitude` = NPC step interval multiplier
    /// (Some(2.0) doubles the interval). When omitted, the slow component is
    /// a no-op and Chill behaves as pure cold DOT.
    Chill,
    /// DOT (fire damage). Magnitude = damage per tick (1s cadence).
    Burning,
    /// DOT (poison damage). Magnitude = damage per tick (1s cadence).
    Poisoned,
    /// Player's movement commands are randomly rotated by ±45° to an adjacent
    /// direction. Magnitude = deviation probability in `[0, 1]` (e.g. 0.3 =
    /// 30% chance to fumble each step). NPCs ignore Drunk for now.
    Drunk,
}

impl EffectKind {
    /// Short, lowercase, player-facing name. Used in tooltips and the inspect
    /// chat line where `{:?}` Debug output ("Poisoned") reads worse than prose.
    pub fn display_name(self) -> &'static str {
        match self {
            EffectKind::Glimmer => "glimmer",
            EffectKind::Haste => "haste",
            EffectKind::Shield => "shield",
            EffectKind::Bless => "bless",
            EffectKind::Slow => "slow",
            EffectKind::Sleep => "sleep",
            EffectKind::Paralyze => "paralyze",
            EffectKind::Chill => "chill",
            EffectKind::Burning => "burning",
            EffectKind::Poisoned => "poison",
            EffectKind::Drunk => "drunk",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpellTargeting {
    /// Player picks an entity. Range is checked against the entity's tile.
    Targeted,
    /// Player picks a tile (entity optional). Used for AoE and patterned
    /// summons like firewall.
    TargetedTile,
    /// Player picks one of their own inventory/equipment items (the next slot
    /// click resolves to an `ItemSlotRef`). Used by item-enchant spells; the
    /// only effect honored is `effects.enchant_item`.
    TargetedItem,
    /// No picker — casts on the caster's tile / self.
    Untargeted,
}

#[derive(Resource, Default)]
pub struct SpellDefinitions {
    definitions: HashMap<String, SpellDefinition>,
}

impl SpellDefinitions {
    pub fn load_from_disk() -> Self {
        let mut definitions = HashMap::new();
        for asset in discover_yaml_assets("spells", "spell definition") {
            let definition = serde_yaml::from_str::<SpellDefinition>(&asset.contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse spell definition {}: {error}",
                        asset.path.display()
                    )
                });
            definitions.insert(asset.id, definition);
        }
        Self { definitions }
    }

    pub fn get(&self, id: &str) -> Option<&SpellDefinition> {
        self.definitions.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_shipped_spells_load_from_disk() {
        // Catches malformed `damage:` expressions (a string the DamageExpr
        // parser rejects fails deserialization at load). Every shipped damaging
        // spell must roll a positive number for a representative caster.
        let defs = SpellDefinitions::load_from_disk();
        let attrs = AttributeSet::new(10, 10, 10, 16, 10, 16); // WIL/FOC 16
        for id in [
            "magic_dart",
            "spark_bolt",
            "fireball",
            "fireball_minor",
            "frost_bolt",
            "frost_lance",
            "immolation",
            "chain_spark",
        ] {
            let spell = defs
                .get(id)
                .unwrap_or_else(|| panic!("missing spell definition: {id}"));
            assert!(
                spell.effects.damage.deals_damage(),
                "spell '{id}' should deal damage"
            );
            assert!(
                spell.effects.damage.roll(&attrs, 5) > 0.0,
                "spell '{id}' rolled non-positive damage"
            );
        }
    }

    #[test]
    fn spell_damage_round_trips_through_serde() {
        // The derived (struct) Serialize must round-trip through the custom
        // Deserialize for both a present expression and `None`.
        let expr = SpellDamage(Some(DamageExpr::parse("2d6+focus/2+level/2").unwrap()));
        let back: SpellDamage =
            serde_yaml::from_str(&serde_yaml::to_string(&expr).unwrap()).unwrap();
        assert_eq!(expr, back);

        let none = SpellDamage(None);
        let back: SpellDamage =
            serde_yaml::from_str(&serde_yaml::to_string(&none).unwrap()).unwrap();
        assert_eq!(none, back);
    }

    #[test]
    fn damage_expression_string_deserializes_and_scales() {
        let yaml = r#"
name: Test Bolt
incantation: Test
mana_cost: 1.0
targeting: targeted
effects:
  damage: "2d6+focus/2+level/2"
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        let attrs = AttributeSet::new(10, 10, 10, 10, 10, 16);
        // Compare like-for-like (min vs min) so dice variance can't muddy it.
        // FOC 16 -> +8; level/2 adds +1 at L2 and +6 at L12.
        let expr = spell.effects.damage.0.as_ref().expect("damage expression");
        assert_eq!(expr.min_damage(&attrs, 2), 2 + 8 + 1);
        assert_eq!(expr.min_damage(&attrs, 12), 2 + 8 + 6);
        assert!(expr.min_damage(&attrs, 12) > expr.min_damage(&attrs, 2));
    }

    #[test]
    fn existing_minimal_spell_parses() {
        let yaml = r#"
name: Spark Bolt
incantation: Exori Vis
mana_cost: 12.0
targeting: targeted
range_tiles: 5
effects:
  damage: 18.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.name, "Spark Bolt");
        assert_eq!(spell.range_tiles, 5);
        // A bare number deserializes to a flat damage expression.
        assert!(spell.effects.damage.deals_damage());
        assert_eq!(spell.effects.damage.roll(&AttributeSet::default(), 1), 18.0);
        assert!(spell.class_access.is_empty());
        assert_eq!(spell.min_caster_level, 0);
        assert!(spell.effects.buffs_self.is_empty());
    }

    #[test]
    fn full_schema_round_trip() {
        let yaml = r#"
name: Frost Lance
incantation: Frigus Hasta
mana_cost: 16.0
targeting: targeted
range_tiles: 6
class_access: [Wizard]
min_caster_level: 3
effects:
  damage: 7.0
  damage_type: frost
  buffs_target:
    - kind: slow
      magnitude: 2.0
      seconds: 3.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.class_access, vec![Class::Wizard]);
        assert_eq!(spell.min_caster_level, 3);
        assert_eq!(spell.effects.buffs_target.len(), 1);
        assert_eq!(spell.effects.buffs_target[0].kind, EffectKind::Slow);
        assert_eq!(
            spell.effects.buffs_target[0].magnitude,
            ScalableAmount::Flat(2.0)
        );
        // Flat magnitudes resolve verbatim regardless of caster stats.
        let resolved = spell.effects.buffs_target[0].resolve(&AttributeSet::default(), 20);
        assert_eq!(resolved.magnitude, 2.0);
        assert_eq!(resolved.seconds, 3.0);
        assert_eq!(spell.effects.damage_type, Some(DamageType::Frost));
        assert_eq!(spell.effects.effective_damage_type(), DamageType::Frost);
    }

    #[test]
    fn scalable_heal_and_dot_magnitudes() {
        // Heals and DoT magnitudes accept expressions keyed off the caster;
        // bare numbers (incl. fractional multipliers like haste 0.7) pass
        // through untouched.
        let yaml = r#"
name: Test Mend
incantation: Test
mana_cost: 8.0
targeting: untargeted
effects:
  restore_health: "2d8+wil_mod*2+level"
  buffs_self:
    - kind: haste
      magnitude: 0.7
      seconds: 6.0
  buffs_target:
    - kind: burning
      magnitude: "1+foc_mod/2+level/4"
      seconds: 6.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        // WIL 16 -> mod +3 -> *2 = +6; L5 -> +5; 2d8 in [2,16].
        let attrs = AttributeSet::new(10, 10, 10, 16, 10, 14);
        let healed = spell.effects.restore_health.resolve(&attrs, 5);
        assert!((13.0..=27.0).contains(&healed), "{healed}");
        // Fractional flat multiplier survives (the SpellDamage rounding trap).
        let haste = spell.effects.buffs_self[0].resolve(&attrs, 5);
        assert_eq!(haste.magnitude, 0.7);
        // FOC 14 -> mod +2 -> /2 = +1; L5/4 = +1; burning tick = 3.
        let burn = spell.effects.buffs_target[0].resolve(&attrs, 5);
        assert_eq!(burn.magnitude, 3.0);
        // Old flat YAML keeps loading.
        assert!(ScalableAmount::Flat(0.0).is_zero());
        assert!(!spell.effects.restore_health.is_zero());
    }

    #[test]
    fn effects_without_damage_type_default_to_arcane() {
        let yaml = r#"
name: Spark Bolt
incantation: Exori Vis
mana_cost: 12.0
targeting: targeted
range_tiles: 5
effects:
  damage: 18.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.effects.damage_type, None);
        assert_eq!(spell.effects.effective_damage_type(), DamageType::Arcane);
    }

    #[test]
    fn all_authored_spells_parse_from_disk() {
        // Sanity-check that every YAML file in `assets/spells/` parses with
        // the current schema. Catches typos in newly-authored spells.
        let defs = SpellDefinitions::load_from_disk();
        let ids: Vec<&str> = defs.ids().collect();
        assert!(
            ids.contains(&"spark_bolt") && ids.contains(&"lesser_heal"),
            "expected baseline spells; got {ids:?}"
        );
        for new_id in [
            "glimmer",
            "light",
            "magic_dart",
            "frost_lance",
            "sleep",
            "shield",
            "slow",
            "cure_wounds",
            "restore",
            "bless",
            "swiftness",
            "immolation",
            "frost_bolt",
            "venom",
            "paralysis",
            "befuddle",
            "fireball",
            "firewall",
        ] {
            assert!(
                ids.contains(&new_id),
                "missing newly-authored spell {new_id}; got {ids:?}"
            );
        }
    }

    #[test]
    fn untargeted_self_buff_with_spawn_object() {
        let yaml = r#"
name: Glimmer
incantation: Lux Minima
mana_cost: 2.0
targeting: untargeted
class_access: [Wizard, Cleric]
min_caster_level: 1
effects:
  buffs_self:
    - kind: glimmer
      magnitude: 4.0
      seconds: 600.0
  spawns_object:
    type_id: magic_light
    lifetime_seconds: 1800.0
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.effects.buffs_self.len(), 1);
        let obj = spell.effects.spawns_object.as_ref().unwrap();
        assert_eq!(obj.type_id, "magic_light");
        assert_eq!(obj.lifetime_seconds, 1800.0);
        assert_eq!(obj.pattern, SpawnPattern::Single);
        assert!(!obj.attribute_to_caster);
    }

    #[test]
    fn firewall_pattern_with_owner_attribution() {
        let yaml = r#"
name: Firewall
incantation: Adori Flam
mana_cost: 28.0
targeting: targeted_tile
range_tiles: 5
effects:
  spawns_object:
    type_id: blazing_fire
    lifetime_seconds: 10.0
    pattern: perpendicular_line_3
    attribute_to_caster: true
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spell.targeting, SpellTargeting::TargetedTile);
        let obj = spell.effects.spawns_object.as_ref().unwrap();
        assert_eq!(obj.pattern, SpawnPattern::PerpendicularLine3);
        assert!(obj.attribute_to_caster);
    }

    #[test]
    fn aoe_field_round_trip() {
        let yaml = r#"
name: Fireball
incantation: Exori Flam
mana_cost: 22.0
targeting: targeted_tile
range_tiles: 6
effects:
  damage: 14.0
  damage_type: fire
  aoe:
    radius_tiles: 1
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        let aoe = spell.effects.aoe.as_ref().unwrap();
        assert_eq!(aoe.radius_tiles, 1);
        assert_eq!(spell.effects.effective_damage_type(), DamageType::Fire);
        // Pattern defaults to Instant when omitted (back-compat with old YAML).
        assert_eq!(aoe.pattern, AoePattern::Instant);
    }

    #[test]
    fn aoe_pattern_variants_round_trip() {
        let spread = r#"
name: Flame Burst
incantation: Exori Flam
mana_cost: 20.0
targeting: targeted_tile
range_tiles: 5
effects:
  damage: 10.0
  damage_type: fire
  aoe:
    radius_tiles: 2
    vfx_on_tile: fire_hit
    pattern:
      kind: spread
      ring_delay_seconds: 0.1
"#;
        let spell: SpellDefinition = serde_yaml::from_str(spread).unwrap();
        let aoe = spell.effects.aoe.as_ref().unwrap();
        assert_eq!(
            aoe.pattern,
            AoePattern::Spread {
                ring_delay_seconds: 0.1
            }
        );

        let spiral = r#"
name: Chain Spark
incantation: Exori Vis Tera
mana_cost: 24.0
targeting: targeted_tile
range_tiles: 6
effects:
  damage: 6.0
  damage_type: lightning
  aoe:
    radius_tiles: 3
    vfx_on_tile: lightning_spark
    pattern:
      kind: spiral
      step_delay_seconds: 0.05
"#;
        let spell: SpellDefinition = serde_yaml::from_str(spiral).unwrap();
        let aoe = spell.effects.aoe.as_ref().unwrap();
        assert_eq!(
            aoe.pattern,
            AoePattern::Spiral {
                step_delay_seconds: 0.05,
                clockwise: false,
            }
        );
    }

    #[test]
    fn projectile_spec_round_trip() {
        let yaml = r#"
name: Fireball
incantation: Exori Flam
mana_cost: 22.0
targeting: targeted_tile
range_tiles: 6
effects:
  damage: 14.0
  damage_type: fire
  projectile:
    sprite: fireball_missile
    speed_tiles_per_second: 9.0
  aoe:
    radius_tiles: 2
"#;
        let spell: SpellDefinition = serde_yaml::from_str(yaml).unwrap();
        let projectile = spell.effects.projectile.as_ref().unwrap();
        assert_eq!(projectile.sprite, "fireball_missile");
        assert_eq!(projectile.speed_tiles_per_second, 9.0);

        // Speed defaults when omitted.
        let defaulted = r#"
name: Magic Dart
incantation: Adori Vis
mana_cost: 6.0
targeting: targeted
range_tiles: 5
effects:
  damage: 8.0
  projectile:
    sprite: arcane_mote
"#;
        let spell: SpellDefinition = serde_yaml::from_str(defaulted).unwrap();
        let projectile = spell.effects.projectile.as_ref().unwrap();
        assert_eq!(
            projectile.speed_tiles_per_second,
            default_projectile_speed()
        );
    }
}
