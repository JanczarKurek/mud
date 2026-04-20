use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::Player;
use crate::world::components::{ClientProjectedWorldObject, TilePosition};
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::resources::ViewScrollOffset;
use crate::world::WorldConfig;

// ── Components ────────────────────────────────────────────────────────────────

/// Drives atlas frame cycling for entities that have a sprite-sheet animation.
#[derive(Component, Clone, Debug)]
pub struct AnimatedSprite {
    pub current_clip: String,
    pub frame_index: u32,
    pub frame_timer: f32,
    // Fields below are cached from the clip def whenever a clip transition occurs.
    pub frame_count: u32,
    pub seconds_per_frame: f32,
    pub atlas_columns: u32,
    pub clip_row: u32,
    pub clip_start_col: u32,
    pub looping: bool,
}

/// Inserted on an entity for exactly one Update frame after its TilePosition
/// changes. Consumed by `trigger_movement_animation` and removed by
/// `cleanup_just_moved`.
#[derive(Component, Clone, Copy, Debug)]
pub struct JustMoved {
    pub dx: i32,
    pub dy: i32,
}

/// Persisted facing direction, updated whenever the entity moves.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FacingDirection {
    pub dx: i32,
    pub dy: i32,
}

/// Per-entity pixel offset (in world space) that lerps toward zero after a
/// move. Added to the entity's tile-based translation by `sync_tile_transforms`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct VisualOffset {
    pub current: Vec2,
    pub elapsed: f32,
    pub duration: f32,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn apply_clip(
    animated: &mut AnimatedSprite,
    clip_name: &str,
    atlas_columns: u32,
    row: u32,
    start_col: u32,
    frame_count: u32,
    fps: f32,
    looping: bool,
) {
    animated.current_clip = clip_name.to_string();
    animated.frame_index = 0;
    animated.frame_timer = 0.0;
    animated.frame_count = frame_count;
    animated.seconds_per_frame = if fps > 0.0 { 1.0 / fps } else { 1.0 };
    animated.atlas_columns = atlas_columns;
    animated.clip_row = row;
    animated.clip_start_col = start_col;
    animated.looping = looping;
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Attaches `AnimatedSprite` + `FacingDirection` to newly spawned entities
/// whose object definition has an `animation:` block, and swaps their `Sprite`
/// to use a `TextureAtlas`.
pub fn attach_animated_sprite(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    definitions: Res<OverworldObjectDefinitions>,
    // World objects without AnimatedSprite yet
    world_objects: Query<(Entity, &ClientProjectedWorldObject), Without<AnimatedSprite>>,
    // Local player without AnimatedSprite yet
    player_query: Query<(Entity, &Sprite), (With<Player>, Without<AnimatedSprite>)>,
) {
    let try_attach = |entity: Entity,
                      definition_id: &str,
                      commands: &mut Commands,
                      asset_server: &AssetServer,
                      texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
                      definitions: &OverworldObjectDefinitions| {
        let Some(def) = definitions.get(definition_id) else {
            return;
        };
        let Some(sheet) = &def.render.animation else {
            return;
        };

        let layout = TextureAtlasLayout::from_grid(
            UVec2::new(sheet.frame_width, sheet.frame_height),
            sheet.sheet_columns,
            sheet.sheet_rows,
            None,
            None,
        );
        let layout_handle = texture_atlas_layouts.add(layout);

        let image_handle: Handle<Image> = asset_server.load(&sheet.sheet_path);

        // Build initial animated sprite state (idle clip).
        let idle_clip = sheet.clips.get("idle");
        let animated = AnimatedSprite {
            current_clip: "idle".to_string(),
            frame_index: 0,
            frame_timer: 0.0,
            frame_count: idle_clip.map_or(1, |c| c.frame_count),
            seconds_per_frame: idle_clip
                .map_or(1.0, |c| if c.fps > 0.0 { 1.0 / c.fps } else { 1.0 }),
            atlas_columns: sheet.sheet_columns,
            clip_row: idle_clip.map_or(0, |c| c.row),
            clip_start_col: idle_clip.map_or(0, |c| c.start_col),
            looping: idle_clip.is_none_or(|c| c.looping),
        };

        let new_sprite = Sprite {
            image: image_handle,
            custom_size: Some(Vec2::new(
                sheet.frame_width as f32,
                sheet.frame_height as f32,
            )),
            texture_atlas: Some(TextureAtlas {
                layout: layout_handle,
                index: 0,
            }),
            ..default()
        };

        commands
            .entity(entity)
            .insert((animated, FacingDirection::default(), new_sprite));
    };

    for (entity, world_obj) in &world_objects {
        try_attach(
            entity,
            &world_obj.definition_id,
            &mut commands,
            &asset_server,
            &mut texture_atlas_layouts,
            &definitions,
        );
    }

    for (entity, _sprite) in &player_query {
        try_attach(
            entity,
            "player",
            &mut commands,
            &asset_server,
            &mut texture_atlas_layouts,
            &definitions,
        );
    }
}

/// Advances frame timers and writes the current atlas index into each
/// `AnimatedSprite`'s `Sprite` component.
pub fn advance_animation_timers(
    time: Res<Time>,
    mut query: Query<(&mut AnimatedSprite, &mut Sprite)>,
) {
    for (mut animated, mut sprite) in &mut query {
        if animated.frame_count == 0 {
            continue;
        }

        animated.frame_timer += time.delta_secs();
        if animated.frame_timer >= animated.seconds_per_frame {
            animated.frame_timer -= animated.seconds_per_frame;
            if animated.looping {
                animated.frame_index = (animated.frame_index + 1) % animated.frame_count;
            } else {
                animated.frame_index =
                    (animated.frame_index + 1).min(animated.frame_count.saturating_sub(1));
            }
        }

        if let Some(atlas) = sprite.texture_atlas.as_mut() {
            atlas.index = (animated.clip_row * animated.atlas_columns
                + animated.clip_start_col
                + animated.frame_index) as usize;
        }
    }
}

/// Switches animated entities to their walk clip when they have `JustMoved`.
pub fn trigger_movement_animation(
    definitions: Res<OverworldObjectDefinitions>,
    mut world_obj_query: Query<(
        &mut AnimatedSprite,
        &mut FacingDirection,
        &ClientProjectedWorldObject,
        &JustMoved,
    )>,
    mut player_query: Query<
        (&mut AnimatedSprite, &mut FacingDirection, &JustMoved),
        (With<Player>, Without<ClientProjectedWorldObject>),
    >,
) {
    let try_walk = |animated: &mut AnimatedSprite,
                    facing: &mut FacingDirection,
                    just_moved: &JustMoved,
                    clips: &std::collections::HashMap<
        String,
        crate::world::object_definitions::AnimationClipDef,
    >,
                    atlas_columns: u32| {
        facing.dx = just_moved.dx;
        facing.dy = just_moved.dy;

        let clip_name = if clips.contains_key("walk") {
            "walk"
        } else {
            "idle"
        };

        if let Some(clip) = clips.get(clip_name) {
            apply_clip(
                animated,
                clip_name,
                atlas_columns,
                clip.row,
                clip.start_col,
                clip.frame_count,
                clip.fps,
                clip.looping,
            );
        }
    };

    for (mut animated, mut facing, world_obj, just_moved) in &mut world_obj_query {
        let Some(def) = definitions.get(&world_obj.definition_id) else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        try_walk(
            &mut animated,
            &mut facing,
            just_moved,
            &sheet.clips,
            sheet.sheet_columns,
        );
    }

    for (mut animated, mut facing, just_moved) in &mut player_query {
        let Some(def) = definitions.get("player") else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        try_walk(
            &mut animated,
            &mut facing,
            just_moved,
            &sheet.clips,
            sheet.sheet_columns,
        );
    }
}

/// Transitions animated entities back to idle when they no longer have `JustMoved`.
pub fn return_to_idle_animation(
    definitions: Res<OverworldObjectDefinitions>,
    mut world_obj_query: Query<
        (&mut AnimatedSprite, &ClientProjectedWorldObject),
        Without<JustMoved>,
    >,
    mut player_query: Query<
        &mut AnimatedSprite,
        (
            With<Player>,
            Without<JustMoved>,
            Without<ClientProjectedWorldObject>,
        ),
    >,
) {
    let try_idle = |animated: &mut AnimatedSprite,
                    clips: &std::collections::HashMap<
        String,
        crate::world::object_definitions::AnimationClipDef,
    >,
                    atlas_columns: u32| {
        if !animated.current_clip.starts_with("walk") {
            return;
        }
        if let Some(clip) = clips.get("idle") {
            apply_clip(
                animated,
                "idle",
                atlas_columns,
                clip.row,
                clip.start_col,
                clip.frame_count,
                clip.fps,
                clip.looping,
            );
        }
    };

    for (mut animated, world_obj) in &mut world_obj_query {
        let Some(def) = definitions.get(&world_obj.definition_id) else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        let cols = sheet.sheet_columns;
        try_idle(&mut animated, &sheet.clips, cols);
    }

    for mut animated in &mut player_query {
        let Some(def) = definitions.get("player") else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        let cols = sheet.sheet_columns;
        try_idle(&mut animated, &sheet.clips, cols);
    }
}

/// Removes the one-frame `JustMoved` marker from all entities.
pub fn cleanup_just_moved(mut commands: Commands, query: Query<Entity, With<JustMoved>>) {
    for entity in &query {
        commands.entity(entity).remove::<JustMoved>();
    }
}

/// Advances the player-movement-driven viewport scroll offset toward zero.
pub fn tick_view_scroll(time: Res<Time>, mut offset: ResMut<ViewScrollOffset>) {
    if offset.duration <= 0.0 || offset.current == Vec2::ZERO {
        return;
    }
    offset.elapsed += time.delta_secs();
    // Decay at a constant rate proportional to 1/duration. Over the full duration
    // this reduces the offset to near zero; we then snap to exactly zero.
    let decay = time.delta_secs() / offset.duration;
    offset.current *= 1.0 - decay.min(1.0);
    if offset.elapsed >= offset.duration {
        offset.current = Vec2::ZERO;
    }
}

/// Advances per-entity visual offsets toward zero.
pub fn tick_visual_offsets(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut VisualOffset)>,
) {
    for (entity, mut offset) in &mut query {
        if offset.duration <= 0.0 {
            commands.entity(entity).remove::<VisualOffset>();
            continue;
        }
        offset.elapsed += time.delta_secs();
        let decay = time.delta_secs() / offset.duration;
        offset.current *= 1.0 - decay.min(1.0);
        if offset.elapsed >= offset.duration || offset.current.length() < 0.5 {
            commands.entity(entity).remove::<VisualOffset>();
        }
    }
}

/// Detects when the local player tile position changes and triggers smooth
/// viewport scrolling + the player walk animation. Runs purely client-side.
pub fn detect_player_movement(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    mut view_scroll: ResMut<ViewScrollOffset>,
    mut commands: Commands,
    player_query: Query<Entity, With<Player>>,
    mut last_tile: Local<Option<TilePosition>>,
) {
    let Some(new_tile) = client_state.player_tile_position else {
        return;
    };

    if let Some(old_tile) = *last_tile {
        let dx = new_tile.x - old_tile.x;
        let dy = new_tile.y - old_tile.y;
        let dz = new_tile.z - old_tile.z;
        // Only animate single-tile steps; skip teleports (portal jumps) and
        // floor transitions (stairs), which shouldn't play the walk clip.
        if dz == 0 && (dx != 0 || dy != 0) && dx.abs() <= 1 && dy.abs() <= 1 {
            view_scroll.current = Vec2::new(
                dx as f32 * world_config.tile_size,
                dy as f32 * world_config.tile_size,
            );
            view_scroll.elapsed = 0.0;
            view_scroll.duration = 0.18;

            if let Ok(entity) = player_query.single() {
                commands.entity(entity).insert(JustMoved { dx, dy });
            }
        }
    }

    *last_tile = Some(new_tile);
}
