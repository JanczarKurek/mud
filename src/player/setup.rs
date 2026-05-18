use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::crafting::CharacterStash;
use crate::magic::effects::MagicEffects;
use crate::persistence::{PlayerStateDump, WorldSnapshotStatus};
use crate::player::classes::Class;
use crate::player::components::{
    BaseStats, ChatLog, DefenseStats, DerivedStats, EquippedItem, Inventory, InventoryStack,
    MovementCooldown, Player, PlayerId, PlayerIdentity, RegenBuffs, RegenTickers, VitalStats,
    WeaponDamage,
};
use crate::player::progression::Experience;
use crate::player::skills::SkillSheet;
use crate::world::components::{
    Collider, DisplayedVitalStats, Facing, HealthBarDisplayPolicy, OverworldObject, SpaceId,
    SpaceResident, TilePosition, ViewPosition,
};
use crate::world::lighting::LightSource;
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::{EquipmentSlot, OverworldObjectDefinitions};
use crate::world::object_registry::ObjectRegistry;
use crate::world::setup::attach_combat_health_bar;
use crate::world::WorldConfig;

/// Populate a fresh player's inventory with a starter shortbow + arrows so the
/// ranged-combat showcase is immediately playable.
pub fn seed_starter_inventory(inventory: &mut Inventory) {
    inventory.restore_equipment_item(EquipmentSlot::Weapon, EquippedItem::new("bow"));
    inventory.set_ammo(EquippedItem::new("arrow"), 20);
    // Seed a handful of apples so the `demo_villager` fetch quest (turn-in
    // condition: 3 apples) is demoable without chasing items across the map.
    if let Some(slot) = inventory
        .backpack_slots
        .iter_mut()
        .find(|slot| slot.is_none())
    {
        *slot = Some(InventoryStack::item(
            "apple".to_owned(),
            ObjectProperties::new(),
            3,
        ));
    }
    // Seed enough coin to demo trading: 5 gold + 5 silver + 20 copper. Enough
    // to buy from the villager shopkeeper (apples 4c, sword 3g, armor 5g).
    for (type_id, qty) in [
        (crate::game::currency::GOLD_TYPE_ID, 5u32),
        (crate::game::currency::SILVER_TYPE_ID, 5u32),
        (crate::game::currency::COPPER_TYPE_ID, 20u32),
    ] {
        if let Some(slot) = inventory
            .backpack_slots
            .iter_mut()
            .find(|slot| slot.is_none())
        {
            *slot = Some(InventoryStack::item(
                type_id.to_owned(),
                ObjectProperties::new(),
                qty,
            ));
        }
    }
    // Seed the gathering toolkit so a new player can immediately try fishing,
    // herb-picking, and mining without first earning coin. Tools live in the
    // backpack; the player swaps one into the weapon slot to use it.
    for tool_id in ["fishing_rod", "pickaxe", "herb_knife"] {
        if let Some(slot) = inventory
            .backpack_slots
            .iter_mut()
            .find(|slot| slot.is_none())
        {
            *slot = Some(InventoryStack::item(
                tool_id.to_owned(),
                ObjectProperties::new(),
                1,
            ));
        }
    }
}

/// Spawn the **projected** local-player entity for TcpClient mode. The
/// authoritative player lives on the server; the client only carries a
/// view-side stand-in so `spawn_player_visual` has a `Player` entity to attach
/// the sprite/health bar/light to, and `sync_projected_player_from_client_state`
/// has a target to write `ViewPosition` / `DisplayedVitalStats` / `Facing` into
/// from `ClientGameState`.
///
/// No `PlayerIdentity` (that's the marker `sync_authoritative_player_display`
/// uses to identify embedded-mode entities and skip the projected branch).
/// No `SpaceResident` / `TilePosition` either — those are server-authoritative
/// per the EmbeddedClient Invariant in `CLAUDE.md`. The inert `VitalStats` is
/// only here because a few server-side queries elsewhere filter on it; the
/// values are never read on the client.
pub fn spawn_projected_local_player(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    existing: Query<Entity, With<Player>>,
) {
    if existing.iter().next().is_some() {
        // Either we re-entered InGame without despawning, or another system
        // already spawned the entity. Either way, don't duplicate.
        return;
    }
    commands.spawn((
        Player,
        ViewPosition {
            space_id: world_config.current_space_id,
            tile: TilePosition::ground(0, 0),
        },
        DisplayedVitalStats::default(),
        Facing::default(),
        VitalStats::full(1.0, 0.0),
    ));
}

/// Despawn the projected local-player entity (and any sprite/visual it ended
/// up carrying) when exiting `InGame`. Without this, logging out and back in
/// leaves a stale entity that the next `spawn_projected_local_player` then
/// short-circuits on, leaving the new session pointing at the previous run's
/// view state.
pub fn despawn_projected_local_player(
    mut commands: Commands,
    query: Query<Entity, (With<Player>, Without<PlayerIdentity>)>,
) {
    for entity in &query {
        commands.entity(entity).despawn();
    }
}

pub fn spawn_embedded_player_authoritative(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    mut object_registry: ResMut<ObjectRegistry>,
    snapshot_status: Option<Res<WorldSnapshotStatus>>,
    player_query: Query<Option<&PlayerIdentity>, With<Player>>,
    db: Option<Res<crate::accounts::AccountDbHandle>>,
    mut var_stores: Option<ResMut<crate::dialog::resources::CharacterVarStores>>,
    selected: Option<Res<crate::app::state::LocalSelectedCharacter>>,
) {
    if snapshot_status
        .as_ref()
        .is_some_and(|s| s.loaded && s.players_restored)
    {
        return;
    }

    if player_query.iter().next().is_some() {
        warn!(
            "spawn_embedded_player_authoritative: existing Player entity present on InGame entry — cleanup leak?"
        );
        return;
    }

    let Some(db) = db.as_deref() else {
        return;
    };

    // Prefer the character explicitly chosen on the CharacterSelect screen.
    // Fall back to "most recently played" if nothing's been chosen yet.
    let target_character_id = selected.as_ref().and_then(|s| s.character_id);

    let (character_id, dump, display_name) = {
        let guard = db.lock();
        let summary = match target_character_id {
            Some(id) => guard
                .list_characters(crate::accounts::LOCAL_ACCOUNT_ID)
                .unwrap_or_default()
                .into_iter()
                .find(|c| c.character_id == id),
            None => guard
                .list_characters(crate::accounts::LOCAL_ACCOUNT_ID)
                .unwrap_or_default()
                .into_iter()
                .next(),
        };
        let Some(summary) = summary else {
            return;
        };
        let dump = guard.load_character(summary.character_id).ok().flatten();
        (summary.character_id, dump, summary.name)
    };

    let player_id = PlayerId(character_id as u64);
    if let Some(mut dump) = dump {
        dump.player_id = player_id;
        let needs_spawn_location =
            dump.space_id.is_none() || (dump.tile_position.x == 0 && dump.tile_position.y == 0);
        if needs_spawn_location {
            dump.space_id = Some(world_config.current_space_id);
            dump.tile_position =
                TilePosition::ground(world_config.map_width / 2, world_config.map_height / 2);
        }
        let yarn_vars = dump.yarn_vars.clone();
        let needs_starter_seed = dump
            .inventory
            .backpack_slots
            .iter()
            .all(|slot| slot.is_none())
            && dump
                .inventory
                .equipment_slots
                .iter()
                .all(|(_, item)| item.is_none());
        let fallback_space_id = world_config.current_space_id;
        let entity = spawn_player_from_dump(
            &mut commands,
            &mut object_registry,
            dump,
            fallback_space_id,
            display_name,
        );
        if needs_starter_seed {
            let mut starter = Inventory::default();
            seed_starter_inventory(&mut starter);
            commands.entity(entity).insert(starter);
        }
        if let Some(stores) = var_stores.as_deref_mut() {
            stores.restore(player_id.0, yarn_vars);
        }
        return;
    }

    let spawn_tile = TilePosition::ground(world_config.map_width / 2, world_config.map_height / 2);
    let object_id = object_registry.allocate_runtime_id("player");
    let entity = spawn_player_authoritative(
        &mut commands,
        &world_config,
        player_id,
        object_id,
        spawn_tile,
        display_name,
    );
    let mut starter = Inventory::default();
    seed_starter_inventory(&mut starter);
    commands.entity(entity).insert(starter);
}

pub fn spawn_player_authoritative(
    commands: &mut Commands,
    world_config: &WorldConfig,
    player_id: PlayerId,
    object_id: u64,
    tile_position: TilePosition,
    display_name: String,
) -> Entity {
    spawn_player_authoritative_in_space(
        commands,
        player_id,
        object_id,
        world_config.current_space_id,
        tile_position,
        display_name,
    )
}

/// Spawn a player entity from a previously-persisted `PlayerStateDump` (restored
/// from an account DB row or a world snapshot). Allocates a fresh runtime
/// `object_id` — runtime ids are opaque and not preserved across loads.
pub fn spawn_player_from_dump(
    commands: &mut Commands,
    object_registry: &mut ObjectRegistry,
    dump: PlayerStateDump,
    fallback_space_id: SpaceId,
    display_name: String,
) -> Entity {
    let space_id = dump.space_id.unwrap_or(fallback_space_id);
    let mut inventory = dump.inventory;
    inventory.ensure_slots();
    let object_id = object_registry.allocate_runtime_id("player");
    let stash = CharacterStash {
        entries: dump.stash,
    };

    let entity = commands
        .spawn((
            Player,
            PlayerIdentity {
                id: dump.player_id,
                display_name,
                home_position: dump.home_position,
            },
            inventory,
            dump.chat_log,
            dump.base_stats,
            dump.derived_stats,
            dump.vital_stats,
            dump.movement_cooldown,
            (
                dump.attack_profile,
                WeaponDamage::default(),
                DefenseStats::default(),
            ),
            (
                dump.combat_leash,
                RegenTickers::default(),
                RegenBuffs::default(),
                dump.magic_effects,
                stash,
            ),
            Collider,
            OverworldObject {
                object_id,
                definition_id: "player".to_owned(),
            },
            SpaceResident { space_id },
            dump.tile_position,
            (
                ViewPosition {
                    space_id,
                    tile: dump.tile_position,
                },
                Facing(dump.facing),
                dump.experience,
                dump.class,
                dump.skill_sheet,
            ),
        ))
        .id();
    entity
}

pub fn spawn_player_authoritative_in_space(
    commands: &mut Commands,
    player_id: PlayerId,
    object_id: u64,
    space_id: SpaceId,
    tile_position: TilePosition,
    display_name: String,
) -> Entity {
    let base_stats = BaseStats::default();
    let derived_stats = DerivedStats::from_base(&base_stats);
    let max_health = derived_stats.max_health as f32;
    let max_mana = derived_stats.max_mana as f32;

    commands
        .spawn((
            Player,
            PlayerIdentity {
                id: player_id,
                display_name,
                home_position: None,
            },
            Inventory::default(),
            ChatLog::default(),
            base_stats,
            derived_stats,
            VitalStats::full(max_health, max_mana),
            MovementCooldown::default(),
            (
                AttackProfile::melee(),
                WeaponDamage::default(),
                DefenseStats::default(),
            ),
            (
                CombatLeash {
                    max_distance_tiles: 6,
                },
                RegenTickers::default(),
                RegenBuffs::default(),
                MagicEffects::default(),
                CharacterStash::default(),
            ),
            Collider,
            OverworldObject {
                object_id,
                definition_id: "player".to_owned(),
            },
            SpaceResident { space_id },
            tile_position,
            (
                ViewPosition {
                    space_id,
                    tile: tile_position,
                },
                Facing::default(),
                Experience::default(),
                Class::default(),
                SkillSheet::default(),
            ),
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
    let entity = match player_query.single() {
        Ok(entity) => entity,
        Err(_) => {
            warn!("spawn_player_visual: no Player entity without Sprite — skipping");
            return;
        }
    };

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
        // Baseline player vision: warm-white, dim ~1.5-tile halo. Always on
        // so dark spaces stay navigable, but tuned low enough that in
        // daylight (curve alpha=0) the shader-clamped subtraction makes the
        // aura visually invisible without any conditional logic.
        LightSource::new([1.0, 0.92, 0.78], 1.5, 0.18),
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
