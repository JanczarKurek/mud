use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::app::AppExit;
use bevy::ecs::message::MessageReader;
use bevy::log::{debug, error, info, warn};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::components::{AttackProfile, CombatLeash, CombatTarget};
use crate::magic::effects::MagicEffects;
use crate::network::resources::TcpServerState;
use crate::npc::components::{
    AiMemory, AiState, HostileBehavior, Npc, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
    SpawnGroupMember,
};
use crate::npc::spawn_groups::{PendingSpawnGroupDumps, SpawnGroupRegistry, SpawnGroupRuntimeDump};
use crate::player::components::{
    BaseStats, ChatLog, DerivedStats, Inventory, InventoryStack, MovementCooldown, Player,
    PlayerId, PlayerIdentity, VitalStats,
};
use crate::world::components::{
    Collider, Container, Movable, ObjectState, OverworldObject, Rotatable, SpaceId, SpaceResident,
    Storable, TilePosition, ViewPosition,
};
use crate::world::floor_definitions::FloorTypeId;
use crate::world::floor_map::{FloorMap, FloorMaps};
use crate::world::lighting::WorldClock;
use crate::world::map_layout::ObjectProperties;
use crate::world::map_layout::{SpaceDefinitions, SpacePermanence};
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::{RuntimeSpace, SpaceManager};
use crate::world::setup::initialize_runtime_spaces;
use crate::world::ttl::Ttl;
use crate::world::WorldConfig;

pub struct PersistenceServerPlugin {
    pub save_path: PathBuf,
}

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PersistenceStartupSet {
    LoadSnapshot,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct WorldSnapshotStatus {
    pub loaded: bool,
    /// True when the snapshot had ≥1 player entries — used by
    /// `spawn_embedded_player_authoritative` to avoid spawning a duplicate
    /// when the snapshot was empty (e.g. server saved after all clients left).
    pub players_restored: bool,
}

#[derive(Resource, Clone, Debug)]
pub struct WorldSaveConfig {
    pub path: PathBuf,
}

impl Plugin for PersistenceServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WorldSaveConfig {
            path: self.save_path.clone(),
        })
        .insert_resource(WorldSnapshotStatus::default())
        .add_systems(
            Startup,
            load_world_from_snapshot
                .in_set(PersistenceStartupSet::LoadSnapshot)
                .before(initialize_runtime_spaces),
        )
        .add_systems(Last, save_world_on_app_exit);
    }
}

/// On-disk world snapshot. The persisted `object_id`s on each
/// `WorldObjectStateDump` are *save-local* — they're used only to resolve
/// cross-references within the file (e.g. `combat_target_object_id`). On load,
/// every world object is given a fresh runtime id; persisted ids never leak
/// into the runtime registry.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldStateDump {
    pub format_version: u32,
    pub saved_at_unix_seconds: u64,
    pub world_config: WorldConfigDump,
    #[serde(default)]
    pub map_layout: Option<MapLayoutDump>,
    #[serde(default)]
    pub spaces: Vec<RuntimeSpaceDump>,
    pub network: NetworkStateDump,
    pub world_objects: Vec<WorldObjectStateDump>,
    #[serde(default)]
    pub floor_maps: Vec<FloorMapDump>,
    /// Per-spawn-group runtime state (cooldowns, RNG seed). Members are not
    /// listed here directly — surviving NPCs carry a `SpawnGroupMember`
    /// component which `bootstrap_spawn_groups` reads to rebuild membership.
    #[serde(default)]
    pub spawn_groups: Vec<SpawnGroupRuntimeDump>,
    /// Persisted in-game world clock in `[0, 1)`. Defaults to noon (0.5)
    /// for snapshots written before this field existed, matching the
    /// previous boot-time value.
    #[serde(default = "default_world_time")]
    pub world_time: f32,
}

fn default_world_time() -> f32 {
    0.5
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldConfigDump {
    #[serde(default)]
    pub current_space_id: Option<crate::world::components::SpaceId>,
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
    #[serde(default)]
    pub fill_floor_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapLayoutDump {
    pub width: i32,
    pub height: i32,
    pub fill_floor_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FloorMapDump {
    pub space_id: SpaceId,
    pub z: i32,
    pub width: i32,
    pub height: i32,
    pub tiles: Vec<Option<FloorTypeId>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetworkStateDump {
    pub next_connection_id: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeSpaceDump {
    pub id: crate::world::components::SpaceId,
    pub authored_id: String,
    pub width: i32,
    pub height: i32,
    pub fill_floor_type: String,
    pub permanence: SpacePermanence,
    pub instance_owner: Option<PortalInstanceKeyDump>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PortalInstanceKeyDump {
    pub source_space_id: crate::world::components::SpaceId,
    pub portal_id: String,
}

/// Persisted player state. Runtime `object_id`s are deliberately *not* stored:
/// they're allocated fresh on every load. Cross-references that used to live
/// in this dump (e.g. a remembered combat target) would be invalid after the
/// id remap, so they aren't persisted either.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlayerStateDump {
    pub player_id: PlayerId,
    #[serde(default)]
    pub space_id: Option<crate::world::components::SpaceId>,
    pub tile_position: TilePosition,
    pub inventory: Inventory,
    pub chat_log: ChatLog,
    pub base_stats: BaseStats,
    pub derived_stats: DerivedStats,
    pub vital_stats: VitalStats,
    pub movement_cooldown: MovementCooldown,
    pub attack_profile: AttackProfile,
    pub combat_leash: CombatLeash,
    /// Per-character Yarn variable store, serialized as a flat key→value map.
    /// Populated by the save path by snapshotting `CharacterVarStores`; restored
    /// at login before any `DialogueRunner` is built. `#[serde(default)]` so
    /// saves written before this field existed still deserialize.
    #[serde(default)]
    pub yarn_vars:
        std::collections::HashMap<String, crate::dialog::variable_storage::YarnValueDump>,
    /// Direction the character was facing when saved. `#[serde(default)]` so
    /// character rows written before this field existed default to South.
    #[serde(default)]
    pub facing: crate::world::direction::Direction,
    /// Where this character respawns after death. `None` for characters that
    /// haven't run `SetHome` yet — the death handler falls back to the map
    /// center in that case. `#[serde(default)]` for back-compat with saves
    /// written before this field existed.
    #[serde(default)]
    pub home_position: Option<(crate::world::components::SpaceId, TilePosition)>,
    /// XP / level state. `#[serde(default)]` so rows written before progression
    /// existed default to a fresh level-1 character.
    #[serde(default)]
    pub experience: crate::player::progression::Experience,
    /// Selected class. Set at character creation via the Character Create
    /// screen. `#[serde(default)]` (Fighter) so any legacy row without a
    /// class field silently migrates to Fighter on load.
    #[serde(default)]
    pub class: crate::player::classes::Class,
    /// Active timed magical effects (buffs / debuffs) on the player.
    /// `#[serde(default)]` so rows written before this field existed default
    /// to no active effects.
    #[serde(default)]
    pub magic_effects: MagicEffects,
    /// Generic per-character JSON key/value store. Holds learned recipes,
    /// quest state snapshots, and anything else subsystems want to persist
    /// without growing this struct. See `crate::crafting::stash`.
    #[serde(default)]
    pub stash: std::collections::HashMap<String, serde_json::Value>,
    /// Per-character skill ranks and unspent points. `#[serde(default)]` so
    /// rows written before the skill system existed default to an empty
    /// sheet (no ranks, no banked points).
    #[serde(default)]
    pub skill_sheet: crate::player::skills::SkillSheet,
    /// Per-character sprite recolor selection (hair / torso / trousers).
    /// `#[serde(default)]` so rows written before character customization
    /// existed fall back to the `PlayerAppearance::default()` palette.
    #[serde(default)]
    pub appearance: crate::player::components::PlayerAppearance,
    /// Tiles the player has revealed, grouped by space and stored as sorted
    /// `(x, y, z)` triples for stable JSON ordering. Rehydrated into the
    /// player's `DiscoveredTiles` component on load and consumed by the
    /// fog-of-war overlay. `#[serde(default)]` so rows written before this
    /// field existed default to "nothing discovered".
    #[serde(default)]
    pub discovered_tiles:
        std::collections::HashMap<crate::world::components::SpaceId, Vec<(i32, i32, i32)>>,
}

/// Build a `PlayerStateDump` from the components of a single player entity.
/// Shared between world snapshot writes and per-account DB saves so both paths
/// serialize the same fields.
#[allow(clippy::too_many_arguments)]
pub fn build_player_state_dump(
    identity: &PlayerIdentity,
    space_resident: &SpaceResident,
    tile_position: &TilePosition,
    inventory: &Inventory,
    chat_log: &ChatLog,
    base_stats: &BaseStats,
    derived_stats: &DerivedStats,
    vital_stats: &VitalStats,
    movement_cooldown: &MovementCooldown,
    attack_profile: &AttackProfile,
    combat_leash: &CombatLeash,
    facing: crate::world::direction::Direction,
    experience: crate::player::progression::Experience,
    class: crate::player::classes::Class,
    magic_effects: &MagicEffects,
    stash: &crate::crafting::CharacterStash,
    skill_sheet: &crate::player::skills::SkillSheet,
    appearance: crate::player::components::PlayerAppearance,
    discovered_tiles: &crate::player::components::DiscoveredTiles,
) -> PlayerStateDump {
    let mut discovered: std::collections::HashMap<
        crate::world::components::SpaceId,
        Vec<(i32, i32, i32)>,
    > = std::collections::HashMap::new();
    for (space_id, set) in &discovered_tiles.by_space {
        let mut tiles: Vec<(i32, i32, i32)> = set.iter().copied().collect();
        tiles.sort_unstable();
        discovered.insert(*space_id, tiles);
    }
    PlayerStateDump {
        player_id: identity.id,
        space_id: Some(space_resident.space_id),
        tile_position: *tile_position,
        inventory: inventory.clone(),
        chat_log: chat_log.clone(),
        base_stats: base_stats.clone(),
        derived_stats: derived_stats.clone(),
        vital_stats: vital_stats.clone(),
        movement_cooldown: movement_cooldown.clone(),
        attack_profile: *attack_profile,
        combat_leash: *combat_leash,
        yarn_vars: std::collections::HashMap::new(),
        facing,
        home_position: identity.home_position,
        experience,
        class,
        magic_effects: magic_effects.clone(),
        stash: stash.entries.clone(),
        skill_sheet: skill_sheet.clone(),
        appearance,
        discovered_tiles: discovered,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldObjectStateDump {
    /// Save-local identifier. Used only to resolve cross-references *within*
    /// this snapshot (e.g. `combat_target_object_id`). Discarded on load —
    /// each entity is allocated a fresh runtime id.
    pub object_id: u64,
    pub definition_id: String,
    #[serde(default)]
    pub properties: ObjectProperties,
    #[serde(default)]
    pub space_id: Option<crate::world::components::SpaceId>,
    pub tile_position: Option<TilePosition>,
    pub collider: bool,
    pub movable: bool,
    #[serde(default)]
    pub rotatable: bool,
    pub storable: bool,
    pub container_slots: Option<Vec<Option<InventoryStack>>>,
    pub npc: Option<NpcStateDump>,
    #[serde(default)]
    pub quantity: Option<u32>,
    /// Remaining seconds on the entity's `Ttl` component (corpses, spell-
    /// summoned objects, ...). `None` for non-transient objects.
    #[serde(default)]
    pub remaining_ttl: Option<f32>,
    /// `#[serde(default)]` so snapshots written before this field existed
    /// default to South on load.
    #[serde(default)]
    pub facing: Option<crate::world::direction::Direction>,
    /// Players who have spotted this hidden object, persisted across restarts
    /// so a re-logged player keeps their detection state. Empty (or absent for
    /// v10 saves) means nobody has spotted it yet — detection re-rolls from
    /// scratch on next encounter. Only meaningful when `hidden_dc` is set; the
    /// runtime `Hidden` component is only attached then.
    #[serde(default)]
    pub hidden_detected_by: Vec<PlayerId>,
    /// Runtime Perception DC carried by this object's `Hidden` component.
    /// `Some(dc)` ⇒ the object was hidden at save time (either authored as
    /// `hidden_dc` in the map YAML, or hidden at runtime via the player Hide
    /// action). `None` ⇒ the object was visible. Older snapshots without this
    /// field default to `None` via `serde(default)`.
    #[serde(default)]
    pub hidden_dc: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NpcStateDump {
    pub base_stats: Option<BaseStats>,
    pub derived_stats: Option<DerivedStats>,
    pub vital_stats: Option<VitalStats>,
    pub attack_profile: Option<AttackProfile>,
    pub combat_leash: Option<CombatLeash>,
    pub combat_target_object_id: Option<u64>,
    pub roaming_behavior: Option<RoamingBehavior>,
    pub hostile_behavior: Option<HostileBehavior>,
    pub roaming_step_timer: Option<RoamingStepTimer>,
    pub roaming_random_state: Option<RoamingRandomState>,
    /// Set when this NPC was instantiated by a `SpawnGroup`. Restored on load
    /// so the spawn group can resume tracking it against `max_count`.
    #[serde(default)]
    pub spawn_group: Option<SpawnGroupMember>,
    /// Active timed magical effects (Slow, Sleep, ...) on this NPC.
    /// `#[serde(default)]` for back-compat with rows written before this
    /// field existed.
    #[serde(default)]
    pub magic_effects: MagicEffects,
}

#[allow(clippy::too_many_arguments)]
fn save_world_on_app_exit(
    mut app_exit_reader: MessageReader<AppExit>,
    app_state: Option<Res<State<crate::app::state::ClientAppState>>>,
    save_config: Res<WorldSaveConfig>,
    world_config: Res<WorldConfig>,
    space_manager: Res<SpaceManager>,
    floor_maps: Res<FloorMaps>,
    object_registry: Res<ObjectRegistry>,
    tcp_server_state: Option<Res<TcpServerState>>,
    world_clock: Res<WorldClock>,
    world_object_query: Query<
        (
            Entity,
            &OverworldObject,
            &SpaceResident,
            Option<&TilePosition>,
            Has<Collider>,
            Has<Movable>,
            Has<Rotatable>,
            Has<Storable>,
            Option<&Container>,
            Has<Npc>,
            Option<&CombatTarget>,
            Option<&crate::world::components::Quantity>,
            Option<&Ttl>,
            Option<&crate::world::components::Facing>,
            Option<&crate::world::hidden::Hidden>,
        ),
        Without<Player>,
    >,
    npc_query: Query<
        (
            Option<&BaseStats>,
            Option<&DerivedStats>,
            Option<&VitalStats>,
            Option<&AttackProfile>,
            Option<&CombatLeash>,
            Option<&RoamingBehavior>,
            Option<&HostileBehavior>,
            Option<&RoamingStepTimer>,
            Option<&RoamingRandomState>,
            Option<&SpawnGroupMember>,
            Option<&MagicEffects>,
        ),
        With<Npc>,
    >,
    spawn_group_registry: Option<Res<SpawnGroupRegistry>>,
) {
    if app_exit_reader.read().next().is_none() {
        return;
    }

    if app_state.is_some_and(|s| *s == crate::app::state::ClientAppState::MapEditor) {
        return;
    }

    let mut entity_to_object_id = std::collections::HashMap::new();
    for (entity, object, _, _, _, _, _, _, _, _, _, _, _, _, _) in world_object_query.iter() {
        entity_to_object_id.insert(entity, object.object_id);
    }

    let mut spaces = space_manager
        .spaces
        .values()
        .map(|space| RuntimeSpaceDump {
            id: space.id,
            authored_id: space.authored_id.clone(),
            width: space.width,
            height: space.height,
            fill_floor_type: space.fill_floor_type.clone(),
            permanence: space.permanence,
            instance_owner: space.instance_owner.as_ref().map(|instance_owner| {
                PortalInstanceKeyDump {
                    source_space_id: instance_owner.source_space_id,
                    portal_id: instance_owner.portal_id.clone(),
                }
            }),
        })
        .collect::<Vec<_>>();
    spaces.sort_by_key(|space| space.id.0);

    let mut floor_map_dumps: Vec<FloorMapDump> = floor_maps
        .iter()
        .map(|(space_id, z, map)| FloorMapDump {
            space_id,
            z,
            width: map.width,
            height: map.height,
            tiles: map.tiles.clone(),
        })
        .collect();
    floor_map_dumps.sort_by_key(|dump| (dump.space_id.0, dump.z));

    let mut world_objects = world_object_query
        .iter()
        .map(
            |(
                entity,
                object,
                space_resident,
                tile_position,
                collider,
                movable,
                rotatable,
                storable,
                container,
                is_npc,
                combat_target,
                quantity,
                ttl,
                facing,
                hidden,
            )| WorldObjectStateDump {
                object_id: object.object_id,
                definition_id: object.definition_id.clone(),
                properties: object_registry
                    .properties(object.object_id)
                    .cloned()
                    .unwrap_or_default(),
                space_id: Some(space_resident.space_id),
                tile_position: tile_position.copied(),
                collider,
                movable,
                rotatable,
                storable,
                container_slots: container.map(|container| container.slots.clone()),
                quantity: quantity.map(|q| q.0).filter(|&q| q > 1),
                remaining_ttl: ttl.map(|t| t.remaining_seconds),
                facing: facing.map(|f| f.0),
                hidden_detected_by: hidden
                    .map(|h| {
                        let mut ids: Vec<PlayerId> = h.detected_by.iter().copied().collect();
                        ids.sort_by_key(|id| id.0);
                        ids
                    })
                    .unwrap_or_default(),
                hidden_dc: hidden.map(|h| h.dc),
                npc: is_npc.then(|| {
                    let (
                        base_stats,
                        derived_stats,
                        vital_stats,
                        attack_profile,
                        combat_leash,
                        roaming_behavior,
                        hostile_behavior,
                        roaming_step_timer,
                        roaming_random_state,
                        spawn_group_member,
                        magic_effects,
                    ) = npc_query.get(entity).unwrap_or_default();

                    NpcStateDump {
                        base_stats: base_stats.copied(),
                        derived_stats: derived_stats.copied(),
                        vital_stats: vital_stats.copied(),
                        attack_profile: attack_profile.copied(),
                        combat_leash: combat_leash.copied(),
                        combat_target_object_id: combat_target
                            .and_then(|target| entity_to_object_id.get(&target.entity).copied()),
                        roaming_behavior: roaming_behavior.copied(),
                        hostile_behavior: hostile_behavior.copied(),
                        roaming_step_timer: roaming_step_timer.copied(),
                        roaming_random_state: roaming_random_state.copied(),
                        spawn_group: spawn_group_member.cloned(),
                        magic_effects: magic_effects.cloned().unwrap_or_default(),
                    }
                }),
            },
        )
        .collect::<Vec<_>>();
    world_objects.sort_by_key(|object| object.object_id);

    let mut spawn_group_dumps: Vec<SpawnGroupRuntimeDump> = spawn_group_registry
        .map(|registry| {
            registry
                .groups
                .iter()
                .map(|(key, runtime)| SpawnGroupRuntimeDump {
                    space_id: key.space_id,
                    group_id: key.group_id.clone(),
                    pending_respawns: runtime.pending_respawns.clone(),
                    rng_seed: runtime.rng_seed,
                })
                .collect()
        })
        .unwrap_or_default();
    spawn_group_dumps.sort_by(|a, b| {
        a.space_id
            .0
            .cmp(&b.space_id.0)
            .then_with(|| a.group_id.cmp(&b.group_id))
    });

    let dump = WorldStateDump {
        format_version: 12,
        spaces,
        saved_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        world_config: WorldConfigDump {
            current_space_id: Some(world_config.current_space_id),
            map_width: world_config.map_width,
            map_height: world_config.map_height,
            tile_size: world_config.tile_size,
            fill_floor_type: Some(world_config.fill_floor_type.clone()),
        },
        map_layout: Some(MapLayoutDump {
            width: world_config.map_width,
            height: world_config.map_height,
            fill_floor_type: world_config.fill_floor_type.clone(),
        }),
        network: NetworkStateDump {
            next_connection_id: tcp_server_state
                .as_ref()
                .map(|state| state.next_connection_id)
                .unwrap_or_default(),
        },
        world_objects,
        floor_maps: floor_map_dumps,
        spawn_groups: spawn_group_dumps,
        world_time: world_clock.time_of_day,
    };

    if let Err(error) = write_world_dump(&save_config.path, &dump) {
        error!(
            "failed to save world state to {}: {error}",
            save_config.path.display()
        );
        return;
    }

    info!("saved world state to {}", save_config.path.display());
}

fn load_world_from_snapshot(
    mut commands: Commands,
    save_config: Res<WorldSaveConfig>,
    mut snapshot_status: ResMut<WorldSnapshotStatus>,
    authored_spaces: Res<SpaceDefinitions>,
    mut world_config: ResMut<WorldConfig>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut space_manager: ResMut<SpaceManager>,
    mut floor_maps: ResMut<FloorMaps>,
    mut tcp_server_state: Option<ResMut<TcpServerState>>,
    object_definitions: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
    mut pending_spawn_groups: ResMut<PendingSpawnGroupDumps>,
    mut world_clock: ResMut<WorldClock>,
) {
    let dump = match read_world_dump(&save_config.path) {
        Ok(dump) => dump,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            info!(
                "no world snapshot found at {}; starting fresh world",
                save_config.path.display()
            );
            return;
        }
        Err(error) => {
            warn!(
                "failed to load world snapshot from {}: {error}",
                save_config.path.display()
            );
            return;
        }
    };

    if dump.format_version < 10 {
        warn!(
            "world snapshot at {} is format_version {} (<10); spawn-group state was not persisted before v10 — discarding stale snapshot and starting fresh world",
            save_config.path.display(),
            dump.format_version
        );
        return;
    }

    let WorldStateDump {
        world_config: dump_world_config,
        map_layout: dump_map_layout,
        spaces: dump_spaces,
        network: dump_network,
        world_objects,
        floor_maps: dump_floor_maps,
        spawn_groups: dump_spawn_groups,
        world_time: dump_world_time,
        ..
    } = dump;

    world_clock.time_of_day = dump_world_time.rem_euclid(1.0);
    world_clock.seconds_since_emit = 0.0;

    let legacy_fill_floor_type = dump_map_layout
        .as_ref()
        .map(|layout| layout.fill_floor_type.clone())
        .or(dump_world_config.fill_floor_type.clone())
        .unwrap_or_else(|| "grass".to_owned());

    if dump_spaces.is_empty() {
        let bootstrap_definition = authored_spaces
            .bootstrap_space()
            .expect("bootstrap space required when loading world save");
        let space_id = space_manager.allocate_space_id();
        space_manager.insert_space(RuntimeSpace {
            id: space_id,
            authored_id: bootstrap_definition.authored_id.clone(),
            width: dump_world_config.map_width,
            height: dump_world_config.map_height,
            fill_floor_type: legacy_fill_floor_type.clone(),
            permanence: SpacePermanence::Persistent,
            instance_owner: None,
            lighting: bootstrap_definition.lighting.clone(),
        });
    } else {
        let max_space_id = dump_spaces
            .iter()
            .map(|space| space.id.0)
            .max()
            .unwrap_or(0);
        space_manager.next_space_id = max_space_id + 1;
        for dump_space in dump_spaces {
            // Lighting is not persisted; pull the latest authored value from
            // the matching space definition (or fall back to defaults). This
            // means edits to a space's lighting block take effect on next load.
            let lighting = authored_spaces
                .get(&dump_space.authored_id)
                .map(|def| def.lighting.clone())
                .unwrap_or_default();
            space_manager.insert_space(RuntimeSpace {
                id: dump_space.id,
                authored_id: dump_space.authored_id,
                width: dump_space.width,
                height: dump_space.height,
                fill_floor_type: dump_space.fill_floor_type,
                permanence: dump_space.permanence,
                instance_owner: dump_space.instance_owner.map(|instance_owner| {
                    crate::world::resources::PortalInstanceKey {
                        source_space_id: instance_owner.source_space_id,
                        portal_id: instance_owner.portal_id,
                    }
                }),
                lighting,
            });
        }
    }

    // Restore floor maps. Empty / legacy: rebuild from the authored space defs.
    if dump_floor_maps.is_empty() {
        for runtime_space in space_manager.spaces.values() {
            if let Some(definition) = authored_spaces.get(&runtime_space.authored_id) {
                floor_maps.insert(
                    runtime_space.id,
                    TilePosition::GROUND_FLOOR,
                    definition.build_floor_map(TilePosition::GROUND_FLOOR),
                );
            }
        }
    } else {
        for dump_floor in dump_floor_maps {
            floor_maps.insert(
                dump_floor.space_id,
                dump_floor.z,
                FloorMap {
                    width: dump_floor.width,
                    height: dump_floor.height,
                    tiles: dump_floor.tiles,
                },
            );
        }
    }

    let current_space_id = dump_world_config
        .current_space_id
        .or_else(|| {
            space_manager
                .spaces
                .keys()
                .copied()
                .min_by_key(|space_id| space_id.0)
        })
        .unwrap_or(crate::world::components::SpaceId(0));
    let current_space = space_manager.get(current_space_id).cloned();

    world_config.current_space_id = current_space_id;
    world_config.map_width = current_space
        .as_ref()
        .map(|space| space.width)
        .unwrap_or(dump_world_config.map_width);
    world_config.map_height = current_space
        .as_ref()
        .map(|space| space.height)
        .unwrap_or(dump_world_config.map_height);
    world_config.tile_size = dump_world_config.tile_size;
    world_config.fill_floor_type = current_space
        .as_ref()
        .map(|space| space.fill_floor_type.clone())
        .unwrap_or(legacy_fill_floor_type);
    // Reset the registry: from_space_definitions filled it with authored ids
    // that are about to be overwritten by snapshot entities anyway. We also
    // intentionally drop any persisted registry contents — runtime ids are
    // opaque and reallocated on every load.
    *object_registry = ObjectRegistry::default();
    if let Some(server_state) = tcp_server_state.as_mut() {
        server_state.next_connection_id = dump_network.next_connection_id;
    }

    // Maps save-local id (as written in the snapshot) → spawned entity. Used
    // to resolve persisted cross-references (e.g. NPC combat targets) onto the
    // freshly allocated runtime entities.
    let mut object_entities = std::collections::HashMap::new();
    let mut pending_combat_targets = Vec::new();

    // Players no longer ride in the world snapshot — they live per-account in
    // the accounts DB now. On load, no player entities are spawned here; the
    // auth path (or embedded mode's DB restore) creates them.
    let players_restored = false;

    for object in world_objects {
        let space_id = object.space_id.unwrap_or(current_space_id);
        let definition_id_for_lookup = object.definition_id.clone();
        let resolved_facing = object.facing.unwrap_or_else(|| {
            object_definitions
                .get(&definition_id_for_lookup)
                .and_then(|def| def.render.default_facing)
                .unwrap_or_default()
        });
        let runtime_id = object_registry.allocate_runtime_id_with_properties(
            object.definition_id.clone(),
            object.properties.clone(),
        );
        let save_local_id = object.object_id;
        let mut entity = commands.spawn((
            OverworldObject {
                object_id: runtime_id,
                definition_id: object.definition_id,
            },
            SpaceResident { space_id },
            crate::world::components::Facing(resolved_facing),
        ));

        if let Some(tile_position) = object.tile_position {
            entity.insert(tile_position);
            entity.insert(ViewPosition {
                space_id,
                tile: tile_position,
            });
        }
        if object.collider {
            entity.insert(Collider);
        }
        if object.movable {
            entity.insert(Movable);
        }
        if object.rotatable {
            entity.insert(Rotatable);
        }
        if object.storable {
            entity.insert(Storable);
        }
        if let Some(container_slots) = object.container_slots {
            entity.insert(Container {
                slots: container_slots,
            });
        }
        if let Some(q) = object.quantity {
            if q > 1 {
                entity.insert(crate::world::components::Quantity(q));
            }
        }
        if let Some(remaining) = object.remaining_ttl {
            if remaining > 0.0 {
                entity.insert(Ttl {
                    remaining_seconds: remaining,
                });
            }
        }
        // Per-instance `dialog_id` property overrides the template's
        // `dialog_node` (set by the editor). Falls back to the template
        // default when no per-instance override is set.
        let resolved_dialog = object
            .properties
            .get("dialog_id")
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| {
                object_definitions
                    .get(&definition_id_for_lookup)
                    .and_then(|def| def.dialog_node.clone())
            });
        if let Some(dialog_node) = resolved_dialog {
            entity.insert(crate::dialog::components::DialogNode(dialog_node));
        }
        // Restore stateful-object state from the persisted properties bag,
        // falling back to the definition's `initial_state` when the bag has
        // no `state` key (legacy saves predate the states feature).
        if let Some(state_value) = object.properties.get("state").cloned().or_else(|| {
            object_definitions
                .get(&definition_id_for_lookup)
                .and_then(|def| def.initial_state.clone())
        }) {
            entity.insert(ObjectState(state_value));
        }
        if let Some(npc) = object.npc {
            // AiState / AiMemory are not persisted (they hold transient FSM
            // state plus an Entity target that can't round-trip through a
            // snapshot). The roaming tick query requires both, so loaded NPCs
            // need fresh defaults — otherwise they're silently filtered out
            // of `update_roaming_npcs` and stand still until killed and
            // respawned.
            entity.insert((Npc, AiState::default(), AiMemory::default()));
            if let Some(base_stats) = npc.base_stats {
                entity.insert(base_stats);
            }
            if let Some(derived_stats) = npc.derived_stats {
                entity.insert(derived_stats);
            }
            if let Some(vital_stats) = npc.vital_stats {
                entity.insert(vital_stats);
            }
            let (derived_profile, derived_damage) =
                crate::world::setup::attack_profile_for_definition(
                    object_definitions.get(&definition_id_for_lookup),
                );
            let resolved_profile = npc.attack_profile.unwrap_or(derived_profile);
            entity.insert((resolved_profile, derived_damage));
            if let Some(combat_leash) = npc.combat_leash {
                entity.insert(combat_leash);
            }
            if let Some(roaming_behavior) = npc.roaming_behavior {
                entity.insert(roaming_behavior);
            }
            if let Some(hostile_behavior) = npc.hostile_behavior {
                entity.insert(hostile_behavior);
            }
            if let Some(roaming_step_timer) = npc.roaming_step_timer {
                entity.insert(roaming_step_timer);
            }
            if let Some(roaming_random_state) = npc.roaming_random_state {
                entity.insert(roaming_random_state);
            }
            if let Some(spawn_group_member) = npc.spawn_group {
                entity.insert(spawn_group_member);
            }
            if !npc.magic_effects.is_empty() {
                entity.insert(npc.magic_effects);
            }
            if let Some(target_object_id) = npc.combat_target_object_id {
                pending_combat_targets.push((save_local_id, target_object_id));
            }
        }

        // Re-derive Shopkeeper + Stockpile from the object definition on load.
        // Stockpile state (decremented finite stock) is not persisted in the
        // snapshot — wares reset to their YAML values on reload. Mirrors the
        // fresh-spawn path in `world::setup::spawn_overworld_object`.
        if let Some(shopkeeper_def) = object_definitions
            .get(&definition_id_for_lookup)
            .and_then(|def| def.shopkeeper.as_ref())
        {
            entity.insert((
                crate::game::shop::Shopkeeper,
                crate::game::shop::Stockpile::from_def(shopkeeper_def),
            ));
        }

        // Re-derive `OnSteppedTriggers` from the definition on load. The
        // component itself is not persisted (triggers are pure config — the
        // accumulator phase is fine to reset). Without this, step triggers
        // silently no-op for every object after the first reload.
        //
        // Note: `HazardOwner` is also intentionally NOT persisted. Hazards
        // that carry it (firewall blazes, future player-armed traps) have
        // short TTLs, so on reload the few that survive lose their owner
        // and revert to `DamageSource::Environment` semantics (no XP credit
        // on kill). If long-lived owned hazards are ever added, add an
        // `owner_player_id: Option<u64>` to `WorldObjectStateDump`.
        if let Some(definition) = object_definitions.get(&definition_id_for_lookup) {
            if !definition.on_stepped.is_empty() {
                let triggers = crate::world::step_triggers::StepTrigger::from_def_list(
                    &definition.on_stepped,
                    &definition.name,
                );
                entity.insert(crate::world::step_triggers::OnSteppedTriggers(triggers));
            }
        }

        // Restore `Hidden` from the snapshot's per-instance dc + detected_by.
        // The DC may be either authored (via the map's `hidden_dc` property)
        // or runtime-chosen (via the player Hide action). Loading a v11
        // snapshot defaults `hidden_dc` to `None`, so any pre-existing hidden
        // state on player-placed objects is lost — by design, since the
        // pre-v12 model only persisted the static type-level DC.
        if let Some(dc) = object.hidden_dc {
            let mut hidden = crate::world::hidden::Hidden::new(dc);
            for pid in &object.hidden_detected_by {
                hidden.detected_by.insert(*pid);
            }
            entity.insert(hidden);
        }

        let entity_id = entity.id();
        object_entities.insert(save_local_id, entity_id);
    }

    for (source_save_local_id, target_save_local_id) in pending_combat_targets {
        let Some(&source_entity) = object_entities.get(&source_save_local_id) else {
            continue;
        };
        let Some(&target_entity) = object_entities.get(&target_save_local_id) else {
            continue;
        };
        commands.entity(source_entity).insert(CombatTarget {
            entity: target_entity,
        });
    }

    pending_spawn_groups.entries = dump_spawn_groups;

    snapshot_status.loaded = true;
    snapshot_status.players_restored = players_restored;
    info!(
        "loaded world state from {} ({} players restored)",
        save_config.path.display(),
        if players_restored { "some" } else { "no" }
    );
}

fn write_world_dump(path: &Path, dump: &WorldStateDump) -> std::io::Result<()> {
    info!(
        "writing world snapshot to {} ({} world objects, {} spaces)",
        path.display(),
        dump.world_objects.len(),
        dump.spaces.len()
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, dump)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn read_world_dump(path: &Path) -> std::io::Result<WorldStateDump> {
    debug!("reading world snapshot from {}", path.display());
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::CombatPlugin;
    use crate::game::GameServerPlugin;
    use crate::magic::MagicServerPlugin;
    use crate::network::resources::TcpServerState;
    use crate::npc::NpcPlugin;
    use crate::player::setup::spawn_player_authoritative;
    use crate::player::PlayerServerPlugin;
    use crate::quest::QuestPlugin;
    use crate::world::WorldServerPlugin;

    fn setup_server_app(save_path: &Path) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(TcpServerState::default());
        // CharacterVarStores normally comes from `DialogServerPlugin`, but that
        // plugin pulls in YarnSpinner which needs `AssetPlugin`. Quest systems
        // only require the resource to exist; inject the bare default here.
        app.init_resource::<crate::dialog::resources::CharacterVarStores>();
        app.add_plugins((
            GameServerPlugin,
            WorldServerPlugin,
            NpcPlugin,
            PlayerServerPlugin,
            CombatPlugin,
            MagicServerPlugin,
            QuestPlugin::default(),
            PersistenceServerPlugin {
                save_path: save_path.to_path_buf(),
            },
        ));
        app.update();
        app
    }

    #[test]
    fn writes_world_dump_on_app_exit() {
        let save_path =
            std::env::temp_dir().join(format!("mud2-world-dump-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&save_path);

        let mut app = setup_server_app(&save_path);
        let object_id = app
            .world_mut()
            .resource_mut::<ObjectRegistry>()
            .allocate_runtime_id("player");
        let spawn_tile = {
            let world_config = app.world().resource::<WorldConfig>();
            TilePosition::ground(world_config.map_width / 2, world_config.map_height / 2)
        };
        let world_config = WorldConfig {
            current_space_id: app.world().resource::<WorldConfig>().current_space_id,
            map_width: app.world().resource::<WorldConfig>().map_width,
            map_height: app.world().resource::<WorldConfig>().map_height,
            tile_size: app.world().resource::<WorldConfig>().tile_size,
            fill_floor_type: app
                .world()
                .resource::<WorldConfig>()
                .fill_floor_type
                .clone(),
        };
        spawn_player_authoritative(
            &mut app.world_mut().commands(),
            &world_config,
            PlayerId(42),
            object_id,
            spawn_tile,
            "tester".to_owned(),
        );
        app.world_mut().flush();

        app.world_mut().write_message(AppExit::Success);
        app.update();

        let dump =
            serde_json::from_str::<WorldStateDump>(&std::fs::read_to_string(&save_path).unwrap())
                .unwrap();

        assert_eq!(dump.format_version, 12);
        assert!(!dump.spaces.is_empty());
        // Players don't appear in the world snapshot at all (they live in the
        // accounts DB) and the object registry is no longer persisted, so the
        // only thing this test can check on the players' behalf is that the
        // snapshot wrote successfully and didn't dump player rows.
        let _ = world_config.current_space_id;

        let _ = std::fs::remove_file(save_path);
    }

    #[test]
    fn loads_world_dump_on_startup_when_snapshot_exists() {
        let save_path =
            std::env::temp_dir().join(format!("mud2-world-load-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&save_path);

        let dump = WorldStateDump {
            format_version: 10,
            saved_at_unix_seconds: 0,
            world_config: WorldConfigDump {
                current_space_id: Some(crate::world::components::SpaceId(7)),
                map_width: 32,
                map_height: 24,
                tile_size: 48.0,
                fill_floor_type: Some("grass".to_owned()),
            },
            map_layout: Some(MapLayoutDump {
                width: 32,
                height: 24,
                fill_floor_type: "grass".to_owned(),
            }),
            spaces: vec![RuntimeSpaceDump {
                id: crate::world::components::SpaceId(7),
                authored_id: "overworld".to_owned(),
                width: 32,
                height: 24,
                fill_floor_type: "grass".to_owned(),
                permanence: SpacePermanence::Persistent,
                instance_owner: None,
            }],
            network: NetworkStateDump {
                next_connection_id: 77,
            },
            world_objects: vec![WorldObjectStateDump {
                object_id: 43,
                definition_id: "barrel".to_owned(),
                properties: Default::default(),
                space_id: Some(crate::world::components::SpaceId(7)),
                tile_position: Some(TilePosition::ground(7, 6)),
                collider: false,
                movable: false,
                rotatable: false,
                storable: false,
                container_slots: Some(vec![None, None]),
                npc: None,
                quantity: None,
                remaining_ttl: None,
                facing: None,
                hidden_detected_by: Vec::new(),
                hidden_dc: None,
            }],
            floor_maps: vec![],
            spawn_groups: vec![],
            world_time: 0.25,
        };
        write_world_dump(&save_path, &dump).unwrap();

        let mut app = setup_server_app(&save_path);
        app.update();

        assert!(app.world().resource::<WorldSnapshotStatus>().loaded);
        assert_eq!(
            app.world().resource::<TcpServerState>().next_connection_id,
            77
        );
        assert!(app
            .world()
            .resource::<SpaceManager>()
            .get(crate::world::components::SpaceId(7))
            .is_some());

        // Players are no longer restored from the world snapshot; the DB path
        // handles that.
        let restored_players = {
            let world = app.world_mut();
            let mut player_query = world.query::<&PlayerIdentity>();
            player_query.iter(world).collect::<Vec<_>>()
        };
        assert!(
            !restored_players
                .iter()
                .any(|identity| identity.id == PlayerId(7)),
            "snapshot should no longer restore PlayerId(7)"
        );

        // The barrel was loaded — but its runtime object_id is freshly allocated,
        // not the save-local 43 from the dump.
        let has_restored_barrel = {
            let world = app.world_mut();
            let mut object_query =
                world.query_filtered::<(&OverworldObject, &TilePosition), Without<Player>>();
            object_query.iter(world).any(|(object, tile)| {
                object.definition_id == "barrel" && *tile == TilePosition::ground(7, 6)
            })
        };
        assert!(has_restored_barrel);

        let _ = std::fs::remove_file(save_path);
    }

    /// `PlayerStateDump` rows written before `home_position` was added must
    /// still deserialize. The `#[serde(default)]` attribute on the field is
    /// what guarantees this; the test fails fast if anyone removes it.
    #[test]
    fn player_state_dump_round_trips_without_home_position() {
        // Hand-rolled JSON with no `home_position` key — represents a row
        // saved by an older binary.
        // Build a fresh dump in code, then serialize *without* the new field
        // by stripping it from the JSON, to mimic an older save.
        let dump_with_home = PlayerStateDump {
            player_id: PlayerId(1),
            space_id: Some(crate::world::components::SpaceId(0)),
            tile_position: TilePosition::ground(5, 7),
            inventory: Inventory::default(),
            chat_log: ChatLog::default(),
            base_stats: BaseStats::default(),
            derived_stats: DerivedStats::default(),
            vital_stats: VitalStats::full(100.0, 50.0),
            movement_cooldown: MovementCooldown::default(),
            attack_profile: AttackProfile::melee(),
            combat_leash: CombatLeash {
                max_distance_tiles: 6,
            },
            yarn_vars: Default::default(),
            facing: Default::default(),
            home_position: None,
            experience: Default::default(),
            class: Default::default(),
            magic_effects: Default::default(),
            stash: Default::default(),
            skill_sheet: Default::default(),
            appearance: Default::default(),
            discovered_tiles: Default::default(),
        };
        let json = serde_json::to_string(&dump_with_home).unwrap();
        // Confirm we didn't accidentally serialize Some(...).
        assert!(json.contains("\"home_position\":null"));
        // Strip the field entirely to simulate an older binary's output.
        let legacy_json = json
            .replace(",\"home_position\":null", "")
            .replace("\"home_position\":null,", "");
        assert!(!legacy_json.contains("home_position"));

        let dump: PlayerStateDump =
            serde_json::from_str(&legacy_json).expect("legacy save must deserialize");
        assert!(
            dump.home_position.is_none(),
            "legacy save should default to no home"
        );

        // Round-trip: serialize then deserialize and assert home stays None.
        let re_json = serde_json::to_string(&dump).unwrap();
        let re_dump: PlayerStateDump = serde_json::from_str(&re_json).unwrap();
        assert!(re_dump.home_position.is_none());
    }

    /// `MagicEffects` and the generic `Ttl` round-trip through serde, and
    /// legacy rows missing `magic_effects` still deserialize with defaults.
    #[test]
    fn magic_state_round_trips_and_defaults_on_legacy_rows() {
        use crate::magic::effects::{ActiveEffect, MagicEffects};
        use crate::magic::resources::EffectKind;

        // Player dump: a Glimmer buff with a few seconds remaining.
        let mut effects = MagicEffects::default();
        effects.active.push(ActiveEffect {
            kind: EffectKind::Glimmer,
            magnitude: 4.0,
            remaining_seconds: 123.0,
            secondary_magnitude: None,
            caster: None,
        });
        let dump = PlayerStateDump {
            player_id: PlayerId(1),
            space_id: Some(crate::world::components::SpaceId(0)),
            tile_position: TilePosition::ground(0, 0),
            inventory: Inventory::default(),
            chat_log: ChatLog::default(),
            base_stats: BaseStats::default(),
            derived_stats: DerivedStats::default(),
            vital_stats: VitalStats::full(10.0, 5.0),
            movement_cooldown: MovementCooldown::default(),
            attack_profile: AttackProfile::melee(),
            combat_leash: CombatLeash {
                max_distance_tiles: 6,
            },
            yarn_vars: Default::default(),
            facing: Default::default(),
            home_position: None,
            experience: Default::default(),
            class: Default::default(),
            magic_effects: effects.clone(),
            skill_sheet: Default::default(),
            stash: Default::default(),
            appearance: Default::default(),
            discovered_tiles: Default::default(),
        };
        let json = serde_json::to_string(&dump).unwrap();
        let restored: PlayerStateDump = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.magic_effects, effects);

        // Legacy player rows without `magic_effects` deserialize to empty.
        // Strip the field from the JSON object regardless of how its inner
        // shape evolves (new ActiveEffect fields can extend the substring).
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("magic_effects")
            .expect("dump should serialize magic_effects");
        let legacy_json = serde_json::to_string(&value).unwrap();
        assert!(!legacy_json.contains("magic_effects"));
        let legacy: PlayerStateDump = serde_json::from_str(&legacy_json).unwrap();
        assert!(legacy.magic_effects.is_empty());

        // Legacy player rows without `appearance` deserialize to the default
        // palette — same back-compat path as the magic_effects case above.
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("appearance")
            .expect("dump should serialize appearance");
        let legacy_json = serde_json::to_string(&value).unwrap();
        assert!(!legacy_json.contains("\"appearance\""));
        let legacy: PlayerStateDump = serde_json::from_str(&legacy_json).unwrap();
        assert_eq!(
            legacy.appearance,
            crate::player::components::PlayerAppearance::default()
        );

        // World object dump: a spell-summoned lantern with a remaining TTL
        // — same `remaining_ttl` field that corpses use.
        let lantern = WorldObjectStateDump {
            object_id: 1,
            definition_id: "magic_light".to_owned(),
            properties: Default::default(),
            space_id: Some(crate::world::components::SpaceId(0)),
            tile_position: Some(TilePosition::ground(1, 1)),
            collider: false,
            movable: false,
            rotatable: false,
            storable: false,
            container_slots: None,
            npc: None,
            quantity: None,
            remaining_ttl: Some(600.0),
            facing: None,
            hidden_detected_by: Vec::new(),
            hidden_dc: None,
        };
        let json = serde_json::to_string(&lantern).unwrap();
        let restored: WorldObjectStateDump = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.remaining_ttl, Some(600.0));
    }

    /// Loaded NPCs must have `AiState` and `AiMemory` attached. The roaming
    /// tick query requires both as non-optional components, so an NPC missing
    /// them is silently skipped — it stands still until something kills it
    /// and a respawn creates a fresh entity. Regression for the gap between
    /// the fresh-spawn path in `world::setup` and the snapshot load path here.
    #[test]
    fn loaded_npc_has_ai_state_and_memory_components() {
        use crate::npc::components::{AiMemory, AiState};

        let save_path =
            std::env::temp_dir().join(format!("mud2-world-npc-load-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&save_path);

        let dump = WorldStateDump {
            format_version: 10,
            saved_at_unix_seconds: 0,
            world_config: WorldConfigDump {
                current_space_id: Some(crate::world::components::SpaceId(7)),
                map_width: 32,
                map_height: 24,
                tile_size: 48.0,
                fill_floor_type: Some("grass".to_owned()),
            },
            map_layout: Some(MapLayoutDump {
                width: 32,
                height: 24,
                fill_floor_type: "grass".to_owned(),
            }),
            spaces: vec![RuntimeSpaceDump {
                id: crate::world::components::SpaceId(7),
                authored_id: "overworld".to_owned(),
                width: 32,
                height: 24,
                fill_floor_type: "grass".to_owned(),
                permanence: SpacePermanence::Persistent,
                instance_owner: None,
            }],
            network: NetworkStateDump {
                next_connection_id: 0,
            },
            world_objects: vec![WorldObjectStateDump {
                object_id: 99,
                definition_id: "goblin".to_owned(),
                properties: Default::default(),
                space_id: Some(crate::world::components::SpaceId(7)),
                tile_position: Some(TilePosition::ground(10, 10)),
                collider: false,
                movable: false,
                rotatable: false,
                storable: false,
                container_slots: None,
                npc: Some(NpcStateDump {
                    base_stats: Some(BaseStats::default()),
                    derived_stats: Some(DerivedStats::default()),
                    vital_stats: Some(VitalStats::full(25.0, 0.0)),
                    attack_profile: Some(AttackProfile::melee()),
                    combat_leash: Some(CombatLeash {
                        max_distance_tiles: 8,
                    }),
                    combat_target_object_id: None,
                    roaming_behavior: Some(RoamingBehavior {
                        bounds: crate::npc::components::RoamBounds {
                            min_x: 0,
                            min_y: 0,
                            max_x: 20,
                            max_y: 20,
                        },
                        step_interval_seconds: 0.5,
                        step_interval_jitter_seconds: 0.0,
                        idle_pause_chance: 0.0,
                        momentum_bias: 0.6,
                    }),
                    hostile_behavior: Some(HostileBehavior {
                        detect_distance_tiles: 5,
                        disengage_distance_tiles: 8,
                        alert_duration_seconds: 4.0,
                        requires_line_of_sight: true,
                    }),
                    // Pin the timer far in the future so the loaded NPC doesn't
                    // immediately wander off (10,10) before the test queries it.
                    roaming_step_timer: Some(RoamingStepTimer {
                        remaining_seconds: 1000.0,
                    }),
                    roaming_random_state: Some(RoamingRandomState { seed: 1 }),
                    spawn_group: None,
                    magic_effects: Default::default(),
                }),
                quantity: None,
                remaining_ttl: None,
                facing: None,
                hidden_detected_by: Vec::new(),
                hidden_dc: None,
            }],
            floor_maps: vec![],
            spawn_groups: vec![],
            world_time: 0.0,
        };
        write_world_dump(&save_path, &dump).unwrap();

        let mut app = setup_server_app(&save_path);
        app.update();

        let world = app.world_mut();
        let mut npc_query = world.query_filtered::<(
            &OverworldObject,
            &TilePosition,
            Option<&AiState>,
            Option<&AiMemory>,
        ), With<Npc>>();
        let mut saw_snapshot_goblin = false;
        for (object, tile, ai_state, ai_memory) in npc_query.iter(world) {
            assert!(
                ai_state.is_some(),
                "loaded NPC {} at {tile:?} is missing AiState — update_roaming_npcs requires it",
                object.definition_id,
            );
            assert!(
                ai_memory.is_some(),
                "loaded NPC {} at {tile:?} is missing AiMemory — update_roaming_npcs requires it",
                object.definition_id,
            );
            if object.definition_id == "goblin" && *tile == TilePosition::ground(10, 10) {
                saw_snapshot_goblin = true;
            }
        }
        assert!(
            saw_snapshot_goblin,
            "the goblin from the snapshot should have been restored at (10,10) with \
             AiState and AiMemory components",
        );

        let _ = std::fs::remove_file(save_path);
    }
}
