use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::npc::components::{Npc, SpawnGroupMember};
use crate::player::components::Player;
use crate::world::components::{Collider, SpaceId, SpaceResident, TilePosition};
use crate::world::map_layout::{
    ResolvedObject, SpaceDefinitions, SpawnArea, SpawnGroupDef, TileCoordinate,
};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::setup::spawn_overworld_object_instance;

/// Identifies a spawn-group runtime: the runtime space it lives in plus its
/// authored id (unique within the space).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SpawnGroupKey {
    pub space_id: SpaceId,
    pub group_id: String,
}

#[derive(Debug)]
pub struct SpawnGroupRuntime {
    pub def: SpawnGroupDef,
    pub members: HashSet<Entity>,
    /// One pending exponential timer (seconds remaining) per empty slot.
    pub pending_respawns: Vec<f32>,
    /// LCG seed for spawn-tile picking and exponential interval sampling.
    pub rng_seed: u64,
}

impl SpawnGroupRuntime {
    pub fn new(def: SpawnGroupDef, rng_seed: u64) -> Self {
        let count = def.max_count as usize;
        Self {
            def,
            members: HashSet::new(),
            pending_respawns: vec![0.0; count],
            rng_seed,
        }
    }
}

#[derive(Resource, Default)]
pub struct SpawnGroupRegistry {
    pub groups: HashMap<SpawnGroupKey, SpawnGroupRuntime>,
}

/// Per-group runtime state lifted from a world snapshot. Populated by the
/// persistence load path; consumed once by `bootstrap_spawn_groups`.
#[derive(Resource, Default)]
pub struct PendingSpawnGroupDumps {
    pub entries: Vec<SpawnGroupRuntimeDump>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpawnGroupRuntimeDump {
    pub space_id: SpaceId,
    pub group_id: String,
    pub pending_respawns: Vec<f32>,
    pub rng_seed: u64,
}

/// LCG step matching `next_random_index` in `npc::systems`. Returns the new
/// state value.
fn next_random_u64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    *seed
}

/// Uniform float in `(0, 1)`. Uses the top 53 bits of the LCG state.
fn next_uniform_01(seed: &mut u64) -> f64 {
    let r = (next_random_u64(seed) >> 11) as f64;
    (r + 1.0) / (((1u64 << 53) as f64) + 1.0)
}

fn sample_exponential(seed: &mut u64, mean: f32) -> f32 {
    let u = next_uniform_01(seed) as f32;
    (-mean * u.ln()).max(0.0)
}

fn default_spawn_seed(authored_id: &str, group_id: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    authored_id.hash(&mut hasher);
    group_id.hash(&mut hasher);
    let h = hasher.finish();
    if h == 0 {
        1
    } else {
        h
    }
}

fn pick_spawn_tile(
    area: &SpawnArea,
    seed: &mut u64,
    blocked: &[TilePosition],
) -> Option<TilePosition> {
    const MAX_ATTEMPTS: u32 = 8;
    for _ in 0..MAX_ATTEMPTS {
        let candidate = if let Some(tiles) = &area.tiles {
            if tiles.is_empty() {
                return None;
            }
            let idx = (next_random_u64(seed) % tiles.len() as u64) as usize;
            let TileCoordinate { x, y, z } = tiles[idx];
            TilePosition::new(x, y, z)
        } else if let Some(rect) = &area.bounds {
            let dx = (rect.max_x - rect.min_x + 1).max(1) as u64;
            let dy = (rect.max_y - rect.min_y + 1).max(1) as u64;
            let x = rect.min_x + (next_random_u64(seed) % dx) as i32;
            let y = rect.min_y + (next_random_u64(seed) % dy) as i32;
            TilePosition::ground(x, y)
        } else {
            return None;
        };

        if !blocked.iter().any(|pos| *pos == candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Trim or pad `pending_respawns` so total active slots
/// (`members + pending`) match `def.max_count`. Padded slots fire on the
/// next tick; trimmed slots are surplus state from a stale dump.
fn reconcile_slot_count(runtime: &mut SpawnGroupRuntime) {
    let max_count = runtime.def.max_count as usize;
    let occupied = runtime.members.len();
    let target_pending = max_count.saturating_sub(occupied);
    if runtime.pending_respawns.len() > target_pending {
        runtime.pending_respawns.truncate(target_pending);
    } else {
        while runtime.pending_respawns.len() < target_pending {
            runtime.pending_respawns.push(0.0);
        }
    }
}

/// Walk every runtime space and ensure each authored `spawn_groups` entry has
/// a matching registry runtime. Idempotent — existing entries are left alone.
fn register_groups_from_definitions(
    registry: &mut SpawnGroupRegistry,
    space_manager: &SpaceManager,
    space_definitions: &SpaceDefinitions,
) {
    for runtime_space in space_manager.spaces.values() {
        let Some(def) = space_definitions.get(&runtime_space.authored_id) else {
            continue;
        };
        for group in &def.spawn_groups {
            let key = SpawnGroupKey {
                space_id: runtime_space.id,
                group_id: group.id.clone(),
            };
            if registry.groups.contains_key(&key) {
                continue;
            }
            let seed = default_spawn_seed(&runtime_space.authored_id, &group.id);
            registry
                .groups
                .insert(key, SpawnGroupRuntime::new(group.clone(), seed));
        }
    }
}

/// Startup: build the registry from authored spawn_groups, apply any persisted
/// dump state, and reconcile member entities loaded from a world snapshot.
pub fn bootstrap_spawn_groups(
    space_manager: Option<Res<SpaceManager>>,
    space_definitions: Option<Res<SpaceDefinitions>>,
    mut registry: ResMut<SpawnGroupRegistry>,
    mut pending_dumps: ResMut<PendingSpawnGroupDumps>,
    member_query: Query<(Entity, &SpawnGroupMember)>,
) {
    let (Some(space_manager), Some(space_definitions)) = (space_manager, space_definitions) else {
        return;
    };

    register_groups_from_definitions(&mut registry, &space_manager, &space_definitions);

    let dump_index: HashMap<SpawnGroupKey, SpawnGroupRuntimeDump> = pending_dumps
        .entries
        .drain(..)
        .map(|d| {
            let key = SpawnGroupKey {
                space_id: d.space_id,
                group_id: d.group_id.clone(),
            };
            (key, d)
        })
        .collect();

    for (key, runtime) in registry.groups.iter_mut() {
        if let Some(dump) = dump_index.get(key) {
            runtime.pending_respawns = dump.pending_respawns.clone();
            runtime.rng_seed = if dump.rng_seed == 0 { 1 } else { dump.rng_seed };
        }
    }

    for (entity, member) in member_query.iter() {
        let key = SpawnGroupKey {
            space_id: member.space_id,
            group_id: member.group_id.clone(),
        };
        if let Some(runtime) = registry.groups.get_mut(&key) {
            runtime.members.insert(entity);
        }
    }

    for runtime in registry.groups.values_mut() {
        reconcile_slot_count(runtime);
    }
}

/// Update: tick respawn timers, pick spawn tiles, instantiate NPCs through the
/// same factory used at startup so the projection layer picks them up via
/// `WorldObjectUpserted` automatically.
pub fn tick_spawn_groups(
    time: Res<Time>,
    space_manager: Option<Res<SpaceManager>>,
    space_definitions: Option<Res<SpaceDefinitions>>,
    object_definitions: Option<Res<OverworldObjectDefinitions>>,
    mut registry: ResMut<SpawnGroupRegistry>,
    mut object_registry: ResMut<ObjectRegistry>,
    blocker_query: Query<(&SpaceResident, &TilePosition), With<Collider>>,
    player_query: Query<(&SpaceResident, &TilePosition), With<Player>>,
    npc_query: Query<(&SpaceResident, &TilePosition), With<Npc>>,
    member_query: Query<Entity, With<SpawnGroupMember>>,
    mut commands: Commands,
) {
    let (Some(space_manager), Some(space_definitions), Some(object_definitions)) =
        (space_manager, space_definitions, object_definitions)
    else {
        return;
    };

    register_groups_from_definitions(&mut registry, &space_manager, &space_definitions);

    let dt = time.delta_secs();

    let live_members: HashSet<Entity> = member_query.iter().collect();
    for runtime in registry.groups.values_mut() {
        let before = runtime.members.len();
        runtime
            .members
            .retain(|entity| live_members.contains(entity));
        let lost = before - runtime.members.len();
        let mean = runtime.def.respawn_mean_seconds;
        for _ in 0..lost {
            let interval = sample_exponential(&mut runtime.rng_seed, mean);
            runtime.pending_respawns.push(interval);
        }
        let max_count = runtime.def.max_count as usize;
        let active_slots = runtime.members.len() + runtime.pending_respawns.len();
        if active_slots > max_count {
            let surplus = active_slots - max_count;
            let new_len = runtime.pending_respawns.len().saturating_sub(surplus);
            runtime.pending_respawns.truncate(new_len);
        }
    }

    let blockers: Vec<(SpaceId, TilePosition)> = blocker_query
        .iter()
        .map(|(r, t)| (r.space_id, *t))
        .collect();
    let players: Vec<(SpaceId, TilePosition)> =
        player_query.iter().map(|(r, t)| (r.space_id, *t)).collect();
    let npcs: Vec<(SpaceId, TilePosition)> =
        npc_query.iter().map(|(r, t)| (r.space_id, *t)).collect();

    for (key, runtime) in registry.groups.iter_mut() {
        let Some(runtime_space) = space_manager.get(key.space_id) else {
            continue;
        };
        let Some(space_def) = space_definitions.get(&runtime_space.authored_id) else {
            continue;
        };

        let blocked_for_space: Vec<TilePosition> = blockers
            .iter()
            .chain(players.iter())
            .chain(npcs.iter())
            .filter(|(s, _)| *s == key.space_id)
            .map(|(_, t)| *t)
            .collect();

        let pending = std::mem::take(&mut runtime.pending_respawns);
        let mut next_pending: Vec<f32> = Vec::with_capacity(pending.len());

        for remaining in pending {
            let next = remaining - dt;
            if next > 0.0 {
                next_pending.push(next);
                continue;
            }

            let Some(spawn_tile) =
                pick_spawn_tile(&runtime.def.area, &mut runtime.rng_seed, &blocked_for_space)
            else {
                next_pending.push(0.0);
                continue;
            };

            let new_id = object_registry.allocate_runtime_id(runtime.def.template.clone());
            let synthetic = ResolvedObject {
                id: new_id,
                type_id: runtime.def.template.clone(),
                properties: Default::default(),
                placement: Some(TileCoordinate {
                    x: spawn_tile.x,
                    y: spawn_tile.y,
                    z: spawn_tile.z,
                }),
                contents: Vec::new(),
                behavior: Some(runtime.def.behavior.clone()),
                facing: None,
            };

            let entity = spawn_overworld_object_instance(
                &mut commands,
                &object_definitions,
                &object_registry,
                space_def,
                &synthetic,
                key.space_id,
                spawn_tile,
            );

            commands.entity(entity).insert(SpawnGroupMember {
                space_id: key.space_id,
                group_id: key.group_id.clone(),
            });
            runtime.members.insert(entity);
        }

        runtime.pending_respawns = next_pending;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map_layout::{MapBehavior, TileRectangle};

    fn small_def(max_count: u32, mean: f32) -> SpawnGroupDef {
        SpawnGroupDef {
            id: "test".to_owned(),
            template: "rat".to_owned(),
            max_count,
            respawn_mean_seconds: mean,
            area: SpawnArea {
                bounds: Some(TileRectangle {
                    min_x: 0,
                    min_y: 0,
                    max_x: 4,
                    max_y: 4,
                }),
                tiles: None,
            },
            behavior: MapBehavior::Roam {
                step_interval_seconds: 0.5,
                bounds: TileRectangle {
                    min_x: 0,
                    min_y: 0,
                    max_x: 4,
                    max_y: 4,
                },
            },
        }
    }

    #[test]
    fn exponential_sampler_is_positive_and_finite() {
        let mut seed: u64 = 0xDEAD_BEEF;
        for _ in 0..100 {
            let s = sample_exponential(&mut seed, 30.0);
            assert!(s.is_finite(), "exponential sample must be finite, got {s}");
            assert!(s >= 0.0, "exponential sample must be non-negative, got {s}");
        }
    }

    #[test]
    fn pick_spawn_tile_avoids_blocked_tiles() {
        let area = SpawnArea {
            bounds: Some(TileRectangle {
                min_x: 0,
                min_y: 0,
                max_x: 3,
                max_y: 3,
            }),
            tiles: None,
        };
        let blocked = vec![TilePosition::ground(0, 0), TilePosition::ground(1, 1)];
        // Try several seeds. Each call should either return None (all 8 attempts
        // landed on a blocked tile — possible but rare) or a tile that is *not*
        // in the blocked list and is inside the area.
        for seed_init in [1u64, 7, 42, 0xCAFE, 0xDEAD_BEEF, 0x1234_5678] {
            let mut seed = seed_init;
            if let Some(picked) = pick_spawn_tile(&area, &mut seed, &blocked) {
                assert!(
                    !blocked.contains(&picked),
                    "seed {seed_init} picked a blocked tile {picked:?}",
                );
                assert!(
                    picked.x >= 0 && picked.x <= 3 && picked.y >= 0 && picked.y <= 3,
                    "seed {seed_init} picked out-of-area tile {picked:?}",
                );
            }
        }
    }

    #[test]
    fn pick_spawn_tile_from_explicit_list() {
        let area = SpawnArea {
            bounds: None,
            tiles: Some(vec![
                TileCoordinate { x: 3, y: 4, z: 0 },
                TileCoordinate { x: 7, y: 8, z: 0 },
            ]),
        };
        let mut seed: u64 = 1;
        let picked = pick_spawn_tile(&area, &mut seed, &[]).unwrap();
        assert!(
            picked == TilePosition::ground(3, 4) || picked == TilePosition::ground(7, 8),
            "picked tile {picked:?} not in the list",
        );
    }

    #[test]
    fn reconcile_slot_count_pads_when_under() {
        let mut runtime = SpawnGroupRuntime::new(small_def(4, 30.0), 1);
        runtime.pending_respawns.clear();
        runtime.members.clear();
        reconcile_slot_count(&mut runtime);
        assert_eq!(runtime.pending_respawns.len(), 4);
    }

    #[test]
    fn reconcile_slot_count_truncates_when_over() {
        let mut runtime = SpawnGroupRuntime::new(small_def(2, 30.0), 1);
        runtime.pending_respawns = vec![1.0, 2.0, 3.0, 4.0];
        runtime.members.clear();
        reconcile_slot_count(&mut runtime);
        assert_eq!(runtime.pending_respawns.len(), 2);
    }

    #[test]
    fn dump_round_trip_keeps_state() {
        let dump = SpawnGroupRuntimeDump {
            space_id: SpaceId(7),
            group_id: "cellar_rats".to_owned(),
            pending_respawns: vec![1.5, 12.0],
            rng_seed: 0xABCD_1234,
        };
        let json = serde_json::to_string(&dump).unwrap();
        let back: SpawnGroupRuntimeDump = serde_json::from_str(&json).unwrap();
        assert_eq!(back.space_id, dump.space_id);
        assert_eq!(back.group_id, dump.group_id);
        assert_eq!(back.pending_respawns, dump.pending_respawns);
        assert_eq!(back.rng_seed, dump.rng_seed);
    }
}
