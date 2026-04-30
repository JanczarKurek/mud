use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::discover_yaml_assets;
use crate::world::components::TilePosition;
use crate::world::direction::Direction;
use crate::world::floor_definitions::FloorTypeId;
use crate::world::floor_map::FloorMap;
use crate::world::object_definitions::OverworldObjectDefinitions;

const DEFAULT_BOOTSTRAP_SPACE_ID: &str = "overworld";

pub type ObjectProperties = HashMap<String, String>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpacePermanence {
    Persistent,
    Ephemeral,
}

impl SpacePermanence {
    pub const fn is_persistent(self) -> bool {
        matches!(self, Self::Persistent)
    }
}

#[derive(Clone, Debug, Deserialize, Resource)]
pub struct SpaceDefinitions {
    pub bootstrap_space_id: String,
    pub spaces: HashMap<String, SpaceDefinition>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct SpaceDefinition {
    pub authored_id: String,
    pub width: i32,
    pub height: i32,
    pub fill_floor_type: FloorTypeId,
    #[serde(default = "default_persistent_permanence")]
    pub permanence: SpacePermanence,
    #[serde(default)]
    pub portals: Vec<PortalDefinition>,
    #[serde(default)]
    pub objects: Vec<MapObjectEntry>,
    /// Floor placements grouped by floor type id. Overlay on top of `fill_floor_type`.
    #[serde(default)]
    pub floors: HashMap<FloorTypeId, FloorPlacements>,
    /// Single-character keys mapping to object type IDs for use in `tiles`.
    #[serde(default)]
    pub legend: HashMap<String, String>,
    /// ASCII grid of tiles, row-major with y=0 at top. Each character maps
    /// via `legend`; unmapped characters are skipped (fill_floor_type applies).
    #[serde(default)]
    pub tiles: Option<String>,
    #[serde(skip)]
    pub resolved_objects: Vec<ResolvedObject>,
    #[serde(skip)]
    object_indices: HashMap<u64, usize>,
    /// Authored-id → runtime u64 lookup, populated by `resolve_objects`. Used
    /// by `resolve_wiring` to rewrite cross-object reference properties
    /// (e.g. a lever's `target` from "cellar_door" to a runtime id) once
    /// `OverworldObjectDefinitions` are available.
    #[serde(skip)]
    authored_id_lookup: HashMap<String, u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct FloorPlacements {
    #[serde(default)]
    pub placement: Vec<TileCoordinate>,
    #[serde(default)]
    pub rects: Vec<TileRectangleArea>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct TileRectangleArea {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    #[serde(default)]
    pub z: i32,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct PortalDefinition {
    pub id: String,
    pub source: TileCoordinate,
    pub destination_space_id: String,
    pub destination_tile: TileCoordinate,
    #[serde(default)]
    pub destination_permanence: Option<SpacePermanence>,
}

/// Authored object entry as it appears in YAML. Carries an *optional* symbolic
/// `id` (a string) used to refer back to it from another object's `contents`.
/// Numeric runtime IDs are assigned by `SpaceDefinition::resolve_objects` —
/// authored YAML never sees them.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct MapObjectInstance {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    #[serde(default)]
    pub placement: Option<TileCoordinate>,
    #[serde(default)]
    pub contents: Vec<MapObjectChild>,
    #[serde(default)]
    pub behavior: Option<MapBehavior>,
    #[serde(default)]
    pub facing: Option<Direction>,
}

/// A child of a container's `contents:` list. Either a symbolic reference to
/// another instance's `id` (must resolve at load time) or an inline nested
/// `MapObjectInstance` whose location is "inside the parent".
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum MapObjectChild {
    Reference(String),
    Inline(Box<MapObjectInstance>),
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum MapObjectEntry {
    Explicit(MapObjectInstance),
    Anonymous(AnonymousObjectPlacements),
}

/// Fully-resolved object instance: numeric runtime id, contents flattened to
/// a list of u64 ids. Built from `MapObjectInstance` /
/// `AnonymousObjectPlacements` by `SpaceDefinition::resolve_objects`.
#[derive(Clone, Debug)]
pub struct ResolvedObject {
    pub id: u64,
    pub type_id: String,
    pub properties: ObjectProperties,
    pub placement: Option<TileCoordinate>,
    pub contents: Vec<u64>,
    pub behavior: Option<MapBehavior>,
    pub facing: Option<Direction>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct AnonymousObjectPlacements {
    #[serde(rename = "type")]
    pub type_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    pub placement: Vec<TileCoordinate>,
    #[serde(default)]
    pub facing: Option<Direction>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapBehavior {
    Roam {
        step_interval_seconds: f32,
        bounds: TileRectangle,
    },
    RoamAndChase {
        step_interval_seconds: f32,
        bounds: TileRectangle,
        detect_distance_tiles: i32,
        disengage_distance_tiles: i32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct TileCoordinate {
    pub x: i32,
    pub y: i32,
    #[serde(default, skip_serializing_if = "is_default_z")]
    pub z: i32,
}

fn is_default_z(z: &i32) -> bool {
    *z == 0
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct TileRectangle {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl TileCoordinate {
    pub const fn to_tile_position(self) -> TilePosition {
        TilePosition::new(self.x, self.y, self.z)
    }
}

impl SpaceDefinitions {
    pub fn load_from_disk() -> Self {
        let mut spaces = HashMap::new();
        // Global runtime-id allocator. Each space's resolve_objects consumes a
        // contiguous range so ids never collide across spaces.
        let mut next_object_id: u64 = 1;

        for asset in discover_yaml_assets("maps", "map layout") {
            let mut definition: SpaceDefinition = serde_yaml::from_str(&asset.contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse map layout {}: {error}",
                        asset.path.display()
                    )
                });

            if definition.authored_id.trim().is_empty() {
                definition.authored_id = asset.id.clone();
            }

            next_object_id = definition.resolve_objects(next_object_id);
            spaces.insert(definition.authored_id.clone(), definition);
            info!("loaded map layout {}", asset.path.display());
        }

        if !spaces.is_empty() {
            assert!(
                spaces.contains_key(DEFAULT_BOOTSTRAP_SPACE_ID),
                "Missing bootstrap space definition '{}'",
                DEFAULT_BOOTSTRAP_SPACE_ID
            );
        }

        for definition in spaces.values() {
            for portal in &definition.portals {
                assert!(
                    spaces.contains_key(&portal.destination_space_id),
                    "Space '{}' portal '{}' points to missing destination '{}'",
                    definition.authored_id,
                    portal.id,
                    portal.destination_space_id
                );
            }
        }

        Self {
            bootstrap_space_id: DEFAULT_BOOTSTRAP_SPACE_ID.to_owned(),
            spaces,
        }
    }

    pub fn bootstrap_space(&self) -> Option<&SpaceDefinition> {
        self.get(&self.bootstrap_space_id)
    }

    pub fn get(&self, authored_id: &str) -> Option<&SpaceDefinition> {
        self.spaces.get(authored_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpaceDefinition> {
        self.spaces.values()
    }

    /// Insert or replace a space definition (e.g. after editor Save As or New Map).
    pub fn insert_or_replace(&mut self, def: SpaceDefinition) {
        self.spaces.insert(def.authored_id.clone(), def);
    }

    /// Run `resolve_wiring` on every space. Call once after `load_from_disk`
    /// AND `OverworldObjectDefinitions::load_from_disk` have both completed —
    /// wiring resolution depends on both being available.
    pub fn resolve_wiring(&mut self, object_definitions: &OverworldObjectDefinitions) {
        for definition in self.spaces.values_mut() {
            definition.resolve_wiring(object_definitions);
        }
    }

    /// Load a single map YAML from `assets/maps/{authored_id}.yaml` and insert it.
    /// Returns `true` if successful. Skips validation assertions for portal destinations.
    pub fn load_single_from_disk(&mut self, authored_id: &str) -> bool {
        let path = format!("assets/maps/{authored_id}.yaml");
        let Ok(yaml) = std::fs::read_to_string(&path) else {
            return false;
        };
        let Ok(mut def) = serde_yaml::from_str::<SpaceDefinition>(&yaml) else {
            return false;
        };
        if def.authored_id.trim().is_empty() {
            def.authored_id = authored_id.to_owned();
        }
        // Pick an id range that doesn't collide with other already-loaded spaces.
        let start_id = self
            .spaces
            .iter()
            .filter(|(other_id, _)| other_id.as_str() != def.authored_id.as_str())
            .flat_map(|(_, space)| space.resolved_objects.iter().map(|o| o.id))
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);
        def.resolve_objects(start_id);
        self.spaces.insert(def.authored_id.clone(), def);
        true
    }
}

impl SpaceDefinition {
    pub fn portal_at(&self, tile_position: TilePosition) -> Option<&PortalDefinition> {
        self.portals
            .iter()
            .find(|portal| portal.source.to_tile_position() == tile_position)
    }

    pub fn is_contained(&self, object_id: u64) -> bool {
        self.resolved_objects
            .iter()
            .any(|object| object.contents.contains(&object_id))
    }

    /// Look up a `ResolvedObject` by its runtime id within this space.
    pub fn find_resolved(&self, object_id: u64) -> Option<&ResolvedObject> {
        self.object_indices
            .get(&object_id)
            .and_then(|i| self.resolved_objects.get(*i))
    }

    /// Expand tile grid + authored objects into a flat `resolved_objects` list,
    /// allocating runtime u64 ids starting from `start_id` and resolving symbolic
    /// `contents:` references. Returns the next free id (caller threads this
    /// across spaces so ids stay globally unique).
    pub fn resolve_objects(&mut self, start_id: u64) -> u64 {
        self.expand_tile_grid();

        let mut next_id: u64 = start_id;
        let mut resolved: Vec<ResolvedObject> = Vec::new();
        let mut name_to_id: HashMap<String, u64> = HashMap::new();
        // Forward references can't be resolved until every instance has been
        // walked and ids are assigned. Stash them with the parent index + slot.
        let mut deferred_refs: Vec<(usize, usize, String)> = Vec::new();

        let space_id = self.authored_id.clone();
        for entry in self.objects.clone() {
            match entry {
                MapObjectEntry::Explicit(instance) => {
                    walk_instance(
                        &instance,
                        &space_id,
                        false,
                        &mut next_id,
                        &mut resolved,
                        &mut name_to_id,
                        &mut deferred_refs,
                    );
                }
                MapObjectEntry::Anonymous(group) => {
                    for tile in &group.placement {
                        let id = next_id;
                        next_id += 1;
                        resolved.push(ResolvedObject {
                            id,
                            type_id: group.type_id.clone(),
                            properties: group.properties.clone(),
                            placement: Some(*tile),
                            contents: Vec::new(),
                            behavior: None,
                            facing: group.facing,
                        });
                    }
                }
            }
        }

        // Resolve deferred references against the now-complete name table.
        for (parent_index, slot, name) in deferred_refs {
            let resolved_id = *name_to_id.get(&name).unwrap_or_else(|| {
                panic!(
                    "Object reference '{}' in space '{}' does not match any object id",
                    name, space_id
                );
            });
            resolved[parent_index].contents[slot] = resolved_id;
        }

        // Build (object_id -> index) map and run validation against the resolved
        // graph: in-bounds placements, no self-containment, no double-placement.
        let mut object_indices: HashMap<u64, usize> = HashMap::new();
        for (index, object) in resolved.iter().enumerate() {
            let previous = object_indices.insert(object.id, index);
            assert!(
                previous.is_none(),
                "Duplicate runtime object id {} in space '{}' (compiler bug?)",
                object.id,
                space_id,
            );
        }

        let mut location_counts: HashMap<u64, usize> = HashMap::new();
        for object in &resolved {
            if let Some(placement) = object.placement {
                assert!(
                    placement.x >= 0
                        && placement.y >= 0
                        && placement.x < self.width
                        && placement.y < self.height,
                    "Object '{}' placement {:?} is outside space '{}'",
                    object.type_id,
                    placement,
                    space_id,
                );
                *location_counts.entry(object.id).or_default() += 1;
            }
        }
        for object in &resolved {
            for contained_id in &object.contents {
                assert!(
                    *contained_id != object.id,
                    "Object {} cannot contain itself in '{}'",
                    object.id,
                    space_id,
                );
                assert!(
                    object_indices.contains_key(contained_id),
                    "Object {} references missing contained id {} in '{}'",
                    object.id,
                    contained_id,
                    space_id,
                );
                *location_counts.entry(*contained_id).or_default() += 1;
            }
        }
        for portal in &self.portals {
            assert!(
                portal.source.x >= 0
                    && portal.source.y >= 0
                    && portal.source.x < self.width
                    && portal.source.y < self.height,
                "Portal '{}' source is outside space '{}'",
                portal.id,
                space_id,
            );
        }
        for (object_id, count) in location_counts {
            assert!(
                count <= 1,
                "Object {} appears in more than one place in '{}'",
                object_id,
                space_id,
            );
        }

        self.resolved_objects = resolved;
        self.object_indices = object_indices;
        self.authored_id_lookup = name_to_id;
        next_id
    }

    /// Rewrite each resolved object's `properties` so that values for keys
    /// listed in the definition's `wires_to:` resolve from authored ids
    /// (the strings authors typed in the map YAML) to runtime u64s (decimal
    /// strings). Panics on dangling references — wiring must be authored
    /// correctly at load time, not silently drop at runtime.
    pub fn resolve_wiring(&mut self, object_definitions: &OverworldObjectDefinitions) {
        let space_id = self.authored_id.clone();
        for object in &mut self.resolved_objects {
            let Some(def) = object_definitions.get(&object.type_id) else {
                continue;
            };
            for key in &def.wires_to {
                let Some(authored_target) = object.properties.get(key) else {
                    continue;
                };
                let Some(&runtime_id) = self.authored_id_lookup.get(authored_target) else {
                    panic!(
                        "Object of type '{}' in space '{}' has property '{}: {}' but no \
                         authored object with that id exists in this space",
                        object.type_id, space_id, key, authored_target
                    );
                };
                object
                    .properties
                    .insert(key.clone(), runtime_id.to_string());
            }
        }
    }

    /// Create a blank space definition (no objects, no portals).
    pub fn new_empty(
        authored_id: String,
        width: i32,
        height: i32,
        fill_floor_type: FloorTypeId,
    ) -> Self {
        Self {
            authored_id,
            width,
            height,
            fill_floor_type,
            permanence: SpacePermanence::Persistent,
            portals: Vec::new(),
            objects: Vec::new(),
            floors: HashMap::new(),
            legend: HashMap::new(),
            tiles: None,
            resolved_objects: Vec::new(),
            object_indices: HashMap::new(),
            authored_id_lookup: HashMap::new(),
        }
    }

    /// Build a fully-baked `FloorMap` for the given z-floor. The map is
    /// initialised to `Some(fill_floor_type)` and overlaid with explicit floor
    /// placements at the matching z. OOB placements are silently dropped.
    pub fn build_floor_map(&self, z: i32) -> FloorMap {
        let fill = if z == TilePosition::GROUND_FLOOR && !self.fill_floor_type.is_empty() {
            Some(self.fill_floor_type.clone())
        } else {
            None
        };
        let mut map = FloorMap::new_filled(self.width, self.height, fill);
        for (floor_id, placements) in &self.floors {
            for tile in &placements.placement {
                if tile.z == z {
                    map.set(tile.x, tile.y, Some(floor_id.clone()));
                }
            }
            for rect in &placements.rects {
                if rect.z != z {
                    continue;
                }
                for dy in 0..rect.h {
                    for dx in 0..rect.w {
                        map.set(rect.x + dx, rect.y + dy, Some(floor_id.clone()));
                    }
                }
            }
        }
        map
    }

    fn expand_tile_grid(&mut self) {
        let Some(tiles_str) = self.tiles.clone() else {
            return;
        };

        let lines: Vec<&str> = tiles_str.lines().collect();
        assert!(
            lines.len() == self.height as usize,
            "Space '{}' tiles grid has {} rows but height is {}",
            self.authored_id,
            lines.len(),
            self.height
        );
        for key in self.legend.keys() {
            assert!(
                key.chars().count() == 1,
                "Space '{}' legend key '{}' must be exactly one character",
                self.authored_id,
                key
            );
        }

        let char_map: HashMap<char, &str> = self
            .legend
            .iter()
            .map(|(k, v)| (k.chars().next().unwrap(), v.as_str()))
            .collect();

        let mut type_to_tiles: HashMap<String, Vec<TileCoordinate>> = HashMap::new();
        for (row_idx, line) in lines.iter().enumerate() {
            assert!(
                line.chars().count() == self.width as usize,
                "Space '{}' tiles row {} has {} chars but width is {}",
                self.authored_id,
                row_idx,
                line.chars().count(),
                self.width
            );
            for (col_idx, ch) in line.chars().enumerate() {
                if let Some(&type_id) = char_map.get(&ch) {
                    type_to_tiles
                        .entry(type_id.to_owned())
                        .or_default()
                        .push(TileCoordinate {
                            x: col_idx as i32,
                            y: row_idx as i32,
                            z: TilePosition::GROUND_FLOOR,
                        });
                }
            }
        }

        for (type_id, placements) in type_to_tiles {
            self.objects
                .push(MapObjectEntry::Anonymous(AnonymousObjectPlacements {
                    type_id,
                    properties: HashMap::new(),
                    placement: placements,
                    facing: None,
                }));
        }
    }

}

/// Recursive depth-first walk of an authored `MapObjectInstance`. Allocates
/// runtime ids for the instance and its inline children, records symbolic
/// names in `name_to_id`, and stashes any `Reference(name)` slots into
/// `deferred_refs` for the second resolution pass to fill in.
fn walk_instance(
    instance: &MapObjectInstance,
    space_id: &str,
    is_inline_child: bool,
    next_id: &mut u64,
    resolved: &mut Vec<ResolvedObject>,
    name_to_id: &mut HashMap<String, u64>,
    deferred_refs: &mut Vec<(usize, usize, String)>,
) -> u64 {
    let id = *next_id;
    *next_id += 1;
    if let Some(name) = &instance.id {
        let name_owned = name.clone();
        let prev = name_to_id.insert(name_owned, id);
        assert!(
            prev.is_none(),
            "Duplicate object id '{}' in space '{}'",
            name,
            space_id
        );
    }
    if is_inline_child {
        assert!(
            instance.placement.is_none(),
            "Inline child object (type '{}') in space '{}' must not declare `placement` — its location is inferred from its parent container",
            instance.type_id,
            space_id,
        );
    }

    let parent_index = resolved.len();
    resolved.push(ResolvedObject {
        id,
        type_id: instance.type_id.clone(),
        properties: instance.properties.clone(),
        placement: instance.placement,
        contents: Vec::with_capacity(instance.contents.len()),
        behavior: instance.behavior.clone(),
        facing: instance.facing,
    });

    let mut child_ids: Vec<u64> = Vec::with_capacity(instance.contents.len());
    for child in &instance.contents {
        match child {
            MapObjectChild::Inline(inner) => {
                let inner_id = walk_instance(
                    inner,
                    space_id,
                    true,
                    next_id,
                    resolved,
                    name_to_id,
                    deferred_refs,
                );
                child_ids.push(inner_id);
            }
            MapObjectChild::Reference(name) => {
                deferred_refs.push((parent_index, child_ids.len(), name.clone()));
                child_ids.push(u64::MAX);
            }
        }
    }
    resolved[parent_index].contents = child_ids;
    id
}

const fn default_persistent_permanence() -> SpacePermanence {
    SpacePermanence::Persistent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_resolve(yaml: &str) -> SpaceDefinition {
        let mut def: SpaceDefinition = serde_yaml::from_str(yaml).expect("yaml parses");
        def.resolve_objects(1);
        def
    }

    #[test]
    fn inline_children_get_unique_ids_and_parent_contents() {
        let yaml = r"
authored_id: t
width: 4
height: 4
fill_floor_type: grass
objects:
  - type: barrel
    placement: { x: 1, y: 1 }
    contents:
      - type: potion
      - type: scroll
        properties:
          spell_id: spark_bolt
";
        let def = parse_and_resolve(yaml);
        assert_eq!(def.resolved_objects.len(), 3);
        let barrel = &def.resolved_objects[0];
        let potion = &def.resolved_objects[1];
        let scroll = &def.resolved_objects[2];
        assert_eq!(barrel.type_id, "barrel");
        assert_eq!(potion.type_id, "potion");
        assert_eq!(scroll.type_id, "scroll");
        assert_eq!(scroll.properties.get("spell_id").unwrap(), "spark_bolt");
        assert_eq!(barrel.contents, vec![potion.id, scroll.id]);
        assert!(potion.placement.is_none());
        assert!(scroll.placement.is_none());
    }

    #[test]
    fn symbolic_references_resolve_to_runtime_ids() {
        let yaml = r"
authored_id: t
width: 4
height: 4
fill_floor_type: grass
objects:
  - type: barrel
    placement: { x: 0, y: 0 }
    contents: [shiny_potion]
  - id: shiny_potion
    type: potion
";
        let def = parse_and_resolve(yaml);
        let barrel = def
            .resolved_objects
            .iter()
            .find(|o| o.type_id == "barrel")
            .unwrap();
        let potion = def
            .resolved_objects
            .iter()
            .find(|o| o.type_id == "potion")
            .unwrap();
        assert_eq!(barrel.contents, vec![potion.id]);
    }

    #[test]
    #[should_panic(expected = "does not match any object id")]
    fn missing_reference_panics() {
        let yaml = r"
authored_id: t
width: 4
height: 4
fill_floor_type: grass
objects:
  - type: barrel
    placement: { x: 0, y: 0 }
    contents: [does_not_exist]
";
        parse_and_resolve(yaml);
    }

    #[test]
    #[should_panic(expected = "Duplicate object id")]
    fn duplicate_symbolic_id_panics() {
        let yaml = r"
authored_id: t
width: 4
height: 4
fill_floor_type: grass
objects:
  - id: foo
    type: potion
  - id: foo
    type: scroll
";
        parse_and_resolve(yaml);
    }

    #[test]
    #[should_panic(expected = "must not declare `placement`")]
    fn inline_child_with_placement_panics() {
        let yaml = r"
authored_id: t
width: 4
height: 4
fill_floor_type: grass
objects:
  - type: barrel
    placement: { x: 0, y: 0 }
    contents:
      - type: potion
        placement: { x: 1, y: 1 }
";
        parse_and_resolve(yaml);
    }

    fn lever_definitions() -> OverworldObjectDefinitions {
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
"#;
        let lever_def: crate::world::object_definitions::OverworldObjectDefinition =
            serde_yaml::from_str(yaml).expect("definition parses");

        let door_yaml = r#"
name: Door
description: ""
colliding: true
movable: false
storable: false
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
"#;
        let door_def: crate::world::object_definitions::OverworldObjectDefinition =
            serde_yaml::from_str(door_yaml).expect("definition parses");

        let mut map = HashMap::new();
        map.insert("lever".to_owned(), lever_def);
        map.insert("wooden_door".to_owned(), door_def);
        OverworldObjectDefinitions::new_for_test(map)
    }

    #[test]
    fn wires_to_resolves_authored_id_to_runtime_u64() {
        let yaml = r#"
authored_id: t
width: 4
height: 4
fill_floor_type: grass
objects:
  - id: cellar_door
    type: wooden_door
    placement: { x: 1, y: 1 }
  - type: lever
    placement: { x: 2, y: 2 }
    properties:
      target: cellar_door
"#;
        let mut def = parse_and_resolve(yaml);
        def.resolve_wiring(&lever_definitions());
        let lever = def
            .resolved_objects
            .iter()
            .find(|o| o.type_id == "lever")
            .unwrap();
        let resolved_target = lever.properties.get("target").unwrap();
        // The lever's `target` should now be the door's runtime u64 (decimal).
        let target_id: u64 = resolved_target
            .parse()
            .expect("resolved target should be a runtime id");
        let door = def
            .resolved_objects
            .iter()
            .find(|o| o.type_id == "wooden_door")
            .unwrap();
        assert_eq!(target_id, door.id);
    }

    #[test]
    #[should_panic(expected = "no authored object with that id exists")]
    fn wires_to_panics_on_missing_target() {
        let yaml = r#"
authored_id: t
width: 4
height: 4
fill_floor_type: grass
objects:
  - type: lever
    placement: { x: 2, y: 2 }
    properties:
      target: ghost_door
"#;
        let mut def = parse_and_resolve(yaml);
        def.resolve_wiring(&lever_definitions());
    }
}
