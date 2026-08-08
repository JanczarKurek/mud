use bevy::prelude::*;

#[cfg(feature = "server-sim")]
use crate::combat::components::{AttackProfile, CombatLeash};
#[cfg(feature = "server-sim")]
use crate::crafting::CharacterStash;
#[cfg(feature = "server-sim")]
use crate::magic::effects::MagicEffects;
#[cfg(feature = "server-sim")]
use crate::npc::components::Faction;
#[cfg(feature = "server-sim")]
use crate::persistence::PlayerStateDump;
#[cfg(feature = "server-sim")]
use crate::player::classes::Class;
use crate::player::components::{
    AppearanceRegion, Player, PlayerAppearance, PlayerIdentity, SpriteLayer, VitalStats,
};
#[cfg(feature = "server-sim")]
use crate::player::components::{
    BaseStats, ChatLog, DefenseStats, DerivedStats, DiscoveredTiles, Exertion, Inventory,
    MovementCooldown, PlayerId, RegenBuffs, RegenTickers, WeaponDamage,
};
#[cfg(feature = "server-sim")]
use crate::player::progression::Experience;
#[cfg(feature = "server-sim")]
use crate::player::skills::SkillSheet;
use crate::world::components::{
    AppliedVisualDefinition, ClientRemotePlayerVisual, DisplayedVitalStats, Facing,
    HealthBarDisplayPolicy, TilePosition, ViewPosition,
};
#[cfg(feature = "server-sim")]
use crate::world::components::{Collider, OverworldObject, SpaceId, SpaceResident};
use crate::world::lighting::LightSource;
use crate::world::object_definitions::OverworldObjectDefinitions;
#[cfg(feature = "server-sim")]
use crate::world::object_registry::ObjectRegistry;
#[cfg(feature = "server-sim")]
use crate::world::resources::SpaceManager;
use crate::world::setup::{attach_combat_health_bar, build_object_visual_bundle};
use crate::world::WorldConfig;

/// Spawn the **projected** local-player entity — the client-side stand-in in
/// every client runtime (TcpClient and, since the loopback unification,
/// EmbeddedClient too). The authoritative player lives on the server side;
/// this stub exists so `spawn_player_visual` has a `Player` entity to attach
/// the sprite/health bar/light to, and `sync_projected_player_from_client_state`
/// has a target to write `ViewPosition` / `DisplayedVitalStats` / `Facing` into
/// from `ClientGameState`.
///
/// No `PlayerIdentity` — that marks the *authoritative* player entity, which
/// coexists in the same `World` in embedded mode; presentation systems filter
/// `Without<PlayerIdentity>` to address the stub unambiguously. No
/// `SpaceResident` / `TilePosition` either — those are server-authoritative
/// per the EmbeddedClient Invariant in `CLAUDE.md`. The inert `VitalStats` is
/// only here because a few client-side queries elsewhere filter on it; the
/// values are never read.
pub fn spawn_projected_local_player(
    mut commands: Commands,
    world_config: Res<WorldConfig>,
    existing: Query<Entity, (With<Player>, Without<PlayerIdentity>)>,
) {
    if existing.iter().next().is_some() {
        // Either we re-entered InGame without despawning, or another system
        // already spawned the entity. Either way, don't duplicate. (The
        // filter ignores the authoritative player an embedded App carries.)
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

/// Whether to place a loaded character at the spawn point rather than resume
/// them at their persisted location. An explicit map pick relocates them —
/// that's the point of picking. Otherwise a character with no saved space, one
/// parked at the origin (never actually placed), or one saved inside a space
/// that no longer exists gets moved. The last case is how characters saved
/// inside an *ephemeral* dungeon instance come back: the instance (and its
/// runtime space id) died with the session, and a later session may hand that
/// id to a completely different instance — resuming there would strand the
/// player in a void or teleport them into someone else's dungeon.
#[cfg(feature = "server-sim")]
pub(crate) fn needs_spawn_location(
    explicit_pick: Option<SpaceId>,
    saved_space_id: Option<SpaceId>,
    saved_tile: TilePosition,
    space_manager: &SpaceManager,
) -> bool {
    let saved_space_is_live = saved_space_id.is_some_and(|id| space_manager.get(id).is_some());
    explicit_pick.is_some() || !saved_space_is_live || (saved_tile.x == 0 && saved_tile.y == 0)
}

#[cfg(feature = "server-sim")]
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
#[cfg(feature = "server-sim")]
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
            // No `ViewPosition`: presentation lives on the projected stub in
            // every client mode; the authoritative entity is simulation-only.
            (
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

#[cfg(feature = "server-sim")]
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
            // No `ViewPosition` — see `spawn_player_from_dump`.
            (
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
    client_state: Res<crate::game::resources::ClientGameState>,
    // `Without<PlayerIdentity>`: the visual goes on the projected stub, never
    // on the authoritative player entity a unified embedded App also holds.
    player_query: Query<Entity, (With<Player>, Without<Sprite>, Without<PlayerIdentity>)>,
) {
    let entity = match player_query.single() {
        Ok(entity) => entity,
        Err(_) => {
            warn!("spawn_player_visual: no Player entity without Sprite — skipping");
            return;
        }
    };

    // Class may not have folded yet at OnEnter(InGame) — the fallback
    // `"player"` definition is used until `swap_player_visual_on_class_change`
    // rebuilds the visual once `ClientGameState.class` lands.
    let (definition_id, definition) =
        crate::world::setup::player_definition_for(&definitions, client_state.class);

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
        AppliedVisualDefinition(definition_id.to_owned()),
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

/// Rebuilds the local player's visual when the replicated class stops matching
/// the definition the sprite was built from — either because the class event
/// folded *after* `spawn_player_visual` ran (bootstrap race), or because the
/// class changed at runtime (admin `set_class`). Remote players get the same
/// treatment via the despawn-and-respawn path in
/// `sync_remote_player_projection`.
pub fn swap_player_visual_on_class_change(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    definitions: Res<OverworldObjectDefinitions>,
    world_config: Res<WorldConfig>,
    client_state: Res<crate::game::resources::ClientGameState>,
    player_query: Query<
        (
            Entity,
            &AppliedVisualDefinition,
            Option<&Children>,
            Option<&crate::world::components::CombatHealthBar>,
        ),
        (With<Player>, Without<PlayerIdentity>),
    >,
    layer_query: Query<(), With<SpriteLayer>>,
) {
    let Ok((entity, applied, children, health_bar)) = player_query.single() else {
        return;
    };
    let (definition_id, definition) =
        crate::world::setup::player_definition_for(&definitions, client_state.class);
    if applied.0 == definition_id {
        return;
    }

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

    // Tear down the visuals derived from the old definition: recolor layers
    // (rebuilt next frame by `spawn_player_recolor_layers`) and the health bar
    // (its anchor height depends on the definition's `logical_height_tiles`).
    if let Some(children) = children {
        for child in children.iter() {
            let is_layer = layer_query.get(child).is_ok();
            let is_bar_root = health_bar.is_some_and(|bar| bar.root_entity == child);
            if is_layer || is_bar_root {
                commands.entity(child).despawn();
            }
        }
    }
    commands.entity(entity).remove::<PlayerLayersInitialized>();

    commands.entity(entity).insert((
        bundle.world_visual,
        AppliedVisualDefinition(definition_id.to_owned()),
        bundle.sprite,
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
    match bundle.animated {
        Some(animated) => {
            commands.entity(entity).insert(animated);
        }
        // Swapping to a definition with no `animation:` block must drop the
        // old clip state — `attach_animated_sprite` only fills in entities
        // that lack `AnimatedSprite`, so a leftover component would keep
        // driving frame indices into a sheet that is no longer atlased.
        None => {
            commands
                .entity(entity)
                .remove::<crate::world::animation::AnimatedSprite>();
        }
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

/// Spawns one child entity per `recolor_layers` entry on the player-visual
/// definition after the entity's animated sprite + atlas have been set up by
/// `attach_animated_sprite`. Runs for both the projected local player and
/// remote-player visuals (`ClientRemotePlayerVisual`) — the local stub gets
/// its `PlayerAppearance` from `sync_player_appearance_from_client_state`,
/// remotes from `spawn_client_remote_player`. Each child shares the parent's
/// `TextureAtlasLayout` handle so frame indices line up automatically; the
/// per-region tint is applied separately by `apply_player_appearance`.
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
            &AppliedVisualDefinition,
            Has<ClientRemotePlayerVisual>,
        ),
        (
            Or<(With<Player>, With<ClientRemotePlayerVisual>)>,
            Without<PlayerLayersInitialized>,
        ),
    >,
) {
    for (entity, sprite, animated, appearance, applied, is_remote) in &player_query {
        spawn_recolor_layers_for(
            &mut commands,
            &asset_server,
            &definitions,
            entity,
            sprite,
            animated,
            appearance,
            applied,
            is_remote,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_recolor_layers_for(
    commands: &mut Commands,
    asset_server: &AssetServer,
    definitions: &OverworldObjectDefinitions,
    entity: Entity,
    sprite: &Sprite,
    animated: &crate::world::animation::AnimatedSprite,
    appearance: &PlayerAppearance,
    applied: &AppliedVisualDefinition,
    is_remote: bool,
) {
    let Some(atlas) = sprite.texture_atlas.as_ref() else {
        return;
    };
    let Some(definition) = definitions.get(&applied.0) else {
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

        let layer_color = layer_tint(appearance, region, is_remote);

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

/// Tint for one recolor layer: the appearance's region color (white for the
/// untinted skin layer), modulated by `REMOTE_GHOST_TINT` for remote-player
/// visuals so the layer stack keeps the same translucent-ghost read as the
/// base sprite.
fn layer_tint(appearance: &PlayerAppearance, region: AppearanceRegion, is_remote: bool) -> Color {
    let base = match appearance.color_for(region) {
        Some(rgb) => rgb.to_bevy(),
        None => Color::WHITE,
    };
    if is_remote {
        crate::world::setup::modulate_colors(base, crate::world::setup::REMOTE_GHOST_TINT)
    } else {
        base
    }
}

/// Copies the parent player's `AnimatedSprite` clip state onto each child
/// recolor layer so the layers stay frame-locked with the base sprite when
/// the player switches between `idle` and `walk` clips. Covers the local
/// stub and remote-player visuals alike.
pub fn propagate_player_animation_to_layers(
    player_q: Query<
        (&Children, &crate::world::animation::AnimatedSprite),
        (
            Or<(With<Player>, With<ClientRemotePlayerVisual>)>,
            Changed<crate::world::animation::AnimatedSprite>,
        ),
    >,
    mut layer_q: Query<
        &mut crate::world::animation::AnimatedSprite,
        (
            With<SpriteLayer>,
            Without<Player>,
            Without<ClientRemotePlayerVisual>,
        ),
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

/// Applies the entity's `PlayerAppearance` colors to each child recolor
/// layer's `Sprite::color`. Fires on initial appearance insert + any future
/// mutation (remote color updates, or e.g. a barber NPC in a follow-up).
pub fn apply_player_appearance(
    player_q: Query<
        (&Children, &PlayerAppearance, Has<ClientRemotePlayerVisual>),
        Changed<PlayerAppearance>,
    >,
    mut layer_q: Query<(&SpriteLayer, &mut Sprite)>,
) {
    for (children, appearance, is_remote) in &player_q {
        for child in children.iter() {
            if let Ok((layer, mut sprite)) = layer_q.get_mut(child) {
                sprite.color = layer_tint(appearance, layer.region, is_remote);
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
    fn explicit_pick_relocates_a_character_with_a_saved_position() {
        let manager = space_manager_with(&[(3, "island")]);
        assert!(needs_spawn_location(
            Some(SpaceId(1)),
            Some(SpaceId(3)),
            TilePosition::ground(37, 3),
            &manager,
        ));
    }

    #[test]
    fn no_pick_resumes_a_character_with_a_saved_position() {
        let manager = space_manager_with(&[(3, "island")]);
        assert!(!needs_spawn_location(
            None,
            Some(SpaceId(3)),
            TilePosition::ground(37, 3),
            &manager,
        ));
    }

    #[test]
    fn no_pick_still_places_unplaced_characters() {
        let manager = space_manager_with(&[(3, "island")]);
        assert!(needs_spawn_location(
            None,
            None,
            TilePosition::ground(37, 3),
            &manager
        ));
        assert!(needs_spawn_location(
            None,
            Some(SpaceId(3)),
            TilePosition::ground(0, 0),
            &manager
        ));
    }

    #[test]
    fn saved_position_in_a_dead_space_respawns() {
        // A character saved inside an ephemeral instance whose space id no
        // longer exists (or now belongs to a future instance) must not resume
        // there.
        let manager = space_manager_with(&[(3, "island")]);
        assert!(needs_spawn_location(
            None,
            Some(SpaceId(7)),
            TilePosition::ground(35, 24),
            &manager,
        ));
    }
}
