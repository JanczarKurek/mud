use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::npc::components::{
    HostileBehavior, Npc, RoamBounds, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
};
use crate::persistence::WorldSnapshotStatus;
use crate::player::components::{BaseStats, DerivedStats, VitalStats};
use crate::world::components::{
    ClientProjectedWorldObject, ClientRemotePlayerVisual, Collider, CombatHealthBar, Container,
    DisplayedVitalStats, HealthBarDisplayPolicy, Movable, OverworldObject, SpaceId, SpacePosition,
    SpaceResident, Storable, TilePosition, WorldVisual,
};
use crate::world::map_layout::{
    MapBehavior, MapObjectInstance, PortalDefinition, SpaceDefinition, SpaceDefinitions,
    SpacePermanence,
};
use crate::world::object_definitions::{OverworldObjectDefinition, OverworldObjectDefinitions};
use crate::world::resources::{PortalInstanceKey, RuntimeSpace, SpaceManager};
use crate::world::WorldConfig;

#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WorldStartupSet {
    InitializeRuntimeSpaces,
}

pub fn initialize_runtime_spaces(
    mut commands: Commands,
    definitions: Res<SpaceDefinitions>,
    object_definitions: Res<OverworldObjectDefinitions>,
    mut space_manager: ResMut<SpaceManager>,
    snapshot_status: Option<Res<WorldSnapshotStatus>>,
) {
    if snapshot_status.as_ref().is_some_and(|status| status.loaded) {
        return;
    }

    for definition in definitions.iter() {
        if !definition.permanence.is_persistent() {
            continue;
        }

        let space_id = instantiate_space(
            &mut commands,
            &mut space_manager,
            definition,
            &object_definitions,
            None,
            definition.permanence,
        );

        if definition.authored_id == definitions.bootstrap_space_id {
            commands.insert_resource(WorldConfig {
                current_space_id: space_id,
                map_width: definition.width,
                map_height: definition.height,
                tile_size: 48.0,
                fill_object_type: definition.fill_object_type.clone(),
            });
        }
    }
}

pub fn instantiate_space(
    commands: &mut Commands,
    space_manager: &mut SpaceManager,
    definition: &SpaceDefinition,
    definitions: &OverworldObjectDefinitions,
    instance_owner: Option<PortalInstanceKey>,
    permanence: SpacePermanence,
) -> SpaceId {
    let space_id = space_manager.allocate_space_id();
    let runtime_space = RuntimeSpace {
        id: space_id,
        authored_id: definition.authored_id.clone(),
        width: definition.width,
        height: definition.height,
        fill_object_type: definition.fill_object_type.clone(),
        permanence,
        instance_owner,
    };
    space_manager.insert_space(runtime_space);

    for object in &definition.resolved_objects {
        if definition.is_contained(object.id) {
            continue;
        }

        let Some(placement) = object.placement else {
            continue;
        };

        spawn_overworld_object_instance(
            commands,
            definitions,
            object,
            space_id,
            placement.to_tile_position(),
        );
    }

    space_id
}

pub fn resolve_portal_destination_space(
    commands: &mut Commands,
    authored_spaces: &SpaceDefinitions,
    definitions: &OverworldObjectDefinitions,
    space_manager: &mut SpaceManager,
    source_space_id: SpaceId,
    portal: &PortalDefinition,
) -> Option<SpaceId> {
    let destination_definition = authored_spaces.get(&portal.destination_space_id)?;
    let permanence = portal
        .destination_permanence
        .unwrap_or(destination_definition.permanence);

    if permanence.is_persistent() {
        return space_manager.persistent_space_id(&portal.destination_space_id);
    }

    let instance_key = PortalInstanceKey {
        source_space_id,
        portal_id: portal.id.clone(),
    };
    if let Some(space_id) = space_manager.portal_instance(&instance_key) {
        return Some(space_id);
    }

    Some(instantiate_space(
        commands,
        space_manager,
        destination_definition,
        definitions,
        Some(instance_key),
        permanence,
    ))
}

pub fn spawn_ground_tiles_for_current_space(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    current_ground_tiles: Query<Entity, With<crate::world::components::ClientGroundTile>>,
) {
    if !world_config.is_changed() {
        return;
    }

    for entity in &current_ground_tiles {
        commands.entity(entity).despawn();
    }

    for y in 0..world_config.map_height {
        for x in 0..world_config.map_width {
            spawn_ground_tile(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                &world_config.fill_object_type,
                TilePosition::new(x, y),
            );
        }
    }
}

pub fn spawn_overworld_object_instance(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    object: &MapObjectInstance,
    space_id: SpaceId,
    tile_position: TilePosition,
) {
    let container_contents = if object.contents.is_empty() {
        None
    } else {
        Some(object.contents.clone())
    };

    let entity = spawn_overworld_object(
        commands,
        definitions,
        object.id,
        &object.type_id,
        container_contents,
        space_id,
        tile_position,
    );

    if let Some(behavior) = &object.behavior {
        let base_stats = BaseStats::npc_default();
        let derived_stats = DerivedStats::from_base(&base_stats);
        let max_health = derived_stats.max_health as f32;
        let max_mana = derived_stats.max_mana as f32;
        {
            let mut entity_commands = commands.entity(entity);
            entity_commands.insert((
                Npc,
                AttackProfile::melee(),
                base_stats,
                derived_stats,
                VitalStats::full(max_health, max_mana),
            ));

            match behavior {
                MapBehavior::Roam {
                    step_interval_seconds,
                    bounds,
                } => {
                    entity_commands.insert((
                        RoamingBehavior {
                            bounds: RoamBounds {
                                min_x: bounds.min_x,
                                min_y: bounds.min_y,
                                max_x: bounds.max_x,
                                max_y: bounds.max_y,
                            },
                            step_interval_seconds: (*step_interval_seconds).max(0.05),
                        },
                        RoamingStepTimer {
                            remaining_seconds: *step_interval_seconds,
                        },
                        RoamingRandomState {
                            seed: object.id.wrapping_mul(1_103_515_245).wrapping_add(12_345),
                        },
                    ));
                }
                MapBehavior::RoamAndChase {
                    step_interval_seconds,
                    bounds,
                    detect_distance_tiles,
                    disengage_distance_tiles,
                } => {
                    entity_commands.insert((
                        RoamingBehavior {
                            bounds: RoamBounds {
                                min_x: bounds.min_x,
                                min_y: bounds.min_y,
                                max_x: bounds.max_x,
                                max_y: bounds.max_y,
                            },
                            step_interval_seconds: (*step_interval_seconds).max(0.05),
                        },
                        HostileBehavior {
                            detect_distance_tiles: (*detect_distance_tiles).max(1),
                            disengage_distance_tiles: (*disengage_distance_tiles)
                                .max(*detect_distance_tiles),
                        },
                        CombatLeash {
                            max_distance_tiles: (*disengage_distance_tiles)
                                .max(*detect_distance_tiles),
                        },
                        RoamingStepTimer {
                            remaining_seconds: *step_interval_seconds,
                        },
                        RoamingRandomState {
                            seed: object.id.wrapping_mul(1_103_515_245).wrapping_add(12_345),
                        },
                    ));
                }
            }
        }
    }
}

pub fn attach_combat_health_bar(commands: &mut Commands, entity: Entity, tile_size: f32) {
    let bar_width = tile_size * 0.72;
    let bar_height = 5.0;
    let bar_y = tile_size * 0.52;
    let fill_width = bar_width - 2.0;

    let mut root_entity = Entity::PLACEHOLDER;
    let mut fill_entity = Entity::PLACEHOLDER;

    commands.entity(entity).with_children(|parent| {
        root_entity = parent
            .spawn((
                Sprite::from_color(
                    Color::srgba(0.08, 0.06, 0.06, 0.92),
                    Vec2::new(bar_width, bar_height),
                ),
                Transform::from_xyz(0.0, bar_y, 5.0),
                Visibility::Hidden,
            ))
            .with_children(|bar_root| {
                fill_entity = bar_root
                    .spawn((
                        Sprite::from_color(
                            Color::srgb(0.78, 0.12, 0.14),
                            Vec2::new(fill_width, bar_height - 2.0),
                        ),
                        Transform::from_xyz(0.0, 0.0, 0.1),
                    ))
                    .id();
            })
            .id();
    });

    commands.entity(entity).insert(CombatHealthBar {
        root_entity,
        fill_entity,
        fill_width,
    });
}

fn spawn_ground_tile(
    commands: &mut Commands,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    definition_id: &str,
    tile_position: TilePosition,
) {
    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));
    let sprite = sprite_for_definition(asset_server, definition, world_config);

    commands.spawn((
        crate::world::components::ClientGroundTile,
        SpaceResident {
            space_id: world_config.current_space_id,
        },
        tile_position,
        WorldVisual {
            z_index: definition.render.z_index,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));
}

pub fn spawn_overworld_object(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    object_id: u64,
    definition_id: &str,
    container_contents: Option<Vec<u64>>,
    space_id: SpaceId,
    tile_position: TilePosition,
) -> Entity {
    let mut entity = commands.spawn((
        OverworldObject {
            object_id,
            definition_id: definition_id.to_owned(),
        },
        SpaceResident { space_id },
        tile_position,
    ));

    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));

    if definition.colliding {
        entity.insert(Collider);
    }

    if definition.movable {
        entity.insert(Movable);
    }

    if definition.storable {
        entity.insert(Storable);
    }

    if let Some(capacity) = definition.container_capacity {
        entity.insert(Container {
            slots: vec![None; capacity],
        });

        if let Some(container_contents) = container_contents {
            let mut slots = vec![None; capacity];
            for (index, object_id) in container_contents.into_iter().enumerate().take(capacity) {
                slots[index] = Some(object_id);
            }
            entity.insert(Container { slots });
        }
    }

    entity.id()
}

pub fn spawn_client_projected_world_object(
    commands: &mut Commands,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    object_id: u64,
    definition_id: &str,
    position: SpacePosition,
    is_npc: bool,
) -> Entity {
    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));
    let sprite = sprite_for_definition(asset_server, definition, world_config);
    let entity = commands
        .spawn((
            ClientProjectedWorldObject {
                object_id,
                definition_id: definition_id.to_owned(),
            },
            SpaceResident {
                space_id: position.space_id,
            },
            position.tile_position,
            WorldVisual {
                z_index: definition.render.z_index,
            },
            DisplayedVitalStats::default(),
            sprite,
            Transform::from_xyz(0.0, 0.0, definition.render.z_index),
        ))
        .id();

    if is_npc {
        commands.entity(entity).insert(HealthBarDisplayPolicy {
            always_visible: false,
        });
        attach_combat_health_bar(commands, entity, world_config.tile_size);
    }

    entity
}

pub fn spawn_client_remote_player(
    commands: &mut Commands,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    player_id: crate::player::components::PlayerId,
    object_id: u64,
    position: SpacePosition,
) -> Entity {
    let definition = definitions
        .get("player")
        .unwrap_or_else(|| panic!("Missing overworld object definition for id 'player'"));
    let mut sprite = sprite_for_definition(asset_server, definition, world_config);
    sprite.color = Color::srgba(0.82, 0.92, 1.0, 0.8);

    let entity = commands
        .spawn((
            ClientRemotePlayerVisual {
                player_id,
                object_id,
            },
            SpaceResident {
                space_id: position.space_id,
            },
            position.tile_position,
            WorldVisual {
                z_index: definition.render.z_index,
            },
            DisplayedVitalStats::default(),
            sprite,
            Transform::from_xyz(0.0, 0.0, definition.render.z_index),
        ))
        .id();

    commands.entity(entity).insert(HealthBarDisplayPolicy {
        always_visible: false,
    });
    attach_combat_health_bar(commands, entity, world_config.tile_size);
    entity
}

fn sprite_for_definition(
    asset_server: &AssetServer,
    definition: &OverworldObjectDefinition,
    world_config: &WorldConfig,
) -> Sprite {
    let mut sprite = if let Some(sprite_path) = &definition.render.sprite_path {
        let mut sprite = Sprite::from_image(asset_server.load(sprite_path));
        sprite.custom_size = Some(Vec2::splat(
            world_config.tile_size * definition.render.debug_size,
        ));
        sprite
    } else {
        Sprite::from_color(
            definition.debug_color(),
            Vec2::splat(world_config.tile_size * definition.render.debug_size),
        )
    };

    sprite.image_mode = SpriteImageMode::Auto;
    sprite
}
