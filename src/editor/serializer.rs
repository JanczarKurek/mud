use std::collections::HashMap;

use bevy::log::info;
use serde::Serialize;

use crate::editor::resources::{
    EditorContext, EditorLightingBuffer, EditorPortalBuffer, EditorSpawnGroupBuffer,
    EditorVendorStashBuffer,
};
use crate::npc::components::SpawnGroupMember;
use crate::player::components::{InventoryStack, Player};
use crate::world::components::{Container, OverworldObject, SpaceResident, TilePosition};
use crate::world::map_layout::{
    MapBehavior, SpaceLightingDef, SpacePermanence, SpawnGroupDef, TileCoordinate, VendorStashDef,
};
use crate::world::object_registry::ObjectRegistry;

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
}

#[derive(Serialize)]
struct ExplicitOutput {
    #[serde(rename = "type")]
    type_id: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    properties: HashMap<String, String>,
    placement: TileCoordinate,
    #[serde(skip_serializing_if = "Option::is_none")]
    behavior: Option<MapBehavior>,
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

/// Collect objects from ECS, serialize as YAML, write to disk.
#[allow(clippy::too_many_arguments)]
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
    floor_maps: &crate::world::floor_map::FloorMaps,
) {
    let mut items: Vec<(
        u64,
        String,
        HashMap<String, String>,
        Option<MapBehavior>,
        Vec<ContainedObjectOutput>,
        TileCoordinate,
    )> = Vec::new();
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
        items.push((
            obj.object_id,
            type_id,
            properties,
            behavior,
            contents,
            TileCoordinate {
                x: tile.x,
                y: tile.y,
                z: tile.z,
            },
        ));
    }

    let mut anonymous: HashMap<String, Vec<TileCoordinate>> = HashMap::new();
    let mut explicit: Vec<ExplicitOutput> = Vec::new();
    for (_object_id, type_id, properties, behavior, contents, tile) in items {
        // A populated container can't be anonymous: `AnonymousOutput` has no
        // `contents:` field, so it would silently drop the items.
        if properties.is_empty() && behavior.is_none() && contents.is_empty() {
            anonymous.entry(type_id).or_default().push(tile);
        } else {
            explicit.push(ExplicitOutput {
                type_id,
                properties,
                placement: tile,
                behavior,
                contents,
            });
        }
    }

    let mut object_entries: Vec<ObjectEntryOutput> = Vec::new();
    let mut anon_sorted: Vec<(String, Vec<TileCoordinate>)> = anonymous.into_iter().collect();
    anon_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (type_id, mut placements) in anon_sorted {
        placements.sort_by(|a, b| a.y.cmp(&b.y).then(a.x.cmp(&b.x)));
        object_entries.push(ObjectEntryOutput::Anonymous(AnonymousOutput {
            type_id,
            properties: HashMap::new(),
            placement: placements,
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
                if z == crate::world::components::TilePosition::GROUND_FLOOR
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
        permanence: SpacePermanence::Persistent,
        lighting: lighting_buffer.config.clone(),
        portals,
        floors: floors_out,
        objects: object_entries,
        spawn_groups: spawn_group_buffer.groups.clone(),
        vendor_stashes: vendor_stash_buffer.stashes.clone(),
    };

    let yaml = serde_yaml::to_string(&output)
        .unwrap_or_else(|e| panic!("Failed to serialize map '{}': {e}", ctx.authored_id));
    let path = format!("assets/maps/{}.yaml", ctx.authored_id);
    std::fs::write(&path, yaml)
        .unwrap_or_else(|e| panic!("Failed to write map file '{path}': {e}"));
    info!("Saved map to {path}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::components::SpaceId;
    use crate::world::map_layout::{AmbientKeyframe, SpaceDefinition};

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
            type_id: "iron_chest".into(),
            properties: HashMap::new(),
            placement: TileCoordinate { x: 2, y: 3, z: 0 },
            behavior: None,
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
        use crate::combat::damage_type::DamageType;
        use crate::combat::modifiers::{ItemModifier, ModifierDuration, ModifierEffect};
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
