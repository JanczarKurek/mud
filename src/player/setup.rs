use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::player::components::{
    BaseStats, ChatLog, DerivedStats, Inventory, MovementCooldown, Player, PlayerId,
    PlayerIdentity, VitalStats,
};
use crate::world::components::{
    Collider, DisplayedVitalStats, HealthBarDisplayPolicy, OverworldObject, SpaceResident,
    TilePosition, WorldVisual,
};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::attach_combat_health_bar;
use crate::world::WorldConfig;

pub fn spawn_embedded_player_authoritative(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    mut object_registry: ResMut<ObjectRegistry>,
    player_query: Query<Entity, With<Player>>,
) {
    if !player_query.is_empty() {
        return;
    }

    let spawn_tile = TilePosition::new(world_config.map_width / 2, world_config.map_height / 2);
    let object_id = object_registry.allocate_runtime_id("player");
    let _ = spawn_player_authoritative(
        &mut commands,
        &world_config,
        PlayerId(0),
        object_id,
        spawn_tile,
    );
}

pub fn spawn_player_authoritative(
    commands: &mut Commands,
    _world_config: &WorldConfig,
    player_id: PlayerId,
    object_id: u64,
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
            AttackProfile::melee(),
            CombatLeash {
                max_distance_tiles: 6,
            },
            Collider,
            OverworldObject {
                object_id,
                definition_id: "player".to_owned(),
            },
            SpaceResident {
                space_id: _world_config.current_space_id,
            },
            tile_position,
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

    let entity = match player_query.single() {
        Ok(entity) => entity,
        Err(_) => commands
            .spawn((
                Player,
                VitalStats::full(1.0, 0.0),
                TilePosition::new(world_config.map_width / 2, world_config.map_height / 2),
            ))
            .id(),
    };

    commands.entity(entity).insert((
        WorldVisual {
            z_index: definition.render.z_index,
        },
        DisplayedVitalStats::default(),
        HealthBarDisplayPolicy {
            always_visible: true,
        },
        sprite,
        Transform::from_xyz(0.0, 0.0, definition.render.z_index),
    ));

    attach_combat_health_bar(&mut commands, entity, world_config.tile_size);
}
