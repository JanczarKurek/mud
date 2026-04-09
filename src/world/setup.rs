use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::npc::components::{
    HostileBehavior, Npc, RoamBounds, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
};
use crate::player::components::{BaseStats, DerivedStats, VitalStats};
use crate::world::components::{
    Collider, CombatHealthBar, Container, Movable, OverworldObject, Storable, TilePosition,
    WorldVisual,
};
use crate::world::map_layout::{MapBehavior, MapLayout, MapObjectInstance};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::WorldConfig;

pub fn spawn_world(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    map_layout: Res<MapLayout>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
) {
    for y in 0..world_config.map_height {
        for x in 0..world_config.map_width {
            spawn_ground_tile(
                &mut commands,
                &asset_server,
                &definitions,
                &world_config,
                &map_layout.fill_object_type,
                TilePosition::new(x, y),
            );
        }
    }

    for object in &map_layout.resolved_objects {
        if map_layout.is_contained(object.id) {
            continue;
        }

        let Some(placement) = object.placement else {
            continue;
        };

        spawn_overworld_object_instance(
            &mut commands,
            &asset_server,
            &map_layout,
            &definitions,
            &world_config,
            object,
            placement.to_tile_position(),
        );
    }
}

pub fn spawn_overworld_object_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    map_layout: &MapLayout,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    object: &MapObjectInstance,
    tile_position: TilePosition,
) {
    let container_contents = if object.contents.is_empty() {
        None
    } else {
        Some(object.contents.clone())
    };

    let entity = spawn_overworld_object(
        commands,
        asset_server,
        definitions,
        world_config,
        object.id,
        &object.type_id,
        container_contents,
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

        attach_combat_health_bar(commands, entity, world_config.tile_size);
    }

    let _ = map_layout;
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

    commands.spawn((
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
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    object_id: u64,
    definition_id: &str,
    container_contents: Option<Vec<u64>>,
    tile_position: TilePosition,
) -> Entity {
    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));

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

    let mut entity = commands.spawn((
        OverworldObject {
            object_id,
            definition_id: definition_id.to_owned(),
        },
        tile_position,
        WorldVisual {
            z_index: definition.render.z_index,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));

    if definition.colliding {
        entity.insert(Collider);
    }

    if definition.movable {
        entity.insert(Movable);
    }

    if definition.storable {
        entity.insert(Storable);
    }

    if let Some(container_capacity) = definition.container_capacity {
        let mut slots = vec![None; container_capacity];
        if let Some(contents) = container_contents {
            assert!(
                contents.len() <= container_capacity,
                "Container object {} exceeds capacity {}",
                object_id,
                container_capacity
            );
            for (index, contained_object_id) in contents.into_iter().enumerate() {
                slots[index] = Some(contained_object_id);
            }
        }

        entity.insert(Container { slots });
    }

    entity.id()
}
