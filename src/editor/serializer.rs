use std::collections::HashMap;

use bevy::log::info;
use serde::Serialize;

use crate::editor::resources::{EditorContext, EditorPortalBuffer};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::map_layout::{SpacePermanence, TileCoordinate};
use crate::world::object_registry::ObjectRegistry;

#[derive(Serialize)]
struct SpaceOutput {
    authored_id: String,
    width: i32,
    height: i32,
    fill_object_type: String,
    permanence: SpacePermanence,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    portals: Vec<PortalOutput>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    objects: Vec<ObjectEntryOutput>,
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
    id: u64,
    #[serde(rename = "type")]
    type_id: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    properties: HashMap<String, String>,
    placement: TileCoordinate,
}

/// Collect objects from ECS, serialize as YAML, write to disk.
pub fn serialize_and_save(
    ctx: &EditorContext,
    portal_buffer: &EditorPortalBuffer,
    object_registry: &ObjectRegistry,
    objects: &bevy::prelude::Query<(&OverworldObject, &SpaceResident, &TilePosition)>,
) {
    let mut items: Vec<(u64, String, HashMap<String, String>, TileCoordinate)> = Vec::new();
    for (obj, resident, tile) in objects.iter() {
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
        items.push((
            obj.object_id,
            type_id,
            properties,
            TileCoordinate {
                x: tile.x,
                y: tile.y,
            },
        ));
    }

    let mut anonymous: HashMap<String, Vec<TileCoordinate>> = HashMap::new();
    let mut explicit: Vec<ExplicitOutput> = Vec::new();
    for (object_id, type_id, properties, tile) in items {
        if properties.is_empty() {
            anonymous.entry(type_id).or_default().push(tile);
        } else {
            explicit.push(ExplicitOutput {
                id: object_id,
                type_id,
                properties,
                placement: tile,
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
    explicit.sort_by_key(|e| e.id);
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

    let output = SpaceOutput {
        authored_id: ctx.authored_id.clone(),
        width: ctx.map_width,
        height: ctx.map_height,
        fill_object_type: ctx.fill_object_type.clone(),
        permanence: SpacePermanence::Persistent,
        portals,
        objects: object_entries,
    };

    let yaml = serde_yaml::to_string(&output)
        .unwrap_or_else(|e| panic!("Failed to serialize map '{}': {e}", ctx.authored_id));
    let path = format!("assets/maps/{}.yaml", ctx.authored_id);
    std::fs::write(&path, yaml)
        .unwrap_or_else(|e| panic!("Failed to write map file '{path}': {e}"));
    info!("Saved map to {path}");
}
