use std::collections::HashMap;
use std::path::PathBuf;

use bevy::log::info;
use serde::Serialize;

use crate::editor::resources::{
    EditorContext, EditorLightingBuffer, EditorPortalBuffer, EditorSpawnGroupBuffer,
    EditorVendorStashBuffer,
};
use mud2::npc::components::SpawnGroupMember;
use mud2::player::components::{InventoryStack, Player};
use mud2::world::components::{Container, OverworldObject, SpaceResident, TilePosition};
use mud2::world::direction::Direction;
use mud2::world::map_layout::{
    MapBehavior, RoutineInstanceDef, SpaceLightingDef, SpacePermanence, SpawnGroupDef,
    TileCoordinate, VendorStashDef,
};
use mud2::world::object_registry::{AuthoredMeta, ObjectRegistry};

#[derive(Serialize)]
struct SpaceOutput {
    authored_id: String,
    width: i32,
    height: i32,
    fill_floor_type: String,
    permanence: SpacePermanence,
    #[serde(skip_serializing_if = "is_default_lighting")]
    lighting: SpaceLightingDef,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    portals: Vec<PortalOutput>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    floors: HashMap<String, FloorPlacementsOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    objects: Vec<ObjectEntryOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    spawn_groups: Vec<SpawnGroupDef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    vendor_stashes: Vec<VendorStashDef>,
}

/// Skip emitting `lighting:` when every field equals `SpaceLightingDef::default()`,
/// keeping YAML for unauthored maps free of noise. Any deviation — a single
/// keyframe, a tweaked ambient — produces the full block.
fn is_default_lighting(lighting: &SpaceLightingDef) -> bool {
    *lighting == SpaceLightingDef::default()
}

#[derive(Serialize, Default)]
struct FloorPlacementsOutput {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    placement: Vec<TileCoordinate>,
}

#[derive(Serialize)]
struct PortalOutput {
    id: String,
    source: TileCoordinate,
    destination_space_id: String,
    destination_tile: TileCoordinate,
    #[serde(skip_serializing_if = "Option::is_none")]
    destination_permanence: Option<SpacePermanence>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ObjectEntryOutput {
    Anonymous(AnonymousOutput),
    Explicit(ExplicitOutput),
}

#[derive(Serialize)]
struct AnonymousOutput {
    #[serde(rename = "type")]
    type_id: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    properties: HashMap<String, String>,
    placement: Vec<TileCoordinate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facing: Option<Direction>,
}

/// Write twin of `MapObjectInstance`. Every field the read type accepts must
/// have a counterpart here or an editor save silently drops it — `id` going
/// missing is what broke `overworld`'s crypt-gate wiring.
#[derive(Serialize)]
struct ExplicitOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(rename = "type")]
    type_id: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    properties: HashMap<String, String>,
    placement: TileCoordinate,
    #[serde(skip_serializing_if = "Option::is_none")]
    behavior: Option<MapBehavior>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facing: Option<Direction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    routine: Option<RoutineInstanceDef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quantity: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    contents: Vec<ContainedObjectOutput>,
}

/// Serialize twin of an inline `MapObjectChild` (a container's `contents:`
/// entry). Mirrors the read-side `MapObjectInstance` shape so authored maps
/// round-trip. `MapObjectChild::Reference` is never emitted: the editor's
/// source of truth is the flattened `Container.slots`, which carries no
/// symbolic ids, so every child is written inline.
#[derive(Serialize)]
struct ContainedObjectOutput {
    #[serde(rename = "type")]
    type_id: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    properties: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quantity: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    contents: Vec<ContainedObjectOutput>,
}

/// Pack a container's slots into a dense `contents:` list, dropping empty
/// slots (`.flatten()`) so YAML's gapless list round-trips exactly.
fn slots_to_contents(slots: &[Option<InventoryStack>]) -> Vec<ContainedObjectOutput> {
    slots.iter().flatten().map(stack_to_contained).collect()
}

fn stack_to_contained(stack: &InventoryStack) -> ContainedObjectOutput {
    if !stack.modifiers.is_empty() {
        // Map YAML `contents:` has no field for per-instance modifiers, so
        // they can't round-trip. Editor-authored contents never carry them
        // (no UI creates them); warn in case a runtime-mutated container is
        // ever saved from the editor.
        bevy::log::warn!(
            "Editor save: dropping {} modifier(s) on contained item '{}' — \
             map YAML contents cannot express item modifiers",
            stack.modifiers.len(),
            stack.type_id,
        );
    }
    ContainedObjectOutput {
        type_id: stack.type_id.clone(),
        properties: stack.properties.clone(),
        quantity: (stack.quantity > 1).then_some(stack.quantity),
        contents: stack
            .contained_slots
            .as_deref()
            .map(slots_to_contents)
            .unwrap_or_default(),
    }
}

/// One saveable object, gathered from the ECS plus its registry entries.
struct Item {
    type_id: String,
    properties: HashMap<String, String>,
    behavior: Option<MapBehavior>,
    contents: Vec<ContainedObjectOutput>,
    tile: TileCoordinate,
    authored: AuthoredMeta,
}

/// Bucket objects into the compact `type + [tiles]` form where possible, and
/// the fully-explicit form otherwise.
///
/// `AnonymousOutput` has no `id:`, `behavior:`, `contents:`, `routine:` or
/// `quantity:` field, so an object carrying any of those *must* go the explicit
/// route — writing it anonymously drops the data silently. That is precisely
/// how `overworld`'s `id: crypt_gate` was lost, leaving the pressure plate's
/// `target: crypt_gate` dangling and panicking the next load.
fn build_object_entries(items: Vec<Item>) -> Vec<ObjectEntryOutput> {
    let mut anonymous: HashMap<(String, Option<Direction>), Vec<TileCoordinate>> = HashMap::new();
    let mut explicit: Vec<ExplicitOutput> = Vec::new();
    for item in items {
        let expressible_anonymously = item.properties.is_empty()
            && item.behavior.is_none()
            && item.contents.is_empty()
            && item.authored.authored_id.is_none()
            && item.authored.routine.is_none()
            && item.authored.quantity.is_none();
        if expressible_anonymously {
            anonymous
                .entry((item.type_id, item.authored.facing))
                .or_default()
                .push(item.tile);
        } else {
            explicit.push(ExplicitOutput {
                id: item.authored.authored_id,
                type_id: item.type_id,
                properties: item.properties,
                placement: item.tile,
                behavior: item.behavior,
                facing: item.authored.facing,
                routine: item.authored.routine,
                quantity: item.authored.quantity,
                contents: item.contents,
            });
        }
    }

    let mut object_entries: Vec<ObjectEntryOutput> = Vec::new();
    let mut anon_sorted: Vec<((String, Option<Direction>), Vec<TileCoordinate>)> =
        anonymous.into_iter().collect();
    anon_sorted.sort_by(|a, b| {
        a.0 .0
            .cmp(&b.0 .0)
            .then_with(|| format!("{:?}", a.0 .1).cmp(&format!("{:?}", b.0 .1)))
    });
    for ((type_id, facing), mut placements) in anon_sorted {
        placements.sort_by(|a, b| a.y.cmp(&b.y).then(a.x.cmp(&b.x)));
        object_entries.push(ObjectEntryOutput::Anonymous(AnonymousOutput {
            type_id,
            properties: HashMap::new(),
            placement: placements,
            facing,
        }));
    }
    explicit.sort_by(|a, b| {
        a.placement
            .y
            .cmp(&b.placement.y)
            .then(a.placement.x.cmp(&b.placement.x))
            .then(a.type_id.cmp(&b.type_id))
    });
    for entry in explicit {
        object_entries.push(ObjectEntryOutput::Explicit(entry));
    }
    object_entries
}

/// Collect objects from ECS, serialize as YAML, write to disk.
pub fn serialize_and_save(
    ctx: &EditorContext,
    portal_buffer: &EditorPortalBuffer,
    spawn_group_buffer: &EditorSpawnGroupBuffer,
    lighting_buffer: &EditorLightingBuffer,
    vendor_stash_buffer: &EditorVendorStashBuffer,
    object_registry: &ObjectRegistry,
    objects: &bevy::prelude::Query<
        (
            &OverworldObject,
            &SpaceResident,
            &TilePosition,
            Option<&Container>,
        ),
        (
            bevy::prelude::Without<SpawnGroupMember>,
            bevy::prelude::Without<Player>,
        ),
    >,
    floor_maps: &mud2::world::floor_map::FloorMaps,
) {
    let mut items: Vec<Item> = Vec::new();
    for (obj, resident, tile, container) in objects.iter() {
        if resident.space_id != ctx.space_id {
            continue;
        }
        let type_id = object_registry
            .type_id(obj.object_id)
            .unwrap_or(&obj.definition_id)
            .to_owned();
        let properties = object_registry
            .properties(obj.object_id)
            .cloned()
            .unwrap_or_default();
        let behavior = object_registry.behavior(obj.object_id).cloned();
        let contents = container
            .map(|c| slots_to_contents(&c.slots))
            .unwrap_or_default();
        items.push(Item {
            type_id,
            properties,
            behavior,
            contents,
            tile: TileCoordinate {
                x: tile.x,
                y: tile.y,
                z: tile.z,
            },
            authored: object_registry
                .authored_meta(obj.object_id)
                .cloned()
                .unwrap_or_default(),
        });
    }

    let object_entries = build_object_entries(items);

    let portals = portal_buffer
        .portals
        .iter()
        .map(|p| PortalOutput {
            id: p.id.clone(),
            source: p.source,
            destination_space_id: p.destination_space_id.clone(),
            destination_tile: p.destination_tile,
            destination_permanence: p.destination_permanence,
        })
        .collect::<Vec<_>>();

    // Collect floor placements for every floor of the active space, grouped
    // by floor type. Omit ground-floor cells whose floor type equals the
    // `fill_floor_type` since they round-trip through the fill at load time;
    // upper floors never get a fill, so every non-empty cell there is
    // explicit.
    let mut floor_groups: HashMap<String, Vec<TileCoordinate>> = HashMap::new();
    for (space_id, z, map) in floor_maps.iter() {
        if space_id != ctx.space_id {
            continue;
        }
        for y in 0..map.height {
            for x in 0..map.width {
                let idx = (y * map.width + x) as usize;
                let Some(floor) = map.tiles.get(idx).and_then(|t| t.as_ref()) else {
                    continue;
                };
                if z == mud2::world::components::TilePosition::GROUND_FLOOR
                    && *floor == ctx.fill_floor_type
                {
                    continue;
                }
                floor_groups
                    .entry(floor.clone())
                    .or_default()
                    .push(TileCoordinate { x, y, z });
            }
        }
    }
    let mut floors_out: HashMap<String, FloorPlacementsOutput> = HashMap::new();
    for (k, mut tiles) in floor_groups {
        tiles.sort_by(|a, b| a.y.cmp(&b.y).then(a.x.cmp(&b.x)));
        floors_out.insert(k, FloorPlacementsOutput { placement: tiles });
    }

    let output = SpaceOutput {
        authored_id: ctx.authored_id.clone(),
        width: ctx.map_width,
        height: ctx.map_height,
        fill_floor_type: ctx.fill_floor_type.clone(),
        permanence: ctx.permanence,
        lighting: lighting_buffer.config.clone(),
        portals,
        floors: floors_out,
        objects: object_entries,
        spawn_groups: spawn_group_buffer.groups.clone(),
        vendor_stashes: vendor_stash_buffer.stashes.clone(),
    };

    let yaml = serde_yaml::to_string(&output)
        .unwrap_or_else(|e| panic!("Failed to serialize map '{}': {e}", ctx.authored_id));
    // Overwrite the file this space came from. Only maps created in-editor
    // lack a source path, and those genuinely belong in `assets/maps/`.
    let path = ctx
        .source_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("assets/maps/{}.yaml", ctx.authored_id)));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            panic!("Failed to create map directory '{}': {e}", parent.display())
        });
    }
    std::fs::write(&path, yaml)
        .unwrap_or_else(|e| panic!("Failed to write map file '{}': {e}", path.display()));
    info!("Saved map to {}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud2::world::components::SpaceId;
    use mud2::world::map_layout::{AmbientKeyframe, MapObjectEntry, SpaceDefinition};

    /// Rebuild the `Item` list the save path would collect for a space, from
    /// the resolved definition rather than a live ECS. `spawn_overworld_object_
    /// instance` copies exactly these fields out of `ResolvedObject`, so this
    /// stands in for "the map was opened in the editor and saved untouched".
    fn items_for(def: &SpaceDefinition) -> Vec<Item> {
        def.resolved_objects
            .iter()
            .filter(|o| !def.is_contained(o.id))
            .filter_map(|o| {
                Some(Item {
                    type_id: o.type_id.clone(),
                    properties: o.properties.clone(),
                    behavior: o.behavior,
                    // Containers round-trip through `Container.slots`; this
                    // test is about the authored anchors, so leave them empty.
                    contents: Vec::new(),
                    tile: o.placement?,
                    authored: AuthoredMeta::from_resolved(o),
                })
            })
            .collect()
    }

    fn load_shipped_maps() -> Vec<SpaceDefinition> {
        let mut next_id = 1;
        let mut defs = Vec::new();
        for asset in mud2::assets::discover_yaml_assets("maps", "map layout") {
            let mut def: SpaceDefinition = serde_yaml::from_str(&asset.contents)
                .unwrap_or_else(|e| panic!("parse {}: {e}", asset.path.display()));
            next_id = def.resolve_objects(next_id);
            defs.push(def);
        }
        assert!(!defs.is_empty(), "no shipped maps discovered");
        defs
    }

    /// The regression this whole change exists for: every authored `id:` in a
    /// shipped map must survive an editor save. Losing one silently dangles
    /// any `wires_to` property or `contents:` reference pointing at it, and
    /// the *next* load panics in `resolve_wiring`.
    #[test]
    fn editor_save_round_trips_authored_ids() {
        let mut total_ids = 0usize;
        for def in load_shipped_maps() {
            let expected: std::collections::BTreeSet<String> = def
                .resolved_objects
                .iter()
                .filter_map(|o| o.authored_id.clone())
                .collect();

            let yaml = serde_yaml::to_string(&build_object_entries(items_for(&def))).unwrap();
            let entries: Vec<MapObjectEntry> = serde_yaml::from_str(&yaml).unwrap();
            let actual: std::collections::BTreeSet<String> = entries
                .iter()
                .filter_map(|e| match e {
                    MapObjectEntry::Explicit(i) => i.id.clone(),
                    MapObjectEntry::Anonymous(_) => None,
                })
                .collect();

            // Contained objects have no placement and are not written as
            // top-level entries, so only compare the ones that are placed.
            let placed: std::collections::BTreeSet<String> = def
                .resolved_objects
                .iter()
                .filter(|o| o.placement.is_some() && !def.is_contained(o.id))
                .filter_map(|o| o.authored_id.clone())
                .collect();
            assert_eq!(
                placed, actual,
                "space '{}': authored ids lost on save (all authored: {expected:?})",
                def.authored_id,
            );
            total_ids += placed.len();
        }
        // Guard against the assertions above passing vacuously if the shipped
        // maps ever stop using authored ids.
        assert!(
            total_ids > 0,
            "no placed authored ids found in any shipped map — test proves nothing"
        );
    }

    /// `facing:` has no other home — it is not inferable from the ECS, since
    /// an unauthored object still gets a `Facing` from its definition default.
    #[test]
    fn editor_save_round_trips_facing_and_placements() {
        let mut total_facings = 0usize;
        for def in load_shipped_maps() {
            let items = items_for(&def);
            let expected_placements: std::collections::BTreeSet<(String, i32, i32, i32)> = items
                .iter()
                .map(|i| (i.type_id.clone(), i.tile.x, i.tile.y, i.tile.z))
                .collect();
            let expected_facings: std::collections::BTreeSet<(String, i32, i32)> = items
                .iter()
                .filter(|i| i.authored.facing.is_some())
                .map(|i| (i.type_id.clone(), i.tile.x, i.tile.y))
                .collect();

            let yaml = serde_yaml::to_string(&build_object_entries(items)).unwrap();
            let entries: Vec<MapObjectEntry> = serde_yaml::from_str(&yaml).unwrap();

            let mut placements = std::collections::BTreeSet::new();
            let mut facings = std::collections::BTreeSet::new();
            for entry in &entries {
                match entry {
                    MapObjectEntry::Explicit(i) => {
                        let t = i.placement.expect("explicit entry lost its placement");
                        placements.insert((i.type_id.clone(), t.x, t.y, t.z));
                        if i.facing.is_some() {
                            facings.insert((i.type_id.clone(), t.x, t.y));
                        }
                    }
                    MapObjectEntry::Anonymous(g) => {
                        for t in &g.placement {
                            placements.insert((g.type_id.clone(), t.x, t.y, t.z));
                            if g.facing.is_some() {
                                facings.insert((g.type_id.clone(), t.x, t.y));
                            }
                        }
                    }
                }
            }

            assert_eq!(
                expected_placements, placements,
                "space '{}': object placements changed on save",
                def.authored_id
            );
            assert_eq!(
                expected_facings, facings,
                "space '{}': authored facing lost on save",
                def.authored_id
            );
            total_facings += facings.len();
        }
        assert!(
            total_facings > 0,
            "no authored facings found in any shipped map — test proves nothing"
        );
    }

    /// End-to-end reproduction of the reported crash: save every shipped map
    /// the way the editor does, re-parse it, and run the same
    /// `resolve_objects` + `resolve_wiring` the game runs at boot. Before the
    /// fix this panicked with "has property 'target: crypt_gate' but no
    /// authored object with that id exists in this space".
    #[test]
    fn saved_maps_still_resolve_their_wiring() {
        let object_definitions =
            mud2::world::object_definitions::OverworldObjectDefinitions::load_from_disk();

        for def in load_shipped_maps() {
            let output = SpaceOutput {
                authored_id: def.authored_id.clone(),
                width: def.width,
                height: def.height,
                fill_floor_type: def.fill_floor_type.clone(),
                permanence: def.permanence,
                lighting: def.lighting.clone(),
                portals: Vec::new(),
                floors: HashMap::new(),
                objects: build_object_entries(items_for(&def)),
                spawn_groups: Vec::new(),
                vendor_stashes: Vec::new(),
            };
            let yaml = serde_yaml::to_string(&output).unwrap();

            let mut reloaded: SpaceDefinition = serde_yaml::from_str(&yaml)
                .unwrap_or_else(|e| panic!("space '{}' re-parse: {e}", def.authored_id));
            reloaded.resolve_objects(1);
            // Panics on a dangling `wires_to` target — the original bug.
            reloaded.resolve_wiring(&object_definitions);
        }
    }

    /// `permanence` used to be hardcoded to `persistent` on save, silently
    /// converting the two ephemeral maps into persistent ones.
    #[test]
    fn shipped_ephemeral_maps_keep_their_permanence() {
        use mud2::world::map_layout::SpacePermanence;
        let defs = load_shipped_maps();
        let ephemeral: Vec<&str> = defs
            .iter()
            .filter(|d| matches!(d.permanence, SpacePermanence::Ephemeral))
            .map(|d| d.authored_id.as_str())
            .collect();
        assert!(
            !ephemeral.is_empty(),
            "expected at least one ephemeral shipped map to guard the save path"
        );
        // The editor writes `ctx.permanence`, which is seeded from the loaded
        // definition; a round-trip through the output struct must preserve it.
        for def in &defs {
            let yaml = serde_yaml::to_string(&def.permanence).unwrap();
            let back: SpacePermanence = serde_yaml::from_str(&yaml).unwrap();
            assert_eq!(
                std::mem::discriminant(&def.permanence),
                std::mem::discriminant(&back),
                "space '{}': permanence did not round-trip",
                def.authored_id
            );
        }
    }

    /// Confirms the save query's `Without<SpawnGroupMember>` filter excludes
    /// runtime-spawned NPCs — the original bug was that respawned rats /
    /// goblins were getting baked back into the YAML on save.
    #[test]
    fn save_query_excludes_spawn_group_members() {
        use bevy::prelude::*;

        let mut app = App::new();
        let space_id = SpaceId(1);

        // Authored object: should be picked up by the save query.
        app.world_mut().spawn((
            OverworldObject {
                object_id: 100,
                definition_id: "wooden_door".into(),
                placement_seq: 0,
            },
            SpaceResident { space_id },
            TilePosition::ground(2, 3),
        ));

        // Spawn-group NPC: tagged with SpawnGroupMember, must be filtered out.
        app.world_mut().spawn((
            OverworldObject {
                object_id: 200,
                definition_id: "rat".into(),
                placement_seq: 0,
            },
            SpaceResident { space_id },
            TilePosition::ground(5, 5),
            SpawnGroupMember {
                space_id,
                group_id: "cellar_rats".into(),
            },
        ));

        let collected = app
            .world_mut()
            .query_filtered::<
                (&OverworldObject, &SpaceResident, &TilePosition),
                (Without<SpawnGroupMember>, Without<Player>),
            >()
            .iter(app.world())
            .map(|(obj, _, tile)| (obj.object_id, obj.definition_id.clone(), tile.x, tile.y))
            .collect::<Vec<_>>();

        assert_eq!(
            collected,
            vec![(100, "wooden_door".to_owned(), 2, 3)],
            "save query should yield only the authored door, not the spawn-group rat",
        );
    }

    #[test]
    fn lighting_round_trips_through_yaml() {
        let lighting = SpaceLightingDef {
            outdoor_ambient: [200, 180, 160],
            indoor_ambient: [40, 30, 30],
            has_day_night: true,
            darkness_color: [0, 0, 0],
            outdoor_curve: vec![
                AmbientKeyframe {
                    time: 0.0,
                    color: [20, 30, 80],
                    alpha: 0.6,
                },
                AmbientKeyframe {
                    time: 0.5,
                    color: [255, 255, 255],
                    alpha: 0.0,
                },
            ],
        };
        let output = SpaceOutput {
            authored_id: "round_trip_test".into(),
            width: 4,
            height: 4,
            fill_floor_type: "grass".into(),
            permanence: SpacePermanence::Persistent,
            lighting: lighting.clone(),
            portals: Vec::new(),
            floors: HashMap::new(),
            objects: Vec::new(),
            spawn_groups: Vec::new(),
            vendor_stashes: Vec::new(),
        };
        let yaml = serde_yaml::to_string(&output).expect("serialize");
        let parsed: SpaceDefinition = serde_yaml::from_str(&yaml).expect("parse");
        assert_eq!(parsed.lighting, lighting);
    }

    /// A container's slots must serialize into a `contents:` block that parses
    /// back with quantity and one level of nesting intact — the write side of
    /// the round-trip the editor relies on.
    #[test]
    fn container_contents_round_trip_through_yaml() {
        let pouch = InventoryStack {
            type_id: "small_pouch".into(),
            properties: HashMap::new(),
            quantity: 1,
            contained_slots: Some(vec![
                Some(InventoryStack::item("herb", HashMap::new(), 3)),
                None,
            ]),
            modifiers: Vec::new(),
        };
        let slots = vec![
            Some(InventoryStack::item("apple", HashMap::new(), 5)),
            None,
            Some(pouch),
        ];
        let explicit = ExplicitOutput {
            id: None,
            type_id: "iron_chest".into(),
            properties: HashMap::new(),
            placement: TileCoordinate { x: 2, y: 3, z: 0 },
            behavior: None,
            facing: None,
            routine: None,
            quantity: None,
            contents: slots_to_contents(&slots),
        };
        // Dense packing: the empty slot between apple and pouch is dropped.
        assert_eq!(explicit.contents.len(), 2);

        let output = SpaceOutput {
            authored_id: "t".into(),
            width: 8,
            height: 8,
            fill_floor_type: "grass".into(),
            permanence: SpacePermanence::Persistent,
            lighting: SpaceLightingDef::default(),
            portals: Vec::new(),
            floors: HashMap::new(),
            objects: vec![ObjectEntryOutput::Explicit(explicit)],
            spawn_groups: Vec::new(),
            vendor_stashes: Vec::new(),
        };
        let yaml = serde_yaml::to_string(&output).expect("serialize");
        let mut parsed: SpaceDefinition = serde_yaml::from_str(&yaml).expect("parse");
        parsed.resolve_objects(1);

        let chest = parsed
            .resolved_objects
            .iter()
            .find(|o| o.type_id == "iron_chest")
            .expect("chest");
        let child_types: Vec<(&str, Option<u32>)> = chest
            .contents
            .iter()
            .filter_map(|&id| parsed.find_resolved(id))
            .map(|c| (c.type_id.as_str(), c.quantity))
            .collect();
        assert!(child_types.contains(&("apple", Some(5))));
        assert!(child_types.iter().any(|(t, _)| *t == "small_pouch"));

        let pouch = chest
            .contents
            .iter()
            .filter_map(|&id| parsed.find_resolved(id))
            .find(|c| c.type_id == "small_pouch")
            .expect("pouch");
        let grandchildren: Vec<&str> = pouch
            .contents
            .iter()
            .filter_map(|&id| parsed.find_resolved(id))
            .map(|c| c.type_id.as_str())
            .collect();
        assert_eq!(grandchildren, vec!["herb"]);
    }

    /// An empty container stays in the anonymous (grouped) bucket; a populated
    /// one is forced into an explicit entry that can carry `contents:`.
    #[test]
    fn empty_container_is_anonymous_populated_is_explicit() {
        let empty = slots_to_contents(&[None, None]);
        assert!(empty.is_empty());
        let populated =
            slots_to_contents(&[Some(InventoryStack::item("apple", HashMap::new(), 1))]);
        assert_eq!(populated.len(), 1);
    }

    /// Per-instance item modifiers have no YAML representation and are dropped
    /// on save (with a warning) — documents the limitation.
    #[test]
    fn modifiers_are_dropped_on_serialize() {
        use mud2::combat::damage_type::DamageType;
        use mud2::combat::modifiers::{ItemModifier, ModifierDuration, ModifierEffect};
        let stack = InventoryStack {
            type_id: "sword".into(),
            properties: HashMap::new(),
            quantity: 1,
            contained_slots: None,
            modifiers: vec![ItemModifier {
                type_ex: "flaming".into(),
                lvl: 1,
                effect: ModifierEffect::BonusDamage {
                    dice: Some((1, 6)),
                    bonus: 0,
                    damage_type: DamageType::Fire,
                },
                duration: ModifierDuration::Permanent,
                label: String::new(),
            }],
        };
        let out = stack_to_contained(&stack);
        assert_eq!(out.type_id, "sword");
        // No modifier data survives — the output struct has no field for it.
    }

    #[test]
    fn default_lighting_is_not_emitted() {
        let output = SpaceOutput {
            authored_id: "default_map".into(),
            width: 2,
            height: 2,
            fill_floor_type: "grass".into(),
            permanence: SpacePermanence::Persistent,
            lighting: SpaceLightingDef::default(),
            portals: Vec::new(),
            floors: HashMap::new(),
            objects: Vec::new(),
            spawn_groups: Vec::new(),
            vendor_stashes: Vec::new(),
        };
        let yaml = serde_yaml::to_string(&output).expect("serialize");
        assert!(
            !yaml.lines().any(|l| l.starts_with("lighting:")),
            "default lighting should not appear in YAML: {yaml}"
        );
    }
}
