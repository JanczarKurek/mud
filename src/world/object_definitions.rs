use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;

use bevy::log::info;
use bevy::prelude::*;
use serde::de::{self, Visitor};
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde_yaml::{Mapping, Value};

use crate::assets::AssetResolver;
use crate::combat::damage_type::DamageType;
use crate::magic::resources::EffectKind;
use crate::world::direction::Direction;

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct OverworldObjectDefinition {
    pub name: String,
    pub description: DescriptionField,
    pub colliding: bool,
    pub movable: bool,
    #[serde(default)]
    pub rotatable: bool,
    pub storable: bool,
    #[serde(default)]
    pub equipment_slot: Option<EquipmentSlot>,
    #[serde(default)]
    pub fillable_properties: Vec<String>,
    #[serde(default)]
    pub stats: StatModifiers,
    #[serde(default)]
    pub use_effects: UseEffects,
    #[serde(default)]
    pub use_texts: Vec<String>,
    #[serde(default)]
    pub use_on_texts: Vec<String>,
    #[serde(default)]
    pub spell_id: Option<String>,
    /// When `Some(N)`, this item starts with N charges. Charges are tracked
    /// per-stack in `InventoryStack::properties["charges_remaining"]`. On use,
    /// one charge is consumed; the item is destroyed when charges reach 0.
    /// When `None`, behaviour is the legacy single-consume-on-use (scrolls).
    #[serde(default)]
    pub max_charges: Option<u32>,
    /// When `true`, this item is never consumed on use. Overrides `max_charges`.
    /// Used for permanent spellcasting items and gathering tools.
    #[serde(default)]
    pub infinite_uses: bool,
    /// Optional cursor sprite (path under `assets/`) shown while the player is
    /// targeting with this item via "Use On". `None` → auto-derived: gathering
    /// tools get `cursors/gather_cursor.png`, everything else gets the default
    /// `cursors/use_on_cursor.png`. Targeted spells take a different `CursorMode`
    /// entirely and ignore this field.
    #[serde(default)]
    pub use_on_cursor: Option<String>,
    #[serde(default)]
    pub container_capacity: Option<usize>,
    /// When this object is itself a container, can it hold other items that are
    /// themselves storable containers? Pouches set this to `false` to keep
    /// nesting depth at 2 (pouch can sit in a backpack/chest, but a pouch
    /// cannot contain another pouch). Default `true` so backpacks/chests
    /// accept everything as before.
    #[serde(default = "default_accepts_storable_containers")]
    pub accepts_storable_containers: bool,
    /// Carry weight in kilograms (per single instance, multiplied by stack
    /// `quantity` for stacked items). `0.0` = weightless; this is the default
    /// for objects that pre-date the weight system.
    #[serde(default)]
    pub weight: f32,
    pub render: RenderMetadata,
    #[serde(default)]
    pub sound_paths: Vec<String>,
    #[serde(default = "default_max_stack_size")]
    pub max_stack_size: u32,
    #[serde(default)]
    pub stack_sprites: Vec<StackSpriteTier>,
    #[serde(default, rename = "loot")]
    pub loot_table: Option<LootTableDef>,
    /// Base number of tiles from which this object can be identified on `Inspect`.
    /// When `None`, callers apply a sensible default (currently 3).
    #[serde(default)]
    pub inspect_range: Option<i32>,
    #[serde(default)]
    pub attack_profile: Option<AttackProfileDef>,
    #[serde(default)]
    pub base_range_tiles: Option<i32>,
    #[serde(default)]
    pub ammo_type: Option<String>,
    #[serde(default)]
    pub damage: Option<String>,
    #[serde(default)]
    pub armor: i32,
    #[serde(default)]
    pub block: i32,
    #[serde(default)]
    pub hp: Option<String>,
    /// Creature level (HD per `docs/content_bible.md` §6). Drives XP awarded
    /// on kill via `xp_grant_for_kill`. Optional so non-creature definitions
    /// (items, scenery, doors) can omit it; spawn paths default to 1.
    #[serde(default)]
    pub level: Option<u32>,
    /// Yarn node name a player reaches when talking to this object. Empty =
    /// not talkable.
    #[serde(default)]
    pub dialog_node: Option<String>,
    /// Per-state visual + collider overrides. When non-empty `initial_state`
    /// must name one of the keys; otherwise the object behaves identically to
    /// one with no states declared.
    #[serde(default)]
    pub states: HashMap<String, ObjectStateDef>,
    /// Default state for fresh instances. Persistence overrides per-instance
    /// via `properties["state"]`.
    #[serde(default)]
    pub initial_state: Option<String>,
    /// Verbs the player can invoke on this object via the context menu.
    /// Each verb declares an optional `from`-state filter, the resulting
    /// `to`-state, and any `side_effects` to run after the transition lands.
    #[serde(default)]
    pub interactions: Vec<ObjectInteractionDef>,
    /// Passive triggers that fire when an entity (player or NPC) steps onto a
    /// tile containing this object. Each trigger can apply status effects,
    /// deal damage, and/or transition the object's `ObjectState`. Run in
    /// declared order so `apply_damage` lands before `set_state` when a trap
    /// snaps. Empty = no trigger (default).
    #[serde(default)]
    pub on_stepped: Vec<StepTriggerDef>,
    /// Property keys whose values are authored object ids. Resolved to runtime
    /// u64s (as decimal strings) during map load — see
    /// `SpaceDefinition::resolve_objects`. Used for cross-object wiring such
    /// as a lever's `target` pointing at a door.
    #[serde(default)]
    pub wires_to: Vec<String>,
    /// Light this object emits in the base / unstated case. May be overridden
    /// or suppressed per-state via `ObjectStateDef::light` / `clear_light`.
    /// Resolved into a `LightSource` ECS component on the projected entity.
    #[serde(default)]
    pub light: Option<LightEmissionDef>,
    /// Marks this object as a shopkeeper (merchant NPC). At spawn time a
    /// matching `Stockpile` entity is created and linked via the
    /// `Shopkeeper` component on this NPC. See `crate::game::shop`.
    #[serde(default)]
    pub shopkeeper: Option<crate::game::shop::ShopkeeperDef>,
    /// When set, using this item (`UseItem`) teaches the recipe with this
    /// id and consumes the scroll. The recipe must exist in
    /// `assets/recipes/`. Designed to share the same use-flow that spell
    /// scrolls already follow.
    #[serde(default)]
    pub learns_recipe: Option<String>,
    /// Optional lock metadata. When present, declares the `lock_id` (matched
    /// by keys' `lock_id`) and DC values referenced by interactions whose
    /// `skill_gate.dc_source` is `FromLockPick` / `FromLockForce`. Only used
    /// on stateful objects that have a `locked` state.
    #[serde(default)]
    pub lock: Option<LockDef>,
    /// On *items*: the `lock_id` this item's key matches. Used by the
    /// `key_gate` verb path to find an inventory key for a locked target.
    #[serde(default)]
    pub lock_id: Option<u32>,
}

/// Per-state override of the rendering / collider knobs on
/// `OverworldObjectDefinition`. `None` fields fall back to the base
/// definition.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct ObjectStateDef {
    #[serde(default)]
    pub sprite_path: Option<String>,
    #[serde(default)]
    pub animation: Option<AnimationSheetDef>,
    /// Override the authoritative `colliding` flag for this state (e.g. a
    /// closed door collides, an open one does not). `None` = inherit base.
    #[serde(default)]
    pub colliding: Option<bool>,
    /// Override the base light for this state. `None` = inherit base.
    #[serde(default)]
    pub light: Option<LightEmissionDef>,
    /// When true, this state suppresses any base light (used for an
    /// `unlit` torch state when the base inherits a light from a parent).
    #[serde(default)]
    pub clear_light: bool,
}

/// Per-object light authoring. `intensity` is a 0..=1+ multiplier on `color`
/// — values above 1 are clamped during the apply pass. `radius` is in tiles.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct LightEmissionDef {
    pub color: [u8; 3],
    pub radius: f32,
    #[serde(default = "default_light_intensity")]
    pub intensity: f32,
}

fn default_light_intensity() -> f32 {
    1.0
}

/// One verb the player may invoke on a stateful object.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct ObjectInteractionDef {
    pub verb: String,
    /// Display label for the context menu. Defaults to `verb` capitalised
    /// when missing.
    #[serde(default)]
    pub label: Option<String>,
    /// Filter: only show this verb when the object is in one of these states.
    /// Empty = always available.
    #[serde(default)]
    pub from: Vec<String>,
    /// Resulting state after the transition lands.
    pub to: String,
    /// Side-effects to execute after the transition (e.g. flipping a wired
    /// door, opening a container panel).
    #[serde(default)]
    pub side_effects: Vec<InteractionSideEffect>,
    /// Optional skill-check gate (`progression.md` §5). When present, the
    /// verb runs a `skill_check` against `dc_source` and only transitions on
    /// success; failures emit a chat-line. The verb is hidden in the context
    /// menu when the actor's rank in `skill` is 0.
    #[serde(default)]
    pub skill_gate: Option<SkillGateDef>,
    /// Optional key-required gate. When present, the verb is hidden in the
    /// context menu when the actor's inventory doesn't contain a matching
    /// `lock_id` key; the handler also re-checks at apply time.
    #[serde(default)]
    pub key_gate: Option<KeyGateDef>,
    /// Optional tool-required gate. Looks for the named item type in the
    /// player's `EquipmentSlot::Weapon`. Used for gathering interactions
    /// where the harvest requires a specific tool (pickaxe, fishing rod,
    /// herb knife).
    #[serde(default)]
    pub tool_gate: Option<ToolGateDef>,
    /// Items to grant the acting player on a successful transition.
    /// Reuses the same `LootDropDef` shape as corpse loot — each entry rolls
    /// quantity + probability independently.
    #[serde(default)]
    pub grants_items: Vec<LootDropDef>,
    /// When set, after this transition fires a `RespawnTimer` is attached to
    /// the object that will revert it to the first entry in `from` after the
    /// given delay. Drives gatherable respawn (mined ore returns after N
    /// seconds, etc.).
    #[serde(default)]
    pub respawn_seconds: Option<f32>,
}

/// Required-tool gate authored on an interaction. The player must have an
/// item of the matching type equipped in `EquipmentSlot::Weapon` for the
/// verb to fire.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct ToolGateDef {
    /// Definition id of the required equipped item (e.g. `"pickaxe"`).
    pub required_type_id: String,
    /// Narrator line shown on rejection. Defaults to a generic prompt.
    #[serde(default)]
    pub fail_message: Option<String>,
}

/// Skill-check description authored on an interaction.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SkillGateDef {
    pub skill: crate::player::skills::Skill,
    pub dc: DcSource,
}

/// Where the DC for a `SkillGateDef` comes from.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum DcSource {
    /// Read `lock.pick_dc` on the object's definition.
    FromLockPick,
    /// Read `lock.force_dc` on the object's definition.
    FromLockForce,
    /// Use the inline DC value.
    Fixed(i32),
}

/// Key-required description authored on an interaction.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct KeyGateDef {
    pub source: KeyIdSource,
}

/// Where the required `lock_id` for a `KeyGateDef` comes from.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum KeyIdSource {
    /// Read `lock.lock_id` on the object's definition.
    FromLock,
    /// Use the inline id.
    Fixed(u32),
}

/// Authored lock metadata on an `OverworldObjectDefinition`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct LockDef {
    pub lock_id: u32,
    pub pick_dc: i32,
    pub force_dc: i32,
}

/// One passive `on_stepped` trigger authored on an object. Fires when an
/// entity moves onto a tile containing this object and the current
/// `ObjectState` matches the `from` filter (or the filter is empty).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct StepTriggerDef {
    /// Allowed source states. Empty = fire regardless of state (or for
    /// stateless objects).
    #[serde(default)]
    pub from: Vec<String>,
    /// If set, in addition to firing on entry, this trigger also fires every
    /// `tick_seconds` while an entity remains colocated with the object and
    /// the `from` filter still matches. `None` = legacy one-shot-on-entry.
    #[serde(default)]
    pub tick_seconds: Option<f32>,
    /// Ordered list of effects to apply. Damage runs before any state
    /// transition that the same trigger also requests.
    pub effects: Vec<StepEffectDef>,
}

/// One effect within an `on_stepped` trigger.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum StepEffectDef {
    /// Apply a timed `MagicEffects` entry (Burning, Chill, Slow, …) to the
    /// stepper. Same parameter shape as a spell's `EffectSpec`. The trap is
    /// the caster (`caster = None` → no XP attribution if the DoT delivers
    /// the killing blow).
    ApplyEffect {
        effect: EffectKind,
        magnitude: f32,
        seconds: f32,
        #[serde(default)]
        secondary_magnitude: Option<f32>,
    },
    /// Deal a `DamageExpr` roll to the stepper as `DamageSource::Environment`.
    ApplyDamage { amount: String },
    /// Transition this object's `ObjectState` to `state`. The definition
    /// must declare matching `states:` + (usually) a re-arm `interactions:`
    /// verb to recover.
    SetState { state: String },
}

/// Side-effect to run after an interaction transitions an object's state.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum InteractionSideEffect {
    /// Resolve `target` (a property template like `"{properties.target}"`)
    /// against the source object's properties, then transition the resolved
    /// object into `to`. The resolved string must parse as a runtime u64
    /// (the map-load pass rewrites authored ids in `wires_to` properties).
    SetTargetState { target: String, to: String },
    /// Emit `GameUiEvent::OpenContainer` for the acting player. Pairs with
    /// `container_capacity` to make a stateful chest both "openable" and
    /// "viewable".
    OpenContainerPanel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AttackProfileKindDef {
    Melee,
    Ranged,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AttackProfileDef {
    pub kind: AttackProfileKindDef,
    /// Override for the VFX played on the target when a hit lands. Looked up
    /// in `VfxDefinitions`; falls back to `"blood_splash"` when omitted.
    #[serde(default)]
    pub hit_vfx: Option<String>,
    /// What kind of damage this weapon deals. When omitted, defaults to
    /// `Blunt` for melee and `Pierce` for ranged (resolved in
    /// `attack_profile_for_definition`).
    #[serde(default)]
    pub damage_type: Option<DamageType>,
    /// Magic effects rolled probabilistically every time this attack lands.
    /// Each entry is rolled independently. Combat reads these straight off the
    /// definition (see `resolve_battle_turn`) rather than mirroring them onto
    /// the `AttackProfile` runtime component, since the latter is `Copy`.
    #[serde(default)]
    pub on_hit_effects: Vec<OnHitEffectDef>,
}

/// A timed `MagicEffects` entry that an attacker rolls for on every landed
/// hit. `chance` is in `[0, 1]`; a `1.0` chance always applies.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct OnHitEffectDef {
    pub kind: EffectKind,
    pub magnitude: f32,
    pub seconds: f32,
    #[serde(default = "default_on_hit_chance")]
    pub chance: f32,
    #[serde(default)]
    pub secondary_magnitude: Option<f32>,
}

fn default_on_hit_chance() -> f32 {
    1.0
}

/// Quantity roll for a loot drop: either a fixed count or a uniform random range.
#[derive(Clone, Debug, Serialize)]
pub enum QuantityDistribution {
    Fixed(u32),
    /// Inclusive [min, max].
    Uniform(u32, u32),
}

impl QuantityDistribution {
    pub fn roll(&self) -> u32 {
        match self {
            QuantityDistribution::Fixed(n) => *n,
            QuantityDistribution::Uniform(min, max) => {
                if min >= max {
                    return *min;
                }
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as u64)
                    .unwrap_or(0);
                let range = (max - min + 1) as u64;
                *min + (nanos % range) as u32
            }
        }
    }
}

impl<'de> Deserialize<'de> for QuantityDistribution {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct QuantityVisitor;

        impl<'de> Visitor<'de> for QuantityVisitor {
            type Value = QuantityDistribution;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an integer or a string like \"uniform(5, 10)\"")
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(QuantityDistribution::Fixed(v as u32))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(QuantityDistribution::Fixed(v.max(0) as u32))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                let s = v.trim();
                if let Some(inner) = s.strip_prefix("uniform(").and_then(|s| s.strip_suffix(')')) {
                    let parts: Vec<&str> = inner.split(',').collect();
                    if parts.len() == 2 {
                        let min = parts[0].trim().parse::<u32>().map_err(de::Error::custom)?;
                        let max = parts[1].trim().parse::<u32>().map_err(de::Error::custom)?;
                        return Ok(QuantityDistribution::Uniform(min, max));
                    }
                }
                Err(de::Error::custom(format!(
                    "unrecognized quantity distribution: '{v}'"
                )))
            }
        }

        deserializer.deserialize_any(QuantityVisitor)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LootDropDef {
    pub type_id: String,
    #[serde(default = "default_quantity")]
    pub quantity: QuantityDistribution,
    #[serde(default = "default_probability")]
    pub probability: f32,
}

fn default_quantity() -> QuantityDistribution {
    QuantityDistribution::Fixed(1)
}

fn default_probability() -> f32 {
    1.0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LootTableDef {
    #[serde(default = "default_corpse_type_id")]
    pub corpse_type_id: String,
    #[serde(default = "default_corpse_despawn_seconds")]
    pub corpse_despawn_seconds: f32,
    #[serde(default)]
    pub drops: Vec<LootDropDef>,
}

fn default_corpse_type_id() -> String {
    "generic_corpse".to_owned()
}

fn default_corpse_despawn_seconds() -> f32 {
    60.0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct StackSpriteTier {
    pub min_count: u32,
    pub sprite_path: String,
}

fn default_max_stack_size() -> u32 {
    1
}

fn default_accepts_storable_containers() -> bool {
    true
}

/// A description field that accepts either a plain string or a list of conditional entries.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum DescriptionField {
    Plain(String),
    Entries(Vec<DescriptionEntry>),
}

impl Default for DescriptionField {
    fn default() -> Self {
        Self::Plain(String::new())
    }
}

/// One element of a description list. Either an unconditional string or a stack-size-gated text.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub enum DescriptionEntry {
    Text(String),
    Conditional {
        text: String,
        /// `[min, max]` — either bound may be `null` for open-ended.
        stack_size: (Option<u32>, Option<u32>),
    },
}

pub fn number_to_written(n: u32) -> String {
    const ONES: &[&str] = &[
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: &[&str] = &[
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    if n < 20 {
        return ONES[n as usize].to_owned();
    }
    if n < 100 {
        let tens = TENS[(n / 10) as usize];
        let unit = n % 10;
        return if unit == 0 {
            tens.to_owned()
        } else {
            format!("{}-{}", tens, ONES[unit as usize])
        };
    }
    if n < 1000 {
        let hundreds = n / 100;
        let rest = n % 100;
        return if rest == 0 {
            format!("{} hundred", ONES[hundreds as usize])
        } else {
            format!(
                "{} hundred and {}",
                ONES[hundreds as usize],
                number_to_written(rest)
            )
        };
    }
    n.to_string()
}

pub fn number_to_customary(n: u32) -> Option<&'static str> {
    match n {
        1 => Some("a singleton"),
        2 => Some("a pair"),
        3 => Some("a trio"),
        12 => Some("a dozen"),
        13 => Some("a baker's dozen"),
        20 => Some("a score"),
        144 => Some("a gross"),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct StatModifiers {
    #[serde(default)]
    pub strength: i32,
    #[serde(default)]
    pub agility: i32,
    #[serde(default)]
    pub constitution: i32,
    #[serde(default)]
    pub willpower: i32,
    #[serde(default)]
    pub charisma: i32,
    #[serde(default)]
    pub focus: i32,
    #[serde(default)]
    pub max_health: i32,
    #[serde(default)]
    pub max_mana: i32,
    #[serde(default)]
    pub storage_slots: i32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct UseEffects {
    #[serde(default)]
    pub restore_health: f32,
    #[serde(default)]
    pub restore_mana: f32,
    /// Multiplier applied to the player's HP/MP regen rate while the buff is
    /// active. `1.0` (default) means no buff. Values below 1.0 are silently
    /// clamped to 1.0 by the consume handler — debuffs aren't a thing yet.
    #[serde(default = "default_regen_multiplier")]
    pub regen_multiplier: f32,
    /// How long the regen buff lasts after consumption, in seconds. Stacking
    /// rule: re-eating extends the remaining time; the multiplier snaps to
    /// `max(current, new)` so a stronger buff isn't diluted by a weaker one.
    #[serde(default)]
    pub regen_duration_seconds: f32,
}

fn default_regen_multiplier() -> f32 {
    1.0
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Hash, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EquipmentSlot {
    Amulet,
    Helmet,
    Weapon,
    Armor,
    Shield,
    Legs,
    Backpack,
    Ring,
    Boots,
    Ammo,
}

impl EquipmentSlot {
    pub const ALL: [Self; 10] = [
        Self::Amulet,
        Self::Helmet,
        Self::Weapon,
        Self::Armor,
        Self::Shield,
        Self::Legs,
        Self::Backpack,
        Self::Ring,
        Self::Boots,
        Self::Ammo,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Amulet => "Amulet",
            Self::Helmet => "Helmet",
            Self::Weapon => "Weapon",
            Self::Armor => "Armor",
            Self::Shield => "Shield",
            Self::Legs => "Legs",
            Self::Backpack => "Backpack",
            Self::Ring => "Ring",
            Self::Boots => "Boots",
            Self::Ammo => "Ammo",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AnimationClipDef {
    pub row: u32,
    pub start_col: u32,
    pub frame_count: u32,
    pub fps: f32,
    #[serde(default = "default_true")]
    pub looping: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AnimationSheetDef {
    pub sheet_path: String,
    pub frame_width: u32,
    pub frame_height: u32,
    pub sheet_columns: u32,
    pub sheet_rows: u32,
    pub clips: HashMap<String, AnimationClipDef>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct RenderMetadata {
    pub z_index: f32,
    pub debug_color: [u8; 3],
    pub debug_size: f32,
    #[serde(default)]
    pub sprite_path: Option<String>,
    #[serde(default)]
    pub sprite_width_tiles: f32,
    #[serde(default)]
    pub sprite_height_tiles: f32,
    #[serde(default)]
    pub y_sort: bool,
    #[serde(default)]
    pub animation: Option<AnimationSheetDef>,
    /// Initial facing for this object type when no per-instance facing is
    /// supplied in the map YAML. Missing = `Direction::default()` (south).
    #[serde(default)]
    pub default_facing: Option<Direction>,
    /// When true, the sprite is rotated via `Transform::rotation_z` to match
    /// the object's `Facing` component. Use this for single-sprite props
    /// (signposts, arrows) that have no per-direction animation frames.
    /// Rotated sprites use center anchoring — the bottom-center y-sort shift
    /// is skipped so they sit square on the tile after rotation.
    #[serde(default)]
    pub rotation_by_facing: bool,
    /// Tiles on floor N with this flag hide everything on floor N+1 and
    /// above from view when the player stands directly beneath them.
    /// Walls and floor planks opt in so buildings feel enclosed.
    #[serde(default)]
    pub occludes_floor_above: bool,
    /// Upper floors are empty space by default; a tile on z > 0 is only
    /// walkable if some object at that tile has this flag set. Floor planks
    /// and stair tiles opt in. The ground floor is always walkable.
    #[serde(default)]
    pub walkable_surface: bool,
    /// Visual height of this object in tiles (wall=1.0, chest~0.4, barrel~0.5,
    /// low_rock~0.3, ground item=0.0). Drives vertical stacking when multiple
    /// objects share a tile, and gates auto-climb together with
    /// `walkable_surface`. Pure visual — collision still governed by
    /// `colliding`/state colliding flags.
    #[serde(default)]
    pub display_height: f32,
    /// Which building-wall side this object represents, for the
    /// hide-when-inside rule. Only `South` and `East` are honoured (the
    /// camera-facing sides). `None` = not a wall.
    #[serde(default)]
    pub hide_when_inside_facing: Option<Direction>,
    /// Tiebreaker for stack ordering when several `display_height > 0` objects
    /// share `(space, x, y, z)`. Suggested values: barrel=10, chest=20. When
    /// equal, the authoritative `object_id` breaks the tie.
    #[serde(default)]
    pub stack_order: i32,
}

impl RenderMetadata {
    pub fn has_oversized_sprite(&self) -> bool {
        self.sprite_width_tiles > 0.0 && self.sprite_height_tiles > 0.0
    }

    pub fn sprite_pixel_size(&self, tile_size: f32) -> Vec2 {
        if self.has_oversized_sprite() {
            Vec2::new(
                self.sprite_width_tiles * tile_size,
                self.sprite_height_tiles * tile_size,
            )
        } else {
            Vec2::splat(tile_size * self.debug_size)
        }
    }
}

impl OverworldObjectDefinition {
    /// Returns the raw description template text appropriate for `count` items.
    /// The caller must still interpolate `{count}`, `{count_written}`, `{count_customary}`.
    pub fn description_for_count(&self, count: u32) -> &str {
        match &self.description {
            DescriptionField::Plain(s) => s,
            DescriptionField::Entries(entries) => {
                for entry in entries {
                    match entry {
                        DescriptionEntry::Text(s) => return s,
                        DescriptionEntry::Conditional {
                            text,
                            stack_size: (min, max),
                        } => {
                            let min_ok = min.map_or(true, |m| count >= m);
                            let max_ok = max.map_or(true, |m| count <= m);
                            if min_ok && max_ok {
                                return text;
                            }
                        }
                    }
                }
                ""
            }
        }
    }

    pub fn sprite_for_count(&self, count: u32) -> Option<&str> {
        self.stack_sprites
            .iter()
            .rev()
            .find(|tier| count >= tier.min_count)
            .map(|tier| tier.sprite_path.as_str())
            .or(self.render.sprite_path.as_deref())
    }

    pub fn debug_color(&self) -> Color {
        Color::srgb_u8(
            self.render.debug_color[0],
            self.render.debug_color[1],
            self.render.debug_color[2],
        )
    }

    pub fn is_usable(&self) -> bool {
        self.use_effects.restore_health > 0.0
            || self.use_effects.restore_mana > 0.0
            || self.use_effects.regen_duration_seconds > 0.0
            || self.spell_id.is_some()
            || self.learns_recipe.is_some()
            || self.max_charges.is_some()
            || self.infinite_uses
    }

    /// True when the item has a notion of remaining uses tied to its definition —
    /// either a finite charge budget or unlimited uses. Distinct from `is_usable`
    /// because a scroll with `spell_id` but no `max_charges` is usable but has
    /// no charge accounting.
    pub fn has_charges(&self) -> bool {
        self.infinite_uses || self.max_charges.is_some()
    }

    /// Sprite path for `state`, falling back to the base `render.sprite_path`
    /// when the state has no override or no entry exists. `None` ⇒ render as
    /// a debug-color rectangle.
    pub fn sprite_path_for_state(&self, state: Option<&str>) -> Option<&str> {
        state
            .and_then(|s| self.states.get(s))
            .and_then(|state_def| state_def.sprite_path.as_deref())
            .or(self.render.sprite_path.as_deref())
    }

    /// Sprite path that combines a per-state override with the
    /// `stack_sprites` quantity tier. State overrides win because they
    /// represent semantic mode swaps (door open/closed, torch lit/unlit);
    /// otherwise the stack-tier sprite is used so a pile of coins on the
    /// ground visually grows with quantity.
    pub fn sprite_path_for_state_count(&self, state: Option<&str>, count: u32) -> Option<&str> {
        if let Some(s) = state {
            if let Some(state_def) = self.states.get(s) {
                if let Some(path) = state_def.sprite_path.as_deref() {
                    return Some(path);
                }
            }
        }
        self.sprite_for_count(count)
    }

    /// Animation sheet for `state`, falling back to the base
    /// `render.animation` when the state has no override.
    pub fn animation_for_state(&self, state: Option<&str>) -> Option<&AnimationSheetDef> {
        if let Some(state_def) = state.and_then(|s| self.states.get(s)) {
            if state_def.animation.is_some() {
                return state_def.animation.as_ref();
            }
        }
        self.render.animation.as_ref()
    }

    /// Light emission for `state`. `clear_light: true` on the state suppresses
    /// the base light; otherwise the state's `light` overrides the base, and
    /// missing-on-state inherits the base.
    pub fn light_for_state(&self, state: Option<&str>) -> Option<&LightEmissionDef> {
        if let Some(state_def) = state.and_then(|s| self.states.get(s)) {
            if state_def.clear_light {
                return None;
            }
            if state_def.light.is_some() {
                return state_def.light.as_ref();
            }
        }
        self.light.as_ref()
    }

    /// Authoritative `colliding` for `state`, falling back to base.
    pub fn colliding_for_state(&self, state: Option<&str>) -> bool {
        state
            .and_then(|s| self.states.get(s))
            .and_then(|state_def| state_def.colliding)
            .unwrap_or(self.colliding)
    }

    /// Pick the matching interaction for `(verb, current_state)`. Returns the
    /// first declaration whose `verb` matches and whose `from` either is empty
    /// or contains the current state.
    pub fn interaction_for(
        &self,
        verb: &str,
        current_state: Option<&str>,
    ) -> Option<&ObjectInteractionDef> {
        self.interactions.iter().find(|i| {
            i.verb == verb
                && (i.from.is_empty()
                    || current_state.is_some_and(|cs| i.from.iter().any(|s| s == cs)))
        })
    }
}

#[derive(Resource, Default)]
pub struct OverworldObjectDefinitions {
    definitions: HashMap<String, OverworldObjectDefinition>,
    /// For each definition id, the ancestor chain followed via the YAML
    /// `extends:` keyword (parent first → grandparent → …). Built at load
    /// time so editors can ask "does this object extend `npc`?" without
    /// re-reading YAML.
    extends_chain: HashMap<String, Vec<String>>,
    /// Type ids that appear as `tool_gate.required_type_id` on any interaction
    /// anywhere in the catalogue. Precomputed at load time so the UI can pick
    /// a gather-flavoured cursor sprite for these items in O(1).
    tool_type_ids: std::collections::HashSet<String>,
}

impl OverworldObjectDefinitions {
    pub fn load_from_disk() -> Self {
        let resolver = AssetResolver::new();
        let scan_dirs = resolver.scan_dirs("overworld_objects");

        let base_values = load_base_values();
        let mut raw_definition_values = HashMap::new();

        for scan_dir in &scan_dirs {
            info!(
                "loading overworld object definitions from {}",
                scan_dir.display()
            );
            let Ok(object_entries) = fs::read_dir(scan_dir) else {
                continue;
            };

            for entry in object_entries {
                let entry = entry.expect("Failed to read overworld object directory entry");
                let path = entry.path();

                if !path.is_dir() {
                    continue;
                }

                let Some(directory_name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };

                let metadata_path = path.join("metadata.yaml");
                info!(
                    "loading overworld object metadata {}",
                    metadata_path.display()
                );
                raw_definition_values.insert(
                    directory_name.to_owned(),
                    load_yaml_value(&metadata_path, "overworld object metadata"),
                );
            }
        }

        let mut resolved_definition_values = HashMap::new();
        for definition_id in raw_definition_values.keys() {
            resolve_extends_chain(
                definition_id,
                &raw_definition_values,
                &base_values,
                &mut resolved_definition_values,
                &mut Vec::new(),
            );
        }

        // Build the ancestor chain for each top-level object definition by
        // following its raw `extends:` field through `base_values`. Templates
        // without an `extends` key produce an empty chain.
        let mut extends_chain: HashMap<String, Vec<String>> = HashMap::new();
        for definition_id in raw_definition_values.keys() {
            let mut chain = Vec::new();
            let mut current_value = raw_definition_values.get(definition_id);
            while let Some(value) = current_value {
                let Value::Mapping(map) = value else {
                    break;
                };
                let Some(Value::String(parent)) = map.get(Value::String("extends".to_owned()))
                else {
                    break;
                };
                if chain.iter().any(|p: &String| p == parent) {
                    break;
                }
                let parent = parent.clone();
                current_value = base_values.get(&parent);
                chain.push(parent);
            }
            extends_chain.insert(definition_id.clone(), chain);
        }

        let mut definitions = HashMap::new();
        for (definition_id, value) in resolved_definition_values {
            let definition = serde_yaml::from_value::<OverworldObjectDefinition>(value)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to deserialize resolved overworld object definition '{}': {error}",
                        definition_id
                    )
                });
            info!(
                "object '{}' render: z_index={}, y_sort={}, sprite={}x{}",
                definition_id,
                definition.render.z_index,
                definition.render.y_sort,
                definition.render.sprite_width_tiles,
                definition.render.sprite_height_tiles,
            );
            definitions.insert(definition_id, definition);
        }

        let tool_type_ids = compute_tool_type_ids(&definitions);

        Self {
            definitions,
            extends_chain,
            tool_type_ids,
        }
    }

    /// Returns true if the definition with `id` (or any of its ancestors via
    /// the YAML `extends:` chain) is the base named `ancestor`. Useful for
    /// editor affordances that only apply to NPC-like templates.
    pub fn extends(&self, id: &str, ancestor: &str) -> bool {
        self.extends_chain
            .get(id)
            .map(|chain| chain.iter().any(|a| a == ancestor))
            .unwrap_or(false)
    }

    pub fn get(&self, id: &str) -> Option<&OverworldObjectDefinition> {
        self.definitions.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }

    /// True when this type id appears as a `tool_gate.required_type_id` on any
    /// interaction. Drives the gather-flavoured cursor sprite when the player
    /// enters "Use On" targeting with this item.
    pub fn is_gathering_tool(&self, type_id: &str) -> bool {
        self.tool_type_ids.contains(type_id)
    }

    #[cfg(test)]
    pub fn new_for_test(definitions: HashMap<String, OverworldObjectDefinition>) -> Self {
        let tool_type_ids = compute_tool_type_ids(&definitions);
        Self {
            definitions,
            extends_chain: HashMap::new(),
            tool_type_ids,
        }
    }
}

fn compute_tool_type_ids(
    definitions: &HashMap<String, OverworldObjectDefinition>,
) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    for def in definitions.values() {
        for interaction in &def.interactions {
            if let Some(gate) = &interaction.tool_gate {
                ids.insert(gate.required_type_id.clone());
            }
        }
    }
    ids
}

fn load_base_values() -> HashMap<String, Value> {
    let mut base_values = HashMap::new();
    for asset in crate::assets::discover_yaml_assets("object_bases", "object base metadata") {
        let value = serde_yaml::from_str::<Value>(&asset.contents).unwrap_or_else(|error| {
            panic!(
                "Failed to parse object base metadata {}: {error}",
                asset.path.display()
            )
        });
        base_values.insert(asset.id, value);
    }
    base_values
}

fn load_yaml_value(path: &Path, kind: &str) -> Value {
    let yaml = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("Failed to read {kind} {}: {error}", path.display()));

    serde_yaml::from_str::<Value>(&yaml)
        .unwrap_or_else(|error| panic!("Failed to parse {kind} {}: {error}", path.display()))
}

fn resolve_extends_chain(
    id: &str,
    object_values: &HashMap<String, Value>,
    base_values: &HashMap<String, Value>,
    resolved_values: &mut HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Value {
    if let Some(value) = resolved_values.get(id) {
        return value.clone();
    }

    assert!(
        !stack.iter().any(|ancestor| ancestor == id),
        "Circular 'extends' chain detected while resolving '{}': {:?}",
        id,
        stack
    );

    let raw_value = object_values
        .get(id)
        .unwrap_or_else(|| panic!("Missing raw overworld object definition value for '{}'", id));

    stack.push(id.to_owned());
    let resolved_value = resolve_value_with_extends(
        id,
        raw_value,
        object_values,
        base_values,
        resolved_values,
        stack,
    );
    stack.pop();

    resolved_values.insert(id.to_owned(), resolved_value.clone());
    resolved_value
}

fn resolve_value_with_extends(
    current_id: &str,
    raw_value: &Value,
    object_values: &HashMap<String, Value>,
    base_values: &HashMap<String, Value>,
    resolved_values: &mut HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Value {
    let mut child_mapping = as_mapping_clone(raw_value, current_id);
    let extends = child_mapping
        .remove(Value::String("extends".to_owned()))
        .and_then(|value| value.as_str().map(str::to_owned));

    if let Some(parent_id) = extends {
        let parent_value = if object_values.contains_key(&parent_id) {
            resolve_extends_chain(
                &parent_id,
                object_values,
                base_values,
                resolved_values,
                stack,
            )
        } else if let Some(parent_base_value) = base_values.get(&parent_id) {
            assert!(
                !stack.iter().any(|ancestor| ancestor == &parent_id),
                "Circular 'extends' chain detected while resolving '{}': {:?}",
                current_id,
                stack
            );
            stack.push(parent_id.clone());
            let resolved = resolve_base_value_with_extends(
                &parent_id,
                parent_base_value,
                object_values,
                base_values,
                resolved_values,
                stack,
            );
            stack.pop();
            resolved
        } else {
            panic!(
                "Object '{}' extends missing parent definition/base '{}'",
                current_id, parent_id
            );
        };

        merge_yaml_values(parent_value, Value::Mapping(child_mapping))
    } else {
        Value::Mapping(child_mapping)
    }
}

fn resolve_base_value_with_extends(
    current_id: &str,
    raw_value: &Value,
    object_values: &HashMap<String, Value>,
    base_values: &HashMap<String, Value>,
    resolved_values: &mut HashMap<String, Value>,
    stack: &mut Vec<String>,
) -> Value {
    let mut child_mapping = as_mapping_clone(raw_value, current_id);
    let extends = child_mapping
        .remove(Value::String("extends".to_owned()))
        .and_then(|value| value.as_str().map(str::to_owned));

    if let Some(parent_id) = extends {
        assert!(
            !stack.iter().any(|ancestor| ancestor == &parent_id),
            "Circular 'extends' chain detected while resolving '{}': {:?}",
            current_id,
            stack
        );

        let parent_value = if let Some(parent_object_value) = object_values.get(&parent_id) {
            let _ = parent_object_value;
            resolve_extends_chain(
                &parent_id,
                object_values,
                base_values,
                resolved_values,
                stack,
            )
        } else if let Some(parent_base_value) = base_values.get(&parent_id) {
            stack.push(parent_id.clone());
            let resolved = resolve_base_value_with_extends(
                &parent_id,
                parent_base_value,
                object_values,
                base_values,
                resolved_values,
                stack,
            );
            stack.pop();
            resolved
        } else {
            panic!(
                "Base '{}' extends missing parent definition/base '{}'",
                current_id, parent_id
            );
        };

        merge_yaml_values(parent_value, Value::Mapping(child_mapping))
    } else {
        Value::Mapping(child_mapping)
    }
}

fn as_mapping_clone(value: &Value, id: &str) -> Mapping {
    value
        .as_mapping()
        .cloned()
        .unwrap_or_else(|| panic!("Resolved YAML for '{}' must be a mapping", id))
}

fn merge_yaml_values(parent: Value, child: Value) -> Value {
    match (parent, child) {
        (Value::Mapping(mut parent_map), Value::Mapping(child_map)) => {
            for (key, child_value) in child_map {
                if let Some(parent_value) = parent_map.remove(&key) {
                    parent_map.insert(key, merge_yaml_values(parent_value, child_value));
                } else {
                    parent_map.insert(key, child_value);
                }
            }
            Value::Mapping(parent_map)
        }
        (_, child) => child,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_def(yaml: &str) -> OverworldObjectDefinition {
        serde_yaml::from_str::<OverworldObjectDefinition>(yaml).expect("yaml parses")
    }

    #[test]
    fn states_and_interactions_round_trip() {
        let yaml = r#"
name: Wooden Door
description: A heavy wooden door.
colliding: true
movable: false
storable: false
render:
  z_index: 0.30
  debug_color: [110, 70, 40]
  debug_size: 0.95
states:
  closed:
    sprite_path: door_closed.png
    colliding: true
  open:
    sprite_path: door_open.png
    colliding: false
initial_state: closed
interactions:
  - verb: open
    label: Open
    from: [closed]
    to: open
  - verb: close
    label: Close
    from: [open]
    to: closed
"#;
        let def = parse_def(yaml);
        assert_eq!(def.initial_state.as_deref(), Some("closed"));
        assert!(def.states.contains_key("closed"));
        assert!(def.states.contains_key("open"));
        assert_eq!(def.states["open"].colliding, Some(false));
        assert_eq!(def.interactions.len(), 2);
        assert_eq!(def.interactions[0].verb, "open");
        assert_eq!(def.interactions[0].to, "open");
        assert!(def.interactions[0].side_effects.is_empty());
    }

    #[test]
    fn interaction_for_filters_by_from_state() {
        let yaml = r#"
name: Wooden Door
description: ""
colliding: true
movable: false
storable: false
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
states:
  closed: {}
  open: {}
initial_state: closed
interactions:
  - verb: open
    from: [closed]
    to: open
  - verb: close
    from: [open]
    to: closed
"#;
        let def = parse_def(yaml);
        assert!(def.interaction_for("open", Some("closed")).is_some());
        assert!(def.interaction_for("open", Some("open")).is_none());
        assert!(def.interaction_for("close", Some("open")).is_some());
        assert!(def.interaction_for("nope", Some("closed")).is_none());
    }

    #[test]
    fn side_effect_set_target_state_round_trips() {
        let yaml = r#"
name: Lever
description: ""
colliding: false
movable: false
storable: false
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
wires_to: [target]
states:
  "off": {}
  "on": {}
initial_state: "off"
interactions:
  - verb: pull
    from: ["off"]
    to: "on"
    side_effects:
      - kind: set_target_state
        target: "{properties.target}"
        to: open
"#;
        let def = parse_def(yaml);
        assert_eq!(def.wires_to, vec!["target".to_owned()]);
        let interaction = def.interaction_for("pull", Some("off")).unwrap();
        match &interaction.side_effects[0] {
            InteractionSideEffect::SetTargetState { target, to } => {
                assert_eq!(target, "{properties.target}");
                assert_eq!(to, "open");
            }
            other => panic!("unexpected side effect: {:?}", other),
        }
    }

    #[test]
    fn lock_and_skill_gates_parse_from_yaml() {
        let yaml = r#"
name: Locked Door
description: ""
colliding: true
movable: false
storable: false
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
states:
  locked: { colliding: true }
  closed: {}
  open: { colliding: false }
initial_state: closed
lock:
  lock_id: 7
  pick_dc: 15
  force_dc: 18
interactions:
  - verb: pick_lock
    from: [locked]
    to: closed
    skill_gate:
      skill: Thievery
      dc: from_lock_pick
  - verb: force_lock
    from: [locked]
    to: closed
    skill_gate:
      skill: Athletics
      dc: from_lock_force
  - verb: use_key
    from: [locked]
    to: closed
    key_gate:
      source: from_lock
  - verb: open
    from: [closed]
    to: open
"#;
        let def = parse_def(yaml);
        let lock = def.lock.expect("lock block parsed");
        assert_eq!(lock.lock_id, 7);
        assert_eq!(lock.pick_dc, 15);
        assert_eq!(lock.force_dc, 18);

        let pick = def
            .interaction_for("pick_lock", Some("locked"))
            .expect("pick_lock interaction parsed");
        let pick_gate = pick.skill_gate.as_ref().expect("skill_gate parsed");
        assert_eq!(pick_gate.skill, crate::player::skills::Skill::Thievery);
        assert!(matches!(pick_gate.dc, DcSource::FromLockPick));

        let use_key = def
            .interaction_for("use_key", Some("locked"))
            .expect("use_key interaction parsed");
        let key_gate = use_key.key_gate.as_ref().expect("key_gate parsed");
        assert!(matches!(key_gate.source, KeyIdSource::FromLock));

        // Plain open interaction still works without gates.
        let open = def
            .interaction_for("open", Some("closed"))
            .expect("open interaction parsed");
        assert!(open.skill_gate.is_none());
        assert!(open.key_gate.is_none());
    }

    #[test]
    fn gatherable_interaction_round_trips() {
        let yaml = r#"
name: Fishing Spot
description: ""
colliding: false
movable: false
storable: false
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
states:
  available: {}
  depleted: {}
initial_state: available
interactions:
  - verb: fish
    from: [available]
    to: depleted
    tool_gate:
      required_type_id: fishing_rod
      fail_message: "You need a fishing rod equipped to fish here."
    skill_gate:
      skill: Survival
      dc: !fixed 8
    grants_items:
      - type_id: raw_fish
        quantity: "uniform(1, 2)"
        probability: 1.0
    respawn_seconds: 180.0
"#;
        let def = parse_def(yaml);
        let fish = def
            .interaction_for("fish", Some("available"))
            .expect("fish interaction parsed");
        let tool = fish.tool_gate.as_ref().expect("tool_gate parsed");
        assert_eq!(tool.required_type_id, "fishing_rod");
        assert!(tool.fail_message.is_some());
        let gate = fish.skill_gate.as_ref().expect("skill_gate parsed");
        assert_eq!(gate.skill, crate::player::skills::Skill::Survival);
        assert!(matches!(gate.dc, DcSource::Fixed(8)));
        assert_eq!(fish.grants_items.len(), 1);
        assert_eq!(fish.grants_items[0].type_id, "raw_fish");
        assert_eq!(fish.respawn_seconds, Some(180.0));
    }

    #[test]
    fn on_stepped_triggers_round_trip() {
        let yaml = r#"
name: Bear Trap
description: ""
colliding: false
movable: false
storable: false
render:
  z_index: 0.2
  debug_color: [120, 120, 120]
  debug_size: 0.8
states:
  armed: {}
  sprung: {}
initial_state: armed
on_stepped:
  - from: [armed]
    effects:
      - kind: apply_damage
        amount: "2d6+4"
      - kind: apply_effect
        effect: chill
        magnitude: 1.0
        seconds: 4.0
        secondary_magnitude: 2.0
      - kind: set_state
        state: sprung
"#;
        let def = parse_def(yaml);
        assert_eq!(def.on_stepped.len(), 1);
        let trigger = &def.on_stepped[0];
        assert_eq!(trigger.from, vec!["armed".to_owned()]);
        assert_eq!(trigger.effects.len(), 3);
        match &trigger.effects[0] {
            StepEffectDef::ApplyDamage { amount } => assert_eq!(amount, "2d6+4"),
            other => panic!("unexpected first effect: {other:?}"),
        }
        match &trigger.effects[1] {
            StepEffectDef::ApplyEffect {
                effect,
                magnitude,
                seconds,
                secondary_magnitude,
            } => {
                assert_eq!(*effect, EffectKind::Chill);
                assert_eq!(*magnitude, 1.0);
                assert_eq!(*seconds, 4.0);
                assert_eq!(*secondary_magnitude, Some(2.0));
            }
            other => panic!("unexpected second effect: {other:?}"),
        }
        match &trigger.effects[2] {
            StepEffectDef::SetState { state } => assert_eq!(state, "sprung"),
            other => panic!("unexpected third effect: {other:?}"),
        }
    }

    #[test]
    fn on_stepped_defaults_to_empty() {
        let yaml = r#"
name: Plain Floor Item
description: ""
colliding: false
movable: false
storable: false
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
"#;
        let def = parse_def(yaml);
        assert!(def.on_stepped.is_empty());
    }

    #[test]
    fn colliding_for_state_falls_back_to_base() {
        let yaml = r#"
name: Door
description: ""
colliding: true
movable: false
storable: false
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
states:
  closed:
    colliding: true
  open:
    colliding: false
"#;
        let def = parse_def(yaml);
        assert!(def.colliding_for_state(Some("closed")));
        assert!(!def.colliding_for_state(Some("open")));
        // Unknown state name: fall back to the base flag.
        assert!(def.colliding_for_state(Some("ajar")));
        // No state argument: also base flag.
        assert!(def.colliding_for_state(None));
    }
}
