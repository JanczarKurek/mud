use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::app::AppExit;
use bevy::ecs::message::MessageReader;
use bevy::log::{error, info};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::components::{AttackProfile, CombatLeash, CombatTarget};
use crate::network::resources::TcpServerState;
use crate::npc::components::{
    HostileBehavior, Npc, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
};
use crate::player::components::{
    BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, Player, PlayerId,
    PlayerIdentity, VitalStats,
};
use crate::world::components::{
    Collider, Container, Movable, OverworldObject, SpaceResident, Storable, TilePosition,
};
use crate::world::map_layout::{SpaceDefinitions, SpacePermanence};
use crate::world::object_registry::{ObjectRegistry, ObjectRegistrySnapshotEntry};
use crate::world::resources::{RuntimeSpace, SpaceManager};
use crate::world::setup::initialize_runtime_spaces;
use crate::world::WorldConfig;

pub const DEFAULT_WORLD_SAVE_PATH: &str = "saves/world-state.json";

pub struct PersistenceServerPlugin {
    pub save_path: Option<String>,
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct WorldSnapshotStatus {
    pub loaded: bool,
}

#[derive(Resource, Clone, Debug)]
pub struct WorldSaveConfig {
    pub path: PathBuf,
}

impl Default for WorldSaveConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from(DEFAULT_WORLD_SAVE_PATH),
        }
    }
}

impl Plugin for PersistenceServerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(WorldSaveConfig {
            path: self
                .save_path
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_WORLD_SAVE_PATH)),
        })
        .insert_resource(WorldSnapshotStatus::default())
        .add_systems(Startup, load_world_from_snapshot.before(initialize_runtime_spaces))
        .add_systems(Last, save_world_on_app_exit);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldStateDump {
    pub format_version: u32,
    pub saved_at_unix_seconds: u64,
    pub world_config: WorldConfigDump,
    pub map_layout: MapLayoutDump,
    pub object_registry: ObjectRegistryDump,
    pub network: NetworkStateDump,
    pub players: Vec<PlayerStateDump>,
    pub world_objects: Vec<WorldObjectStateDump>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldConfigDump {
    pub map_width: i32,
    pub map_height: i32,
    pub tile_size: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MapLayoutDump {
    pub width: i32,
    pub height: i32,
    pub fill_object_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObjectRegistryDump {
    pub next_runtime_id: u64,
    pub entries: Vec<ObjectRegistrySnapshotEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NetworkStateDump {
    pub next_connection_id: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlayerStateDump {
    pub player_id: PlayerId,
    pub object_id: u64,
    pub tile_position: TilePosition,
    pub inventory: Inventory,
    pub chat_log: ChatLog,
    pub base_stats: BaseStats,
    pub derived_stats: DerivedStats,
    pub vital_stats: VitalStats,
    pub movement_cooldown: MovementCooldown,
    pub attack_profile: AttackProfile,
    pub combat_leash: CombatLeash,
    pub combat_target_object_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorldObjectStateDump {
    pub object_id: u64,
    pub definition_id: String,
    pub tile_position: Option<TilePosition>,
    pub collider: bool,
    pub movable: bool,
    pub storable: bool,
    pub container_slots: Option<Vec<Option<u64>>>,
    pub npc: Option<NpcStateDump>,
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
}

fn save_world_on_app_exit(
    mut app_exit_reader: MessageReader<AppExit>,
    save_config: Res<WorldSaveConfig>,
    world_config: Res<WorldConfig>,
    object_registry: Res<ObjectRegistry>,
    tcp_server_state: Option<Res<TcpServerState>>,
    player_query: Query<
        (
            Entity,
            &PlayerIdentity,
            &OverworldObject,
            &TilePosition,
            &Inventory,
            &ChatLog,
            &BaseStats,
            &DerivedStats,
            &VitalStats,
            &MovementCooldown,
            &AttackProfile,
            &CombatLeash,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    world_object_query: Query<
        (
            Entity,
            &OverworldObject,
            Option<&TilePosition>,
            Has<Collider>,
            Has<Movable>,
            Has<Storable>,
            Option<&Container>,
            Has<Npc>,
            Option<&CombatTarget>,
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
        ),
        With<Npc>,
    >,
) {
    if app_exit_reader.read().next().is_none() {
        return;
    }

    let mut entity_to_object_id = std::collections::HashMap::new();
    for (entity, _, object, _, _, _, _, _, _, _, _, _, _) in player_query.iter() {
        entity_to_object_id.insert(entity, object.object_id);
    }
    for (entity, object, _, _, _, _, _, _, _) in world_object_query.iter() {
        entity_to_object_id.insert(entity, object.object_id);
    }

    let mut players = player_query
        .iter()
        .map(
            |(
                _entity,
                identity,
                object,
                tile_position,
                inventory,
                chat_log,
                base_stats,
                derived_stats,
                vital_stats,
                movement_cooldown,
                attack_profile,
                combat_leash,
                combat_target,
            )| PlayerStateDump {
                player_id: identity.id,
                object_id: object.object_id,
                tile_position: *tile_position,
                inventory: inventory.clone(),
                chat_log: chat_log.clone(),
                base_stats: base_stats.clone(),
                derived_stats: derived_stats.clone(),
                vital_stats: vital_stats.clone(),
                movement_cooldown: movement_cooldown.clone(),
                attack_profile: *attack_profile,
                combat_leash: *combat_leash,
                combat_target_object_id: combat_target
                    .and_then(|target| entity_to_object_id.get(&target.entity).copied()),
            },
        )
        .collect::<Vec<_>>();
    players.sort_by_key(|player| player.player_id.0);

    let mut world_objects = world_object_query
        .iter()
        .map(
            |(
                entity,
                object,
                tile_position,
                collider,
                movable,
                storable,
                container,
                is_npc,
                combat_target,
            )| WorldObjectStateDump {
                object_id: object.object_id,
                definition_id: object.definition_id.clone(),
                tile_position: tile_position.copied(),
                collider,
                movable,
                storable,
                container_slots: container.map(|container| container.slots.clone()),
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
                    }
                }),
            },
        )
        .collect::<Vec<_>>();
    world_objects.sort_by_key(|object| object.object_id);

    let dump = WorldStateDump {
        format_version: 1,
        saved_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        world_config: WorldConfigDump {
            map_width: world_config.map_width,
            map_height: world_config.map_height,
            tile_size: world_config.tile_size,
        },
        map_layout: MapLayoutDump {
            width: world_config.map_width,
            height: world_config.map_height,
            fill_object_type: world_config.fill_object_type.clone(),
        },
        object_registry: ObjectRegistryDump {
            next_runtime_id: object_registry.next_runtime_id(),
            entries: object_registry.snapshot_entries(),
        },
        network: NetworkStateDump {
            next_connection_id: tcp_server_state
                .as_ref()
                .map(|state| state.next_connection_id)
                .unwrap_or_default(),
        },
        players,
        world_objects,
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
    mut tcp_server_state: Option<ResMut<TcpServerState>>,
) {
    let Ok(dump) = read_world_dump(&save_config.path) else {
        return;
    };

    let WorldStateDump {
        world_config: dump_world_config,
        map_layout: dump_map_layout,
        object_registry: dump_registry,
        network: dump_network,
        players,
        world_objects,
        ..
    } = dump;

    let bootstrap_definition = authored_spaces.bootstrap_space();
    let space_id = space_manager.allocate_space_id();
    space_manager.insert_space(RuntimeSpace {
        id: space_id,
        authored_id: bootstrap_definition.authored_id.clone(),
        width: dump_world_config.map_width,
        height: dump_world_config.map_height,
        fill_object_type: dump_map_layout.fill_object_type.clone(),
        permanence: SpacePermanence::Persistent,
        instance_owner: None,
    });

    world_config.current_space_id = space_id;
    world_config.map_width = dump_world_config.map_width;
    world_config.map_height = dump_world_config.map_height;
    world_config.tile_size = dump_world_config.tile_size;
    world_config.fill_object_type = dump_map_layout.fill_object_type;
    *object_registry =
        ObjectRegistry::from_snapshot(dump_registry.entries, dump_registry.next_runtime_id);
    if let Some(server_state) = tcp_server_state.as_mut() {
        server_state.next_connection_id = dump_network.next_connection_id;
    }

    let mut object_entities = std::collections::HashMap::new();
    let mut pending_combat_targets = Vec::new();

    for player in players {
        let entity = commands
            .spawn((
                Player,
                PlayerIdentity {
                    id: player.player_id,
                },
                player.inventory,
                player.chat_log,
                player.base_stats,
                player.derived_stats,
                player.vital_stats,
                player.movement_cooldown,
                player.attack_profile,
                player.combat_leash,
                Collider,
                OverworldObject {
                    object_id: player.object_id,
                    definition_id: "player".to_owned(),
                },
                SpaceResident { space_id },
                player.tile_position,
            ))
            .id();
        object_entities.insert(player.object_id, entity);
        if let Some(target_object_id) = player.combat_target_object_id {
            pending_combat_targets.push((player.object_id, target_object_id));
        }
    }

    for object in world_objects {
        let mut entity = commands.spawn((OverworldObject {
            object_id: object.object_id,
            definition_id: object.definition_id,
        }, SpaceResident { space_id }));

        if let Some(tile_position) = object.tile_position {
            entity.insert(tile_position);
        }
        if object.collider {
            entity.insert(Collider);
        }
        if object.movable {
            entity.insert(Movable);
        }
        if object.storable {
            entity.insert(Storable);
        }
        if let Some(container_slots) = object.container_slots {
            entity.insert(Container {
                slots: container_slots,
            });
        }
        if let Some(npc) = object.npc {
            entity.insert(Npc);
            if let Some(base_stats) = npc.base_stats {
                entity.insert(base_stats);
            }
            if let Some(derived_stats) = npc.derived_stats {
                entity.insert(derived_stats);
            }
            if let Some(vital_stats) = npc.vital_stats {
                entity.insert(vital_stats);
            }
            if let Some(attack_profile) = npc.attack_profile {
                entity.insert(attack_profile);
            }
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
            if let Some(target_object_id) = npc.combat_target_object_id {
                pending_combat_targets.push((object.object_id, target_object_id));
            }
        }

        let entity_id = entity.id();
        object_entities.insert(object.object_id, entity_id);
    }

    for (source_object_id, target_object_id) in pending_combat_targets {
        let Some(&source_entity) = object_entities.get(&source_object_id) else {
            continue;
        };
        let Some(&target_entity) = object_entities.get(&target_object_id) else {
            continue;
        };
        commands.entity(source_entity).insert(CombatTarget {
            entity: target_entity,
        });
    }

    snapshot_status.loaded = true;
    info!("loaded world state from {}", save_config.path.display());
}

fn write_world_dump(path: &Path, dump: &WorldStateDump) -> std::io::Result<()> {
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
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::CombatPlugin;
    use crate::game::GameServerPlugin;
    use crate::magic::MagicPlugin;
    use crate::network::resources::TcpServerState;
    use crate::npc::NpcPlugin;
    use crate::player::setup::spawn_player_authoritative;
    use crate::player::PlayerServerPlugin;
    use crate::world::WorldServerPlugin;

    fn setup_server_app(save_path: &Path) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.insert_resource(TcpServerState::default());
        app.add_plugins((
            GameServerPlugin,
            WorldServerPlugin,
            NpcPlugin,
            PlayerServerPlugin,
            CombatPlugin,
            MagicPlugin,
            PersistenceServerPlugin {
                save_path: Some(save_path.display().to_string()),
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
            TilePosition::new(world_config.map_width / 2, world_config.map_height / 2)
        };
        let world_config = WorldConfig {
            current_space_id: app.world().resource::<WorldConfig>().current_space_id,
            map_width: app.world().resource::<WorldConfig>().map_width,
            map_height: app.world().resource::<WorldConfig>().map_height,
            tile_size: app.world().resource::<WorldConfig>().tile_size,
            fill_object_type: app
                .world()
                .resource::<WorldConfig>()
                .fill_object_type
                .clone(),
        };
        spawn_player_authoritative(
            &mut app.world_mut().commands(),
            &world_config,
            PlayerId(42),
            object_id,
            spawn_tile,
        );
        app.world_mut().flush();

        app.world_mut().write_message(AppExit::Success);
        app.update();

        let dump =
            serde_json::from_str::<WorldStateDump>(&std::fs::read_to_string(&save_path).unwrap())
                .unwrap();

        assert_eq!(dump.format_version, 1);
        assert!(dump
            .players
            .iter()
            .any(|player| player.player_id == PlayerId(42)));
        assert!(dump
            .object_registry
            .entries
            .iter()
            .any(|entry| entry.object_id == object_id && entry.type_id == "player"));

        let _ = std::fs::remove_file(save_path);
    }

    #[test]
    fn loads_world_dump_on_startup_when_snapshot_exists() {
        let save_path =
            std::env::temp_dir().join(format!("mud2-world-load-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&save_path);

        let dump = WorldStateDump {
            format_version: 1,
            saved_at_unix_seconds: 0,
            world_config: WorldConfigDump {
                map_width: 32,
                map_height: 24,
                tile_size: 48.0,
            },
            map_layout: MapLayoutDump {
                width: 32,
                height: 24,
                fill_object_type: "grass".to_owned(),
            },
            object_registry: ObjectRegistryDump {
                next_runtime_id: 1000,
                entries: vec![
                    ObjectRegistrySnapshotEntry {
                        object_id: 42,
                        type_id: "player".to_owned(),
                        properties: Default::default(),
                    },
                    ObjectRegistrySnapshotEntry {
                        object_id: 43,
                        type_id: "barrel".to_owned(),
                        properties: Default::default(),
                    },
                ],
            },
            network: NetworkStateDump {
                next_connection_id: 77,
            },
            players: vec![PlayerStateDump {
                player_id: PlayerId(7),
                object_id: 42,
                tile_position: TilePosition::new(5, 6),
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
                combat_target_object_id: None,
            }],
            world_objects: vec![WorldObjectStateDump {
                object_id: 43,
                definition_id: "barrel".to_owned(),
                tile_position: Some(TilePosition::new(7, 6)),
                collider: false,
                movable: false,
                storable: false,
                container_slots: Some(vec![None, None]),
                npc: None,
            }],
        };
        write_world_dump(&save_path, &dump).unwrap();

        let mut app = setup_server_app(&save_path);
        app.update();

        assert!(app.world().resource::<WorldSnapshotStatus>().loaded);
        assert_eq!(
            app.world().resource::<TcpServerState>().next_connection_id,
            77
        );
        assert_eq!(
            app.world().resource::<ObjectRegistry>().next_runtime_id(),
            1000
        );

        let restored_players = {
            let world = app.world_mut();
            let mut player_query =
                world.query::<(&PlayerIdentity, &TilePosition, &OverworldObject)>();
            player_query.iter(world).collect::<Vec<_>>()
        };
        assert_eq!(restored_players.len(), 1);
        assert_eq!(restored_players[0].0.id, PlayerId(7));
        assert_eq!(*restored_players[0].1, TilePosition::new(5, 6));
        assert_eq!(restored_players[0].2.object_id, 42);

        let has_restored_object = {
            let world = app.world_mut();
            let mut object_query =
                world.query_filtered::<(&OverworldObject, &TilePosition), Without<Player>>();
            object_query
                .iter(world)
                .any(|(object, tile)| object.object_id == 43 && *tile == TilePosition::new(7, 6))
        };
        assert!(has_restored_object);

        let _ = std::fs::remove_file(save_path);
    }
}
