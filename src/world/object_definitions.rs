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
    #[serde(default)]
    pub container_capacity: Option<usize>,
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
    pub hp: Option<String>,
    /// When present, walking onto this object's tile shifts the player's floor
    /// by `delta` (±1 for stairs_up / stairs_down, etc.).
    #[serde(default)]
    pub floor_transition: Option<FloorTransitionDef>,
    /// Yarn node name a player reaches when talking to this object. Empty =
    /// not talkable.
    #[serde(default)]
    pub dialog_node: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct FloorTransitionDef {
    pub delta: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AttackProfileKindDef {
    Melee,
    Ranged,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AttackProfileDef {
    pub kind: AttackProfileKindDef,
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
            || self.spell_id.is_some()
    }
}

#[derive(Resource, Default)]
pub struct OverworldObjectDefinitions {
    definitions: HashMap<String, OverworldObjectDefinition>,
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

        Self { definitions }
    }

    pub fn get(&self, id: &str) -> Option<&OverworldObjectDefinition> {
        self.definitions.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }
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
