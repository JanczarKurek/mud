use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::persistence::WorldSnapshotStatus;
use crate::player::components::{
    BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, Player, PlayerId,
    PlayerIdentity, VitalStats, WeaponDamage,
};
use crate::world::components::{
    Collider, DisplayedVitalStats, HealthBarDisplayPolicy, OverworldObject, SpaceId, SpaceResident,
    TilePosition, ViewPosition,
};
use crate::world::object_definitions::{EquipmentSlot, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::attach_combat_health_bar;
use crate::world::WorldConfig;

/// Populate a fresh player's inventory with a starter shortbow + arrows so the
/// ranged-combat showcase is immediately playable.
pub fn seed_starter_inventory(inventory: &mut Inventory, object_registry: &mut ObjectRegistry) {
    let bow_id = object_registry.allocate_runtime_id("bow");
    inventory.restore_equipment_item(EquipmentSlot::Weapon, bow_id);
    let arrow_id = object_registry.allocate_runtime_id("arrow");
    inventory.set_ammo(arrow_id, 20);
}

pub fn spawn_embedded_player_authoritative(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    mut object_registry: ResMut<ObjectRegistry>,
    snapshot_status: Option<Res<WorldSnapshotStatus>>,
    player_query: Query<Option<&PlayerIdentity>, With<Player>>,
) {
    // If the snapshot loaded player entities, don't create a duplicate.
    // But if the snapshot had NO players (e.g. server saved after all clients left),
    // we still need to spawn the local player.
    if snapshot_status
        .as_ref()
        .is_some_and(|s| s.loaded && s.players_restored)
    {
        return;
    }

    // Don't spawn if any player entity already exists.
    if player_query.iter().next().is_some() {
        return;
    }

    let spawn_tile = TilePosition::ground(world_config.map_width / 2, world_config.map_height / 2);
    let object_id = object_registry.allocate_runtime_id("player");
    let entity = spawn_player_authoritative(
        &mut commands,
        &world_config,
        PlayerId(0),
        object_id,
        spawn_tile,
    );
    let mut starter = Inventory::default();
    seed_starter_inventory(&mut starter, &mut object_registry);
    commands.entity(entity).insert(starter);
}

pub fn spawn_player_authoritative(
    commands: &mut Commands,
    world_config: &WorldConfig,
    player_id: PlayerId,
    object_id: u64,
    tile_position: TilePosition,
) -> Entity {
    spawn_player_authoritative_in_space(
        commands,
        player_id,
        object_id,
        world_config.current_space_id,
        tile_position,
    )
}

pub fn spawn_player_authoritative_in_space(
    commands: &mut Commands,
    player_id: PlayerId,
    object_id: u64,
    space_id: SpaceId,
    tile_position: TilePosition,
) -> Entity {
    let base_stats = BaseStats::default();
    let derived_stats = DerivedStats::from_base(&base_stats);
    let max_health = derived_stats.max_health as f32;
    let max_mana = derived_stats.max_mana as f32;

    commands
        .spawn((
            Player,
            PlayerIdentity { id: player_id },
            Inventory::default(),
            ChatLog::default(),
            base_stats,
            derived_stats,
            VitalStats::full(max_health, max_mana),
            MovementCooldown::default(),
            (AttackProfile::melee(), WeaponDamage::default()),
            CombatLeash {
                max_distance_tiles: 6,
            },
            Collider,
            OverworldObject {
                object_id,
                definition_id: "player".to_owned(),
            },
            SpaceResident { space_id },
            tile_position,
            ViewPosition {
                space_id,
                tile: tile_position,
            },
        ))
        .id()
}

pub fn spawn_player_visual(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    player_query: Query<Entity, (With<Player>, Without<Sprite>)>,
) {
    let definition = definitions
        .get("player")
        .unwrap_or_else(|| panic!("Missing overworld object definition for id 'player'"));

    let size = definition.render.sprite_pixel_size(world_config.tile_size);

    let mut sprite = if let Some(sprite_path) = &definition.render.sprite_path {
        let mut sprite = Sprite::from_image(asset_server.load(sprite_path));
        sprite.custom_size = Some(size);
        sprite
    } else {
        Sprite::from_color(definition.debug_color(), size)
    };
    sprite.image_mode = SpriteImageMode::Auto;

    let entity = match player_query.single() {
        Ok(entity) => entity,
        Err(_) => {
            let spawn_tile =
                TilePosition::ground(world_config.map_width / 2, world_config.map_height / 2);
            commands
                .spawn((
                    Player,
                    VitalStats::full(1.0, 0.0),
                    ViewPosition {
                        space_id: world_config.current_space_id,
                        tile: spawn_tile,
                    },
                ))
                .id()
        }
    };

    let visual =
        crate::world::setup::world_visual_for_definition(definition, world_config.tile_size);
    let sprite_height = visual.sprite_height;
    let uses_y_sort = visual.y_sort;

    commands.entity(entity).insert((
        visual,
        DisplayedVitalStats::default(),
        HealthBarDisplayPolicy {
            always_visible: true,
        },
        sprite,
        Transform::from_xyz(
            0.0,
            if uses_y_sort {
                -world_config.tile_size * 0.5
            } else {
                0.0
            },
            definition.render.z_index,
        ),
    ));

    if uses_y_sort {
        commands
            .entity(entity)
            .insert(bevy::sprite::Anchor::BOTTOM_CENTER);
    }

    attach_combat_health_bar(&mut commands, entity, world_config.tile_size, sprite_height);
}
