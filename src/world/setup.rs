use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::combat::damage_expr::DamageExpr;
use crate::npc::components::{
    HostileBehavior, Npc, RoamBounds, RoamingBehavior, RoamingRandomState, RoamingStepTimer,
};
use crate::persistence::WorldSnapshotStatus;
use crate::player::components::InventoryStack;
use crate::player::components::{AttributeSet, BaseStats, DerivedStats, VitalStats, WeaponDamage};
use crate::world::components::{
    ClientProjectedWorldObject, ClientRemotePlayerVisual, CombatHealthBar, DisplayedVitalStats,
    Facing, HealthBarDisplayPolicy, OverworldObject, SpaceId, SpacePosition, SpaceResident,
    TilePosition, ViewPosition, WorldVisual,
};
use crate::world::direction::Direction;
use crate::world::floor_map::FloorMaps;
use crate::world::map_layout::{
    MapBehavior, PortalDefinition, ResolvedObject, SpaceDefinition, SpaceDefinitions,
    SpacePermanence,
};
use crate::world::object_definitions::{
    AttackProfileKindDef, OverworldObjectDefinition, OverworldObjectDefinitions, StatModifiers,
};
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
    mut floor_maps: ResMut<FloorMaps>,
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
            &mut floor_maps,
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
                fill_floor_type: definition.fill_floor_type.clone(),
            });
        }
    }
}

pub fn instantiate_space(
    commands: &mut Commands,
    space_manager: &mut SpaceManager,
    floor_maps: &mut FloorMaps,
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
        fill_floor_type: definition.fill_floor_type.clone(),
        permanence,
        instance_owner,
    };
    space_manager.insert_space(runtime_space);
    floor_maps.insert(
        space_id,
        TilePosition::GROUND_FLOOR,
        definition.build_floor_map(TilePosition::GROUND_FLOOR),
    );

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
            definition,
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
    floor_maps: &mut FloorMaps,
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
        floor_maps,
        destination_definition,
        definitions,
        Some(instance_key),
        permanence,
    ))
}

pub fn spawn_overworld_object_instance(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    space: &SpaceDefinition,
    object: &ResolvedObject,
    space_id: SpaceId,
    tile_position: TilePosition,
) {
    let container_contents = if object.contents.is_empty() {
        None
    } else {
        Some(
            object
                .contents
                .iter()
                .map(|&id| {
                    space.find_resolved(id).map(|child| InventoryStack {
                        type_id: child.type_id.clone(),
                        properties: child.properties.clone(),
                        quantity: 1,
                    })
                })
                .collect(),
        )
    };

    let entity = spawn_overworld_object(
        commands,
        definitions,
        object.id,
        &object.type_id,
        container_contents,
        space_id,
        tile_position,
        None,
    );

    if let Some(facing) = object.facing {
        commands.entity(entity).insert(Facing(facing));
    }

    if let Some(behavior) = &object.behavior {
        let definition = definitions.get(&object.type_id);
        let base_stats = npc_base_stats_from_definition(definition);
        let derived_stats = DerivedStats::from_base(&base_stats);
        let max_health = definition
            .and_then(|d| d.hp.as_deref())
            .and_then(|raw| DamageExpr::parse(raw).ok())
            .map(|expr| expr.roll(&derived_stats.attributes).max(1))
            .unwrap_or(derived_stats.max_health) as f32;
        let max_mana = derived_stats.max_mana as f32;
        let (attack_profile, weapon_damage) = attack_profile_for_definition(definition);
        {
            let mut entity_commands = commands.entity(entity);
            entity_commands.insert((
                Npc,
                attack_profile,
                weapon_damage,
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

pub fn attach_combat_health_bar(
    commands: &mut Commands,
    entity: Entity,
    tile_size: f32,
    sprite_height: f32,
) {
    let bar_width = tile_size * 0.72;
    let bar_height = 5.0;
    let bar_y = sprite_height + 2.0;
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

/// Applies the definition-driven optional components (Collider, Movable, Storable,
/// Container, Quantity) to a freshly spawned overworld-object entity. Works with any
/// receiver that exposes Bevy's `.insert(Bundle)` method — i.e. `EntityCommands` or
/// `EntityWorldMut` — which is why this is a macro: there is no shared trait in Bevy.
#[macro_export]
macro_rules! apply_overworld_definition_components {
    ($entity:expr, $definition:expr, $container_contents:expr, $quantity:expr) => {{
        let __definition: &$crate::world::object_definitions::OverworldObjectDefinition =
            $definition;
        let __provided: Option<Vec<Option<$crate::player::components::InventoryStack>>> =
            $container_contents;
        let __quantity: Option<u32> = $quantity;
        if __definition.colliding {
            $entity.insert($crate::world::components::Collider);
        }
        if __definition.movable {
            $entity.insert($crate::world::components::Movable);
        }
        if __definition.rotatable {
            $entity.insert($crate::world::components::Rotatable);
        }
        if __definition.storable {
            $entity.insert($crate::world::components::Storable);
        }
        if let Some(capacity) = __definition.container_capacity {
            let slots: Vec<Option<$crate::player::components::InventoryStack>> = match __provided {
                Some(provided) => {
                    let mut padded = vec![None; capacity];
                    for (i, s) in provided.into_iter().enumerate().take(capacity) {
                        padded[i] = s;
                    }
                    padded
                }
                None => vec![None; capacity],
            };
            $entity.insert($crate::world::components::Container { slots });
        }
        if let Some(__q) = __quantity {
            if __q > 1 {
                $entity.insert($crate::world::components::Quantity(__q));
            }
        }
        if let Some(__dialog_node) = __definition.dialog_node.as_ref() {
            $entity.insert($crate::dialog::components::DialogNode(
                __dialog_node.clone(),
            ));
        }
    }};
}

pub fn spawn_overworld_object(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    object_id: u64,
    definition_id: &str,
    container_contents: Option<Vec<Option<InventoryStack>>>,
    space_id: SpaceId,
    tile_position: TilePosition,
    quantity: Option<u32>,
) -> Entity {
    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));

    let initial_facing = Facing(definition.render.default_facing.unwrap_or_default());

    let mut entity = commands.spawn((
        OverworldObject {
            object_id,
            definition_id: definition_id.to_owned(),
        },
        SpaceResident { space_id },
        tile_position,
        ViewPosition {
            space_id,
            tile: tile_position,
        },
        initial_facing,
    ));

    apply_overworld_definition_components!(entity, definition, container_contents, quantity);

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
    let visual = world_visual_for_definition(definition, world_config.tile_size);
    let sprite_height = visual.sprite_height;
    let uses_y_sort = visual.y_sort;
    let mut entity_commands = commands.spawn((
        ClientProjectedWorldObject {
            object_id,
            definition_id: definition_id.to_owned(),
        },
        ViewPosition {
            space_id: position.space_id,
            tile: position.tile_position,
        },
        visual,
        DisplayedVitalStats::default(),
        Facing(definition.render.default_facing.unwrap_or_default()),
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));
    if uses_y_sort && !definition.render.rotation_by_facing {
        entity_commands.insert(bevy::sprite::Anchor::BOTTOM_CENTER);
    }
    let entity = entity_commands.id();

    if is_npc {
        commands.entity(entity).insert(HealthBarDisplayPolicy {
            always_visible: false,
        });
        attach_combat_health_bar(commands, entity, world_config.tile_size, sprite_height);
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
    let visual = world_visual_for_definition(definition, world_config.tile_size);
    let sprite_height = visual.sprite_height;
    let uses_y_sort = visual.y_sort;

    let mut entity_commands = commands.spawn((
        ClientRemotePlayerVisual {
            player_id,
            object_id,
        },
        ViewPosition {
            space_id: position.space_id,
            tile: position.tile_position,
        },
        visual,
        DisplayedVitalStats::default(),
        Facing(Direction::default()),
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));
    if uses_y_sort && !definition.render.rotation_by_facing {
        entity_commands.insert(bevy::sprite::Anchor::BOTTOM_CENTER);
    }
    let entity = entity_commands.id();

    commands.entity(entity).insert(HealthBarDisplayPolicy {
        always_visible: false,
    });
    attach_combat_health_bar(commands, entity, world_config.tile_size, sprite_height);
    entity
}

pub fn world_visual_for_definition(
    definition: &OverworldObjectDefinition,
    tile_size: f32,
) -> WorldVisual {
    let sprite_size = definition.render.sprite_pixel_size(tile_size);
    WorldVisual {
        z_index: definition.render.z_index,
        y_sort: definition.render.y_sort,
        sprite_height: sprite_size.y,
        rotation_by_facing: definition.render.rotation_by_facing,
    }
}

pub fn sprite_for_definition(
    asset_server: &AssetServer,
    definition: &OverworldObjectDefinition,
    world_config: &WorldConfig,
) -> Sprite {
    let size = definition.render.sprite_pixel_size(world_config.tile_size);

    let mut sprite = if let Some(sprite_path) = &definition.render.sprite_path {
        let mut sprite = Sprite::from_image(asset_server.load(sprite_path));
        sprite.custom_size = Some(size);
        sprite
    } else {
        Sprite::from_color(definition.debug_color(), size)
    };

    sprite.image_mode = SpriteImageMode::Auto;

    sprite
}

pub fn npc_base_stats_from_definition(definition: Option<&OverworldObjectDefinition>) -> BaseStats {
    match definition {
        Some(def) => npc_base_stats_from_modifiers(&def.stats),
        None => BaseStats::npc_default(),
    }
}

fn npc_base_stats_from_modifiers(stats: &StatModifiers) -> BaseStats {
    let mut base = BaseStats::npc_default();
    let defaults = base.attributes;
    base.attributes = AttributeSet::new(
        if stats.strength != 0 {
            stats.strength
        } else {
            defaults.strength
        },
        if stats.agility != 0 {
            stats.agility
        } else {
            defaults.agility
        },
        if stats.constitution != 0 {
            stats.constitution
        } else {
            defaults.constitution
        },
        if stats.willpower != 0 {
            stats.willpower
        } else {
            defaults.willpower
        },
        if stats.charisma != 0 {
            stats.charisma
        } else {
            defaults.charisma
        },
        if stats.focus != 0 {
            stats.focus
        } else {
            defaults.focus
        },
    );
    base.max_health = stats.max_health;
    base.max_mana = stats.max_mana;
    base.storage_slots = stats.storage_slots;
    base
}

pub fn attack_profile_for_definition(
    definition: Option<&OverworldObjectDefinition>,
) -> (AttackProfile, WeaponDamage) {
    let Some(definition) = definition else {
        return (AttackProfile::melee(), WeaponDamage::default());
    };
    let damage = definition
        .damage
        .as_deref()
        .and_then(|raw| DamageExpr::parse(raw).ok())
        .map(WeaponDamage)
        .unwrap_or_default();
    let profile = match definition.attack_profile {
        Some(def) => match def.kind {
            AttackProfileKindDef::Melee => AttackProfile::melee(),
            AttackProfileKindDef::Ranged => {
                let range = definition.base_range_tiles.unwrap_or(4).max(1);
                AttackProfile::ranged(range)
            }
        },
        None => AttackProfile::melee(),
    };
    (profile, damage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npc_stats_fall_back_to_default_when_none() {
        let base = npc_base_stats_from_definition(None);
        let default = BaseStats::npc_default();
        assert_eq!(base.attributes, default.attributes);
        assert_eq!(base.max_health, default.max_health);
        assert_eq!(base.max_mana, default.max_mana);
        assert_eq!(base.storage_slots, default.storage_slots);
    }

    #[test]
    fn npc_stats_fall_back_per_field_when_modifier_is_zero() {
        let base = npc_base_stats_from_modifiers(&StatModifiers::default());
        assert_eq!(base.attributes, BaseStats::npc_default().attributes);
    }

    #[test]
    fn npc_stats_override_when_modifier_is_set() {
        let stats = StatModifiers {
            strength: 18,
            agility: 5,
            constitution: 16,
            willpower: 7,
            charisma: 4,
            focus: 7,
            max_health: 0,
            max_mana: 0,
            storage_slots: 0,
        };
        let base = npc_base_stats_from_modifiers(&stats);
        assert_eq!(base.attributes, AttributeSet::new(18, 5, 16, 7, 4, 7));
    }

    #[test]
    fn npc_stats_partial_override_keeps_defaults_for_zero_fields() {
        let stats = StatModifiers {
            strength: 20,
            ..StatModifiers::default()
        };
        let base = npc_base_stats_from_modifiers(&stats);
        let defaults = BaseStats::npc_default().attributes;
        assert_eq!(base.attributes.strength, 20);
        assert_eq!(base.attributes.agility, defaults.agility);
        assert_eq!(base.attributes.constitution, defaults.constitution);
        assert_eq!(base.attributes.willpower, defaults.willpower);
        assert_eq!(base.attributes.charisma, defaults.charisma);
        assert_eq!(base.attributes.focus, defaults.focus);
    }
}
