use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::combat::damage_expr::DamageExpr;
use crate::npc::components::{
    AiMemory, AiState, HostileBehavior, Npc, RoamBounds, RoamingBehavior, RoamingRandomState,
    RoamingStepTimer,
};
use crate::persistence::WorldSnapshotStatus;
use crate::player::components::InventoryStack;
use crate::player::components::{
    AttributeSet, BaseStats, DefenseStats, DerivedStats, VitalStats, WeaponDamage,
};
use crate::world::animation::{build_animated_sprite_components, AnimatedSprite};
use crate::world::components::{
    ClientProjectedWorldObject, ClientRemotePlayerVisual, CombatHealthBar, DisplayedVitalStats,
    Facing, HealthBarDisplayPolicy, OverworldObject, SpaceId, SpacePosition, SpaceResident,
    TilePosition, ViewPosition, WorldVisual,
};
use crate::world::direction::Direction;
use crate::world::floor_map::FloorMaps;
use crate::world::map_layout::{
    PortalDefinition, ResolvedObject, SpaceDefinition, SpaceDefinitions, SpacePermanence,
};
use crate::world::object_definitions::{
    AttackProfileKindDef, OverworldObjectDefinition, OverworldObjectDefinitions, StatModifiers,
};
use crate::world::object_registry::ObjectRegistry;
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
    object_registry: Res<ObjectRegistry>,
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
            &object_registry,
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

#[allow(clippy::too_many_arguments)]
pub fn instantiate_space(
    commands: &mut Commands,
    space_manager: &mut SpaceManager,
    floor_maps: &mut FloorMaps,
    definition: &SpaceDefinition,
    definitions: &OverworldObjectDefinitions,
    object_registry: &ObjectRegistry,
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
        lighting: definition.lighting.clone(),
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
            object_registry,
            definition,
            object,
            space_id,
            placement.to_tile_position(),
        );
    }

    space_id
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_portal_destination_space(
    commands: &mut Commands,
    authored_spaces: &SpaceDefinitions,
    definitions: &OverworldObjectDefinitions,
    object_registry: &ObjectRegistry,
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
        object_registry,
        Some(instance_key),
        permanence,
    ))
}

pub fn spawn_overworld_object_instance(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    object_registry: &ObjectRegistry,
    space: &SpaceDefinition,
    object: &ResolvedObject,
    space_id: SpaceId,
    tile_position: TilePosition,
) -> Entity {
    let container_contents = if object.contents.is_empty() {
        None
    } else {
        Some(
            object
                .contents
                .iter()
                .map(|&id| {
                    space.find_resolved(id).map(|child| {
                        InventoryStack::item(child.type_id.clone(), child.properties.clone(), 1)
                    })
                })
                .collect(),
        )
    };

    let entity = spawn_overworld_object(
        commands,
        definitions,
        object_registry,
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

    // Shopkeeper / Stockpile: if the definition declares a `shopkeeper:` block,
    // the entity gains a `Shopkeeper` marker plus a `Stockpile` component
    // built from the YAML ware list. Sibling components live on the same NPC
    // entity for simplicity; the abstraction is preserved by making them
    // distinct components so admin scripts and projection code can target
    // either independently.
    //
    // Per-instance `vendor_stash` override: when the NPC's `properties`
    // carries a `vendor_stash` key naming a stash defined in the map's
    // `vendor_stashes:` list, the stash wares replace the template defaults.
    // A `vendor_stash` property also promotes a non-shopkeeper template into
    // a vendor so map authors can attach wares to any NPC without editing the
    // template metadata.
    let template_shopkeeper = definitions
        .get(&object.type_id)
        .and_then(|def| def.shopkeeper.as_ref());
    let stash_override = object
        .properties
        .get("vendor_stash")
        .and_then(|stash_id| space.find_vendor_stash(stash_id));
    if let Some(stash) = stash_override {
        commands.entity(entity).insert((
            crate::game::shop::Shopkeeper,
            crate::game::shop::Stockpile::from_wares(&stash.wares),
        ));
    } else if let Some(shopkeeper_def) = template_shopkeeper {
        commands.entity(entity).insert((
            crate::game::shop::Shopkeeper,
            crate::game::shop::Stockpile::from_def(shopkeeper_def),
        ));
    }

    // Per-instance dialog override: the editor surfaces a `dialog_id` property
    // on placed NPCs that should win over the template's `dialog_node`. Apply
    // it here so the override is in place before any TalkToNpc lookup runs.
    if let Some(dialog_id) = object.properties.get("dialog_id") {
        if !dialog_id.is_empty() {
            commands
                .entity(entity)
                .insert(crate::dialog::components::DialogNode(dialog_id.clone()));
        }
    }

    // Per-instance `hidden_dc` override: a map author tags a specific placed
    // object as starting hidden (e.g. a buried trap) by setting this property.
    // Player-driven drops never carry this property, so they never auto-hide.
    if let Some(dc_str) = object.properties.get("hidden_dc") {
        if let Ok(dc) = dc_str.parse::<u32>() {
            commands
                .entity(entity)
                .insert(crate::world::hidden::Hidden::new(dc));
        }
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
        let level = definition.and_then(|d| d.level).unwrap_or(1);
        let defense_stats = DefenseStats {
            armor: definition.map(|d| d.armor).unwrap_or(0),
            block: definition.map(|d| d.block).unwrap_or(0),
            dodge_bonus: definition.map(|d| d.dodge_bonus).unwrap_or(0),
            block_chance: definition.map(|d| d.block_chance).unwrap_or(0),
        };
        {
            let mut entity_commands = commands.entity(entity);
            entity_commands.insert((
                Npc,
                attack_profile,
                weapon_damage,
                base_stats,
                derived_stats,
                VitalStats::full(max_health, max_mana),
                defense_stats,
                crate::player::progression::Experience::at_level(level),
            ));

            // Deterministic per-NPC jitter so that NPCs don't all decrement
            // their step timers in lockstep — without this, every
            // `step_interval_seconds` boundary the entire NPC population fires
            // `update_roaming_npcs`'s search on the same frame and produces a
            // visible spike. Spreads the work across all frames in the cycle.
            let seed = object.id.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let jitter_frac = (seed % 1024) as f32 / 1024.0;

            let bounds = behavior.bounds();
            let hostile = behavior.hostile();
            let Some(npc_defaults) = definition.and_then(|d| d.npc_behavior.as_ref()) else {
                warn!(
                    "spawn for '{}' has a MapBehavior but the template has no \
                     `npc_behavior:` block — skipping behavior attachment",
                    object.type_id
                );
                return entity;
            };
            let step = npc_defaults.step_interval_seconds.max(0.05);
            let jitter = if npc_defaults.step_interval_jitter_seconds > 0.0 {
                npc_defaults.step_interval_jitter_seconds
            } else {
                step * 0.3
            };
            entity_commands.insert((
                RoamingBehavior {
                    bounds: RoamBounds {
                        min_x: bounds.min_x,
                        min_y: bounds.min_y,
                        max_x: bounds.max_x,
                        max_y: bounds.max_y,
                    },
                    step_interval_seconds: step,
                    step_interval_jitter_seconds: jitter,
                    idle_pause_chance: npc_defaults.idle_pause_chance,
                    momentum_bias: npc_defaults.momentum_bias,
                },
                RoamingStepTimer {
                    remaining_seconds: jitter_frac * step,
                },
                RoamingRandomState { seed },
                AiState::default(),
                AiMemory::default(),
            ));
            if hostile {
                let detect = npc_defaults.detect_distance_tiles.max(1);
                let disengage = npc_defaults.disengage_distance_tiles.max(detect);
                entity_commands.insert((
                    HostileBehavior {
                        detect_distance_tiles: detect,
                        disengage_distance_tiles: disengage,
                        alert_duration_seconds: npc_defaults.alert_duration_seconds,
                        requires_line_of_sight: npc_defaults.requires_line_of_sight,
                    },
                    CombatLeash {
                        max_distance_tiles: disengage,
                    },
                ));
            }
        }
    }

    entity
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
/// Container, Quantity, ObjectState) to a freshly spawned overworld-object entity.
/// Works with any receiver that exposes Bevy's `.insert(Bundle)` method — i.e.
/// `EntityCommands` or `EntityWorldMut` — which is why this is a macro: there is
/// no shared trait in Bevy.
///
/// Stateful objects (those with `initial_state`) get their `Collider` decided
/// by `colliding_for_state(initial_state)`, so a door declared with
/// `initial_state: closed` spawns colliding even when the base `colliding`
/// flag is false.
#[macro_export]
macro_rules! apply_overworld_definition_components {
    ($entity:expr, $definition:expr, $container_contents:expr, $quantity:expr) => {{
        $crate::apply_overworld_definition_components!(
            $entity,
            $definition,
            $container_contents,
            $quantity,
            ::std::option::Option::<&str>::None
        )
    }};
    ($entity:expr, $definition:expr, $container_contents:expr, $quantity:expr, $state_override:expr) => {{
        let __definition: &$crate::world::object_definitions::OverworldObjectDefinition =
            $definition;
        let __provided: Option<Vec<Option<$crate::player::components::InventoryStack>>> =
            $container_contents;
        let __quantity: Option<u32> = $quantity;
        let __state_override: ::std::option::Option<&str> = $state_override;
        let __initial_state = __state_override.or(__definition.initial_state.as_deref());
        if __definition.colliding_for_state(__initial_state) {
            $entity.insert($crate::world::components::Collider);
        }
        if let Some(__state) = __initial_state {
            $entity.insert($crate::world::components::ObjectState(__state.to_owned()));
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
        if !__definition.on_stepped.is_empty() {
            let __triggers = $crate::world::step_triggers::StepTrigger::from_def_list(
                &__definition.on_stepped,
                &__definition.name,
            );
            $entity.insert($crate::world::step_triggers::OnSteppedTriggers(__triggers));
        }
    }};
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_overworld_object(
    commands: &mut Commands,
    definitions: &OverworldObjectDefinitions,
    object_registry: &ObjectRegistry,
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

    // Honor any per-instance `state` already recorded in the registry (e.g. a
    // bear trap that was picked up sprung and is now being re-spawned). Falls
    // back to the definition's `initial_state` when no override is present.
    let state_override = object_registry
        .properties(object_id)
        .and_then(|p| p.get("state"))
        .map(String::as_str);

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

    apply_overworld_definition_components!(
        entity,
        definition,
        container_contents,
        quantity,
        state_override
    );

    entity.id()
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_client_projected_world_object(
    commands: &mut Commands,
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    object_id: u64,
    definition_id: &str,
    position: SpacePosition,
    is_npc: bool,
    state: Option<&str>,
    quantity: u32,
) -> Entity {
    let definition = definitions
        .get(definition_id)
        .unwrap_or_else(|| panic!("Missing overworld object definition for id '{definition_id}'"));
    let bundle = build_object_visual_bundle(
        asset_server,
        texture_atlas_layouts,
        definition,
        world_config,
        state,
        quantity,
    );
    let sprite_height = bundle.sprite_height;
    let mut entity_commands = commands.spawn((
        ClientProjectedWorldObject {
            object_id,
            definition_id: definition_id.to_owned(),
        },
        ViewPosition {
            space_id: position.space_id,
            tile: position.tile_position,
        },
        bundle.world_visual,
        DisplayedVitalStats::default(),
        Facing(definition.render.default_facing.unwrap_or_default()),
        bundle.sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));
    if let Some(animated) = bundle.animated {
        entity_commands.insert(animated);
    }
    if let Some(anchor) = bundle.anchor {
        entity_commands.insert(anchor);
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

#[allow(clippy::too_many_arguments)]
pub fn spawn_client_remote_player(
    commands: &mut Commands,
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    definitions: &OverworldObjectDefinitions,
    world_config: &WorldConfig,
    player_id: crate::player::components::PlayerId,
    object_id: u64,
    position: SpacePosition,
) -> Entity {
    let definition = definitions
        .get("player")
        .unwrap_or_else(|| panic!("Missing overworld object definition for id 'player'"));
    let mut bundle = build_object_visual_bundle(
        asset_server,
        texture_atlas_layouts,
        definition,
        world_config,
        None,
        1,
    );
    // Remote-player ghost tint so the local player can distinguish their own
    // entity from other connected players visually.
    bundle.sprite.color = Color::srgba(0.82, 0.92, 1.0, 0.8);
    let sprite_height = bundle.sprite_height;

    let mut entity_commands = commands.spawn((
        ClientRemotePlayerVisual {
            player_id,
            object_id,
        },
        ViewPosition {
            space_id: position.space_id,
            tile: position.tile_position,
        },
        bundle.world_visual,
        DisplayedVitalStats::default(),
        Facing(Direction::default()),
        bundle.sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));
    if let Some(animated) = bundle.animated {
        entity_commands.insert(animated);
    }
    if let Some(anchor) = bundle.anchor {
        entity_commands.insert(anchor);
    }
    let entity = entity_commands.id();

    commands.entity(entity).insert(HealthBarDisplayPolicy {
        always_visible: false,
    });
    attach_combat_health_bar(commands, entity, world_config.tile_size, sprite_height);
    entity
}

/// Sprites are bottom-anchored when their footprint sits on a tile and they
/// may rise above it. That includes y-sorted characters (NPCs, players) and
/// any block-sized object (walls, chests, barrels). Sprites that rotate
/// with facing keep the default center anchor so rotation pivots around the
/// sprite center.
pub fn bottom_anchor_for(render: &crate::world::object_definitions::RenderMetadata) -> bool {
    if render.rotation_by_facing {
        return false;
    }
    render.y_sort || render.block_size > 0
}

/// Full visual component set for an object definition. Built once per spawn
/// (gameplay and editor) so both paths render identically. The animated path
/// is preferred whenever the definition (or its current state) declares one;
/// the still-sprite path is the fallback.
pub struct ObjectVisualBundle {
    pub sprite: Sprite,
    pub world_visual: WorldVisual,
    pub animated: Option<AnimatedSprite>,
    pub anchor: Option<bevy::sprite::Anchor>,
    /// Height in pixels used for healthbar / `WorldVisual.sprite_height`.
    /// Matches the animation frame height when present so non-square sprites
    /// (e.g. 32×48) keep their proportions instead of falling back to a square
    /// from `sprite_pixel_size`.
    pub sprite_height: f32,
}

/// Single source of truth for "definition → render components". Replaces the
/// previous split between `sprite_for_definition_state_count` (still sprite)
/// and `attach_animated_sprite` (atlas swap-in). When the definition declares
/// an animation, the returned `Sprite` is already atlas-backed and an
/// `AnimatedSprite` accompanies it — no two-phase swap needed.
pub fn build_object_visual_bundle(
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    definition: &OverworldObjectDefinition,
    world_config: &WorldConfig,
    state: Option<&str>,
    count: u32,
) -> ObjectVisualBundle {
    let tile_size = world_config.tile_size;
    let effective_state = state.or(definition.initial_state.as_deref());

    let (sprite, animated, sprite_height) =
        if let Some(sheet) = definition.animation_for_state(effective_state) {
            let (animated, sprite) =
                build_animated_sprite_components(sheet, asset_server, texture_atlas_layouts);
            let height = sheet.frame_height as f32;
            (sprite, Some(animated), height)
        } else {
            let size = definition.render.sprite_pixel_size(tile_size);
            let mut sprite = if let Some(sprite_path) = definition
                .sprite_path_for_state_count(effective_state, count.max(1))
                .map(str::to_owned)
            {
                let mut sprite = Sprite::from_image(asset_server.load(sprite_path));
                sprite.custom_size = Some(size);
                sprite
            } else {
                Sprite::from_color(definition.debug_color(), size)
            };
            sprite.image_mode = SpriteImageMode::Auto;
            (sprite, None, size.y)
        };

    let world_visual = WorldVisual {
        z_index: definition.render.z_index,
        y_sort: definition.render.y_sort,
        sprite_height,
        rotation_by_facing: definition.render.rotation_by_facing,
        block_size: definition.render.block_size,
        stack_order: definition.render.stack_order,
        hide_when_inside_facing: definition.render.hide_when_inside_facing,
    };

    let anchor = if bottom_anchor_for(&definition.render) {
        Some(bevy::sprite::Anchor::BOTTOM_CENTER)
    } else {
        None
    };

    ObjectVisualBundle {
        sprite,
        world_visual,
        animated,
        anchor,
        sprite_height,
    }
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
    let profile = match definition.attack_profile.as_ref() {
        Some(def) => {
            let damage_type = def.damage_type.unwrap_or(match def.kind {
                AttackProfileKindDef::Melee => crate::combat::damage_type::DamageType::Blunt,
                AttackProfileKindDef::Ranged => crate::combat::damage_type::DamageType::Pierce,
            });
            match def.kind {
                AttackProfileKindDef::Melee => AttackProfile::melee_with(damage_type),
                AttackProfileKindDef::Ranged => {
                    let range = definition.base_range_tiles.unwrap_or(4).max(1);
                    AttackProfile::ranged_with(range, damage_type)
                }
            }
        }
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
    fn attack_profile_defaults_to_blunt_for_melee_and_pierce_for_ranged() {
        use crate::combat::components::AttackKind;
        use crate::combat::damage_type::DamageType;

        let render_block = "render:\n  z_index: 1.0\n  debug_color: [0, 0, 0]\n  debug_size: 1.0\n";

        let yaml_melee = format!(
            "name: TestSword\ndescription: A test blade\ncolliding: false\nmovable: true\nstorable: true\ndamage: \"1d6\"\nattack_profile:\n  kind: melee\n{render_block}"
        );
        let def_melee: OverworldObjectDefinition = serde_yaml::from_str(&yaml_melee).unwrap();
        let (profile, _) = attack_profile_for_definition(Some(&def_melee));
        assert_eq!(profile.kind, AttackKind::Melee);
        assert_eq!(profile.damage_type, DamageType::Blunt);

        let yaml_ranged = format!(
            "name: TestBow\ndescription: A test bow\ncolliding: false\nmovable: true\nstorable: true\ndamage: \"1d6\"\nbase_range_tiles: 5\nattack_profile:\n  kind: ranged\n{render_block}"
        );
        let def_ranged: OverworldObjectDefinition = serde_yaml::from_str(&yaml_ranged).unwrap();
        let (profile, _) = attack_profile_for_definition(Some(&def_ranged));
        assert!(matches!(profile.kind, AttackKind::Ranged { .. }));
        assert_eq!(profile.damage_type, DamageType::Pierce);

        let yaml_cut = format!(
            "name: TestSaber\ndescription: A test saber\ncolliding: false\nmovable: true\nstorable: true\ndamage: \"1d8\"\nattack_profile:\n  kind: melee\n  damage_type: cut\n{render_block}"
        );
        let def_cut: OverworldObjectDefinition = serde_yaml::from_str(&yaml_cut).unwrap();
        let (profile, _) = attack_profile_for_definition(Some(&def_cut));
        assert_eq!(profile.damage_type, DamageType::Cut);
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
