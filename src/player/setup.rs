use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::crafting::CharacterStash;
use crate::magic::effects::MagicEffects;
use crate::npc::components::Faction;
use crate::persistence::{PlayerStateDump, WorldSnapshotStatus};
use crate::player::classes::Class;
use crate::player::components::{
    AppearanceRegion, BaseStats, ChatLog, DefenseStats, DerivedStats, DiscoveredTiles, Exertion,
    Inventory, MovementCooldown, Player, PlayerAppearance, PlayerId, PlayerIdentity, RegenBuffs,
    RegenTickers, SpriteLayer, VitalStats, WeaponDamage,
};
use crate::player::loadout::StartingLoadout;
use crate::player::progression::Experience;
use crate::player::skills::SkillSheet;
use crate::world::components::{
    Collider, DisplayedVitalStats, Facing, HealthBarDisplayPolicy, OverworldObject, SpaceId,
    SpaceResident, TilePosition, ViewPosition,
};
use crate::world::lighting::LightSource;
use crate::world::map_layout::SpaceDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::resources::SpaceManager;
use crate::world::setup::{attach_combat_health_bar, build_object_visual_bundle};
use crate::world::WorldConfig;

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

/// Where a character enters the world, given an already-resolved title-screen
/// pick. Falls back to the authored bootstrap space, and only then to
/// `current_space_id` — which tracks whichever space the player last stood in
/// (`sync_client_world_projection`) and is persisted in the world snapshot, so
/// it drifts off the bootstrap space and is a last resort, not the default.
fn resolve_spawn_space(
    space_manager: &SpaceManager,
    bootstrap_space_id: &str,
    explicit_pick: Option<SpaceId>,
    current_space_id: SpaceId,
) -> SpaceId {
    explicit_pick
        .or_else(|| space_manager.persistent_space_id(bootstrap_space_id))
        .unwrap_or(current_space_id)
}

/// Whether to place a loaded character at the spawn point rather than resume
/// them at their persisted location. An explicit map pick relocates them —
/// that's the point of picking. Otherwise only a character with no saved space,
/// or one parked at the origin (never actually placed), gets moved.
fn needs_spawn_location(
    explicit_pick: Option<SpaceId>,
    saved_space_id: Option<SpaceId>,
    saved_tile: TilePosition,
) -> bool {
    explicit_pick.is_some() || saved_space_id.is_none() || (saved_tile.x == 0 && saved_tile.y == 0)
}

pub fn spawn_embedded_player_authoritative(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    space_manager: Res<SpaceManager>,
    mut object_registry: ResMut<ObjectRegistry>,
    snapshot_status: Option<Res<WorldSnapshotStatus>>,
    player_query: Query<Option<&PlayerIdentity>, With<Player>>,
    db: Option<Res<crate::accounts::AccountDbHandle>>,
    mut var_stores: Option<ResMut<crate::dialog::resources::CharacterVarStores>>,
    selected: Option<Res<crate::app::state::LocalSelectedCharacter>>,
    mut selected_map: Option<ResMut<crate::ui::settings::SelectedStartingMap>>,
    space_definitions: Res<SpaceDefinitions>,
    loadout: Res<StartingLoadout>,
) {
    if snapshot_status
        .as_ref()
        .is_some_and(|s| s.loaded && s.players_restored)
    {
        return;
    }

    // Resolve the map this character spawns in.
    //
    // An explicit title-screen pick (`SelectedStartingMap::map_id`) overrides a
    // returning character's saved position — relocating is the whole point of
    // picking. The pick is *consumed* here, so the next launch resumes the
    // character wherever they left off instead of yanking them back.
    //
    // With no pick, a returning character resumes at their saved position and a
    // new one starts in the authored bootstrap space. Note the fallback resolves
    // `bootstrap_space_id` through `SpaceManager` rather than reading
    // `world_config.current_space_id`: the latter tracks whichever space the
    // player last stood in (`sync_client_world_projection`) and is persisted in
    // the world snapshot, so it drifts off the bootstrap space over time.
    let explicit_map_id = selected_map.as_ref().and_then(|s| s.map_id.clone());
    let explicit_pick = explicit_map_id
        .as_deref()
        .and_then(|id| space_manager.persistent_space_id(id));
    if let Some(map_id) = &explicit_map_id {
        if explicit_pick.is_none() {
            warn!(
                "starting map '{map_id}' is not a live persistent space; falling back to bootstrap space '{}'",
                space_definitions.bootstrap_space_id
            );
        }
        if let Some(selected_map) = selected_map.as_mut() {
            selected_map.map_id = None;
            selected_map.dirty = true;
        }
    }
    let spawn_space_id = resolve_spawn_space(
        &space_manager,
        &space_definitions.bootstrap_space_id,
        explicit_pick,
        world_config.current_space_id,
    );
    let (spawn_width, spawn_height) = space_manager
        .get(spawn_space_id)
        .map(|space| (space.width, space.height))
        .unwrap_or((world_config.map_width, world_config.map_height));

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
            needs_spawn_location(explicit_pick, dump.space_id, dump.tile_position);
        if needs_spawn_location {
            dump.space_id = Some(spawn_space_id);
            dump.tile_position = TilePosition::ground(spawn_width / 2, spawn_height / 2);
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
        let fallback_space_id = spawn_space_id;
        let entity = spawn_player_from_dump(
            &mut commands,
            &mut object_registry,
            dump,
            fallback_space_id,
            display_name,
        );
        if needs_starter_seed {
            let mut starter = Inventory::default();
            loadout.apply_to(&mut starter);
            commands.entity(entity).insert(starter);
        }
        if let Some(stores) = var_stores.as_deref_mut() {
            stores.restore(player_id.0, yarn_vars);
        }
        return;
    }

    let spawn_tile = TilePosition::ground(spawn_width / 2, spawn_height / 2);
    let object_id = object_registry.allocate_runtime_id("player");
    let entity = spawn_player_authoritative_in_space(
        &mut commands,
        player_id,
        object_id,
        spawn_space_id,
        spawn_tile,
        display_name,
    );
    let mut starter = Inventory::default();
    loadout.apply_to(&mut starter);
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

    // A character can be persisted at HP 0 if they disconnected while dead and
    // awaiting respawn (the `AwaitingRespawn` marker is session-only). Regen is
    // gated off at `health <= 0`, so a reloaded 0-HP player with no overlay would
    // soft-lock. Clamp to alive on load — this also repairs any legacy 0-HP save.
    let mut vital_stats = dump.vital_stats;
    if vital_stats.health < 1.0 {
        vital_stats.health = vital_stats.max_health.max(1.0);
    }
    let object_id = object_registry.allocate_runtime_id("player");
    let stash = CharacterStash {
        entries: dump.stash,
    };

    let mut discovered = DiscoveredTiles::default();
    for (space, tiles) in dump.discovered_tiles {
        discovered
            .by_space
            .insert(space, tiles.into_iter().collect());
    }

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
            vital_stats,
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
                Exertion::default(),
                dump.magic_effects,
                stash,
                Faction::PlayerSide,
            ),
            Collider,
            OverworldObject {
                object_id,
                definition_id: "player".to_owned(),
                placement_seq: 0,
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
                dump.appearance,
                discovered,
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
                Exertion::default(),
                MagicEffects::default(),
                CharacterStash::default(),
                Faction::PlayerSide,
            ),
            Collider,
            OverworldObject {
                object_id,
                definition_id: "player".to_owned(),
                placement_seq: 0,
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
                DiscoveredTiles::default(),
                PlayerAppearance::default(),
            ),
        ))
        .id()
}

pub fn spawn_player_visual(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
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

    let bundle = build_object_visual_bundle(
        &asset_server,
        &mut texture_atlas_layouts,
        definition,
        &world_config,
        None,
        1,
    );
    let hud_anchor_height = bundle.hud_anchor_height;
    let uses_y_sort = bundle.world_visual.y_sort;

    commands.entity(entity).insert((
        bundle.world_visual,
        DisplayedVitalStats::default(),
        HealthBarDisplayPolicy {
            always_visible: true,
        },
        bundle.sprite,
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

    if let Some(animated) = bundle.animated {
        commands.entity(entity).insert(animated);
    }
    if let Some(anchor) = bundle.anchor {
        commands.entity(entity).insert(anchor);
    }

    attach_combat_health_bar(
        &mut commands,
        entity,
        world_config.tile_size,
        hud_anchor_height,
    );
}

/// Marker inserted on the player entity once its recolor sprite layers have
/// been spawned. Gates `spawn_player_recolor_layers` from running twice.
#[derive(Component)]
pub struct PlayerLayersInitialized;

/// Spawns one child entity per `recolor_layers` entry on the player definition
/// after the player's animated sprite + atlas have been set up by
/// `attach_animated_sprite`. Each child shares the parent's `TextureAtlasLayout`
/// handle so frame indices line up automatically; the per-region tint is
/// applied separately by `apply_player_appearance`.
pub fn spawn_player_recolor_layers(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    definitions: Res<OverworldObjectDefinitions>,
    player_query: Query<
        (
            Entity,
            &Sprite,
            &crate::world::animation::AnimatedSprite,
            &PlayerAppearance,
        ),
        (With<Player>, Without<PlayerLayersInitialized>),
    >,
) {
    let Ok((entity, sprite, animated, appearance)) = player_query.single() else {
        return;
    };
    let Some(atlas) = sprite.texture_atlas.as_ref() else {
        return;
    };
    let Some(definition) = definitions.get("player") else {
        return;
    };
    if definition.render.recolor_layers.is_empty() {
        commands.entity(entity).insert(PlayerLayersInitialized);
        return;
    }

    // Match the base sprite's `custom_size` exactly. `attach_animated_sprite`
    // sizes the base to the animation sheet's frame_width/frame_height, which
    // is asymmetric (e.g. 32×48). Using `sprite_pixel_size` here would fall
    // back to a square (tile_size * debug_size), stretching the layer wider
    // than the base and clipping the base sprite's hands behind the wider
    // torso layer.
    let size = sprite.custom_size.unwrap_or_else(|| {
        Vec2::new(
            definition
                .render
                .animation
                .as_ref()
                .map(|a| a.frame_width as f32)
                .unwrap_or(0.0),
            definition
                .render
                .animation
                .as_ref()
                .map(|a| a.frame_height as f32)
                .unwrap_or(0.0),
        )
    });
    let uses_y_sort = definition.render.y_sort;
    let layout_handle = atlas.layout.clone();

    for (idx, layer) in definition.render.recolor_layers.iter().enumerate() {
        let region = match layer.key.as_str() {
            "skin" => AppearanceRegion::Skin,
            "hair" => AppearanceRegion::Hair,
            "torso" => AppearanceRegion::Torso,
            "trousers" => AppearanceRegion::Trousers,
            other => {
                warn!("unknown recolor layer key '{other}' on player definition — skipping");
                continue;
            }
        };

        let layer_color = match appearance.color_for(region) {
            Some(rgb) => rgb.to_bevy(),
            None => Color::WHITE,
        };

        let layer_sprite = Sprite {
            image: asset_server.load(&layer.sheet_path),
            custom_size: Some(size),
            texture_atlas: Some(TextureAtlas {
                layout: layout_handle.clone(),
                index: atlas.index,
            }),
            color: layer_color,
            image_mode: SpriteImageMode::Auto,
            ..default()
        };

        // Stack each layer slightly above the previous one (and above the
        // base sprite) so they composite in declaration order. The base
        // sprite sits 0.005 below `y_sort_z(tile_y)` (see `sync_player_z`)
        // so that world objects on the same tile_y render in front of the
        // player; layer offsets must fit inside that 0.005 epsilon or
        // clothes will appear on top of an occluder while the base sprite
        // is correctly hidden.
        let z_offset = 0.001 * (idx as f32 + 1.0);

        let mut layer_entity = commands.spawn((
            layer_sprite,
            animated.clone(),
            SpriteLayer { region },
            Transform::from_xyz(0.0, 0.0, z_offset),
            Visibility::Inherited,
        ));
        if uses_y_sort {
            layer_entity.insert(bevy::sprite::Anchor::BOTTOM_CENTER);
        }
        let layer_id = layer_entity.id();
        commands.entity(entity).add_child(layer_id);
    }

    commands.entity(entity).insert(PlayerLayersInitialized);
}

/// Copies the parent player's `AnimatedSprite` clip state onto each child
/// recolor layer so the layers stay frame-locked with the base sprite when
/// the player switches between `idle` and `walk` clips.
pub fn propagate_player_animation_to_layers(
    player_q: Query<
        (&Children, &crate::world::animation::AnimatedSprite),
        (
            With<Player>,
            Changed<crate::world::animation::AnimatedSprite>,
        ),
    >,
    mut layer_q: Query<
        &mut crate::world::animation::AnimatedSprite,
        (With<SpriteLayer>, Without<Player>),
    >,
) {
    for (children, parent_anim) in &player_q {
        for child in children.iter() {
            if let Ok(mut child_anim) = layer_q.get_mut(child) {
                *child_anim = parent_anim.clone();
            }
        }
    }
}

/// Applies the player's `PlayerAppearance` colors to each child recolor
/// layer's `Sprite::color`. Fires on initial appearance insert + any future
/// mutation (e.g. a barber NPC in a follow-up).
pub fn apply_player_appearance(
    player_q: Query<(&Children, &PlayerAppearance), Changed<PlayerAppearance>>,
    mut layer_q: Query<(&SpriteLayer, &mut Sprite)>,
) {
    for (children, appearance) in &player_q {
        for child in children.iter() {
            if let Ok((layer, mut sprite)) = layer_q.get_mut(child) {
                sprite.color = match appearance.color_for(layer.region) {
                    Some(rgb) => rgb.to_bevy(),
                    None => Color::WHITE,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map_layout::SpacePermanence;
    use crate::world::resources::RuntimeSpace;

    fn space_manager_with(spaces: &[(u64, &str)]) -> SpaceManager {
        let mut manager = SpaceManager::default();
        for (id, authored_id) in spaces {
            manager.insert_space(RuntimeSpace {
                id: SpaceId(*id),
                authored_id: (*authored_id).to_owned(),
                width: 32,
                height: 32,
                fill_floor_type: "grass".to_owned(),
                permanence: SpacePermanence::Persistent,
                instance_owner: None,
                lighting: default(),
            });
        }
        manager
    }

    #[test]
    fn spawn_space_prefers_explicit_pick() {
        let manager = space_manager_with(&[(1, "overworld"), (3, "island")]);
        let picked = resolve_spawn_space(&manager, "overworld", Some(SpaceId(3)), SpaceId(3));
        assert_eq!(picked, SpaceId(3));
    }

    /// Regression: `current_space_id` follows the player between spaces and is
    /// written into the world snapshot, so a snapshot saved while standing on
    /// `island` must not become the spawn point for pick-less characters.
    #[test]
    fn spawn_space_falls_back_to_bootstrap_not_drifted_current_space() {
        let manager = space_manager_with(&[(1, "overworld"), (3, "island")]);
        let drifted = SpaceId(3);
        assert_eq!(
            resolve_spawn_space(&manager, "overworld", None, drifted),
            SpaceId(1)
        );
    }

    #[test]
    fn spawn_space_falls_back_to_current_space_when_bootstrap_is_absent() {
        let manager = space_manager_with(&[(3, "island")]);
        assert_eq!(
            resolve_spawn_space(&manager, "overworld", None, SpaceId(3)),
            SpaceId(3)
        );
    }

    #[test]
    fn explicit_pick_relocates_a_character_with_a_saved_position() {
        assert!(needs_spawn_location(
            Some(SpaceId(1)),
            Some(SpaceId(3)),
            TilePosition::ground(37, 3),
        ));
    }

    #[test]
    fn no_pick_resumes_a_character_with_a_saved_position() {
        assert!(!needs_spawn_location(
            None,
            Some(SpaceId(3)),
            TilePosition::ground(37, 3),
        ));
    }

    #[test]
    fn no_pick_still_places_unplaced_characters() {
        assert!(needs_spawn_location(
            None,
            None,
            TilePosition::ground(37, 3)
        ));
        assert!(needs_spawn_location(
            None,
            Some(SpaceId(3)),
            TilePosition::ground(0, 0)
        ));
    }
}
