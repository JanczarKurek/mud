use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::Player;
use crate::world::components::{ClientProjectedWorldObject, Facing, TilePosition};
use crate::world::direction::Direction;
use crate::world::lerp_anim::LinearLerp;
use crate::world::object_definitions::{
    AnimationClipDef, AnimationSheetDef, OverworldObjectDefinitions,
};
use crate::world::resources::{FloorTransitionOffset, ViewScrollOffset};
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

/// Per-entity pixel offset (in world space) that lerps toward zero after a
/// move. Added to the entity's tile-based translation by `sync_tile_transforms`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct VisualOffset {
    pub lerp: LinearLerp<Vec2>,
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

fn facing_suffix(facing: Direction) -> &'static str {
    match facing {
        Direction::North => "_n",
        Direction::South => "_s",
        Direction::East => "_e",
        Direction::West => "_w",
    }
}

/// Resolve `base` (e.g. `"idle"` or `"walk"`) against the clip map, preferring
/// the facing-suffixed variant (`"walk_n"`) when present. Falls back to the
/// unsuffixed `base` so legacy sheets without per-direction clips keep working.
fn resolved_clip<'a>(
    clips: &'a std::collections::HashMap<String, AnimationClipDef>,
    base: &str,
    facing: Option<Direction>,
) -> Option<(String, &'a AnimationClipDef)> {
    if let Some(dir) = facing {
        let key = format!("{base}{}", facing_suffix(dir));
        if let Some(c) = clips.get(&key) {
            return Some((key, c));
        }
    }
    clips.get(base).map(|c| (base.to_string(), c))
}

/// `true` if the current clip is some variant of `walk` (`walk`, `walk_n`, …).
fn is_walk_clip(name: &str) -> bool {
    name == "walk" || name.starts_with("walk_")
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Build an `(AnimatedSprite, Sprite)` pair from a sheet definition. Used both
/// at spawn time (`attach_animated_sprite`) and when an `ObjectState`
/// transition swaps a still sprite for an animated one (sprite_state.rs).
pub fn build_animated_sprite_components(
    sheet: &AnimationSheetDef,
    asset_server: &AssetServer,
    texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
) -> (AnimatedSprite, Sprite) {
    let layout = TextureAtlasLayout::from_grid(
        UVec2::new(sheet.frame_width, sheet.frame_height),
        sheet.sheet_columns,
        sheet.sheet_rows,
        None,
        None,
    );
    let layout_handle = texture_atlas_layouts.add(layout);
    let image_handle: Handle<Image> = asset_server.load(&sheet.sheet_path);

    let idle_clip = sheet.clips.get("idle");
    let animated = AnimatedSprite {
        current_clip: "idle".to_string(),
        frame_index: 0,
        frame_timer: 0.0,
        frame_count: idle_clip.map_or(1, |c| c.frame_count),
        seconds_per_frame: idle_clip.map_or(1.0, |c| if c.fps > 0.0 { 1.0 / c.fps } else { 1.0 }),
        atlas_columns: sheet.sheet_columns,
        clip_row: idle_clip.map_or(0, |c| c.row),
        clip_start_col: idle_clip.map_or(0, |c| c.start_col),
        looping: idle_clip.is_none_or(|c| c.looping),
    };

    let sprite = Sprite {
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

    (animated, sprite)
}

/// Attaches `AnimatedSprite` to newly spawned entities whose object definition
/// has an `animation:` block, and swaps their `Sprite` to use a `TextureAtlas`.
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
    let mut try_attach = |entity: Entity, definition_id: &str| {
        let Some(def) = definitions.get(definition_id) else {
            return;
        };
        let Some(sheet) = &def.render.animation else {
            return;
        };

        let (animated, sprite) =
            build_animated_sprite_components(sheet, &asset_server, &mut texture_atlas_layouts);
        commands.entity(entity).insert((animated, sprite));
    };

    for (entity, world_obj) in &world_objects {
        try_attach(entity, &world_obj.definition_id);
    }

    for (entity, _sprite) in &player_query {
        try_attach(entity, "player");
    }
}

/// Advances frame timers and writes the current atlas index into each
/// `AnimatedSprite`'s `Sprite` component.
pub fn advance_animation_timers(
    time: Res<Time>,
    mut query: Query<(&mut AnimatedSprite, &mut Sprite)>,
) {
    let _t = crate::diagnostics::SystemTimer::new("advance_animation_timers", 1.0);
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
        &ClientProjectedWorldObject,
        &JustMoved,
        Option<&Facing>,
    )>,
    mut player_query: Query<
        (&mut AnimatedSprite, &JustMoved, Option<&Facing>),
        (With<Player>, Without<ClientProjectedWorldObject>),
    >,
) {
    let try_walk = |animated: &mut AnimatedSprite,
                    clips: &std::collections::HashMap<String, AnimationClipDef>,
                    atlas_columns: u32,
                    facing: Option<Direction>| {
        // Prefer a directional walk; fall back to plain walk; then to idle.
        let resolved =
            resolved_clip(clips, "walk", facing).or_else(|| resolved_clip(clips, "idle", facing));
        if let Some((clip_name, clip)) = resolved {
            // Don't restart the clip if we're already playing it — otherwise
            // every frame that `JustMoved` is present (now the whole step
            // duration, not just the trigger frame) would reset `frame_index`
            // to 0 and freeze the walk on its first frame.
            if animated.current_clip == clip_name {
                return;
            }
            apply_clip(
                animated,
                &clip_name,
                atlas_columns,
                clip.row,
                clip.start_col,
                clip.frame_count,
                clip.fps,
                clip.looping,
            );
        }
    };

    for (mut animated, world_obj, _just_moved, facing) in &mut world_obj_query {
        let Some(def) = definitions.get(&world_obj.definition_id) else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        try_walk(
            &mut animated,
            &sheet.clips,
            sheet.sheet_columns,
            facing.map(|f| f.0),
        );
    }

    for (mut animated, _just_moved, facing) in &mut player_query {
        let Some(def) = definitions.get("player") else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        try_walk(
            &mut animated,
            &sheet.clips,
            sheet.sheet_columns,
            facing.map(|f| f.0),
        );
    }
}

/// Transitions animated entities back to idle when they no longer have `JustMoved`.
pub fn return_to_idle_animation(
    definitions: Res<OverworldObjectDefinitions>,
    mut world_obj_query: Query<
        (
            &mut AnimatedSprite,
            &ClientProjectedWorldObject,
            Option<&Facing>,
        ),
        Without<JustMoved>,
    >,
    mut player_query: Query<
        (&mut AnimatedSprite, Option<&Facing>),
        (
            With<Player>,
            Without<JustMoved>,
            Without<ClientProjectedWorldObject>,
        ),
    >,
) {
    let try_idle = |animated: &mut AnimatedSprite,
                    clips: &std::collections::HashMap<String, AnimationClipDef>,
                    atlas_columns: u32,
                    facing: Option<Direction>| {
        let Some((clip_name, clip)) = resolved_clip(clips, "idle", facing) else {
            return;
        };
        // Swap if we were walking, OR if the directional idle for the current
        // facing differs from the clip we're currently playing (turn-in-place).
        let needs_swap = is_walk_clip(&animated.current_clip) || animated.current_clip != clip_name;
        if !needs_swap {
            return;
        }
        apply_clip(
            animated,
            &clip_name,
            atlas_columns,
            clip.row,
            clip.start_col,
            clip.frame_count,
            clip.fps,
            clip.looping,
        );
    };

    for (mut animated, world_obj, facing) in &mut world_obj_query {
        let Some(def) = definitions.get(&world_obj.definition_id) else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        let cols = sheet.sheet_columns;
        try_idle(&mut animated, &sheet.clips, cols, facing.map(|f| f.0));
    }

    for (mut animated, facing) in &mut player_query {
        let Some(def) = definitions.get("player") else {
            continue;
        };
        let Some(sheet) = &def.render.animation else {
            continue;
        };
        let cols = sheet.sheet_columns;
        try_idle(&mut animated, &sheet.clips, cols, facing.map(|f| f.0));
    }
}

/// Removes `JustMoved` from an entity once its movement lerp has completed.
/// Keeping the marker alive for the whole step (vs the one-frame design that
/// used to live here) is what lets the walk clip actually cycle through its
/// frames instead of flashing for one frame and immediately returning to idle.
///
/// "Movement done" is determined by the lerp that drives the entity's smooth
/// scroll — `ViewScrollOffset` for the local player, the per-entity
/// `VisualOffset` for projected world objects and remote players. A missing
/// `VisualOffset` means `tick_visual_offsets` already despawned it, so the
/// step is finished.
pub fn cleanup_just_moved(
    mut commands: Commands,
    view_scroll: Res<ViewScrollOffset>,
    player_query: Query<Entity, (With<Player>, With<JustMoved>)>,
    other_query: Query<(Entity, Option<&VisualOffset>), (With<JustMoved>, Without<Player>)>,
) {
    if !view_scroll.lerp.is_active() {
        for entity in &player_query {
            commands.entity(entity).remove::<JustMoved>();
        }
    }
    for (entity, visual_offset) in &other_query {
        if visual_offset.is_none_or(|v| !v.lerp.is_active()) {
            commands.entity(entity).remove::<JustMoved>();
        }
    }
}

/// Advances the player-movement-driven viewport scroll offset toward zero.
pub fn tick_view_scroll(time: Res<Time>, mut offset: ResMut<ViewScrollOffset>) {
    offset.lerp.tick(time.delta_secs());
}

/// Advances the local player's floor-transition residual toward zero.
pub fn tick_floor_transition(time: Res<Time>, mut offset: ResMut<FloorTransitionOffset>) {
    // Only touch the resource while a transition is live. A bare
    // `offset.lerp.tick(..)` goes through `ResMut`'s `DerefMut`, which marks the
    // resource changed *every frame* regardless of the early-return inside
    // `tick` — that would defeat the `resource_changed::<FloorTransitionOffset>`
    // gate on `sync_floor_render_transforms`. `is_active()` is false the frame
    // after the lerp completes (tick self-zeroes `duration`), so the final
    // animating frame still ticks and the first idle frame is correctly skipped.
    if offset.lerp.is_active() {
        offset.lerp.tick(time.delta_secs());
    }
}

/// Advances per-entity visual offsets toward zero.
pub fn tick_visual_offsets(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut VisualOffset)>,
) {
    let dt = time.delta_secs();
    for (entity, mut offset) in &mut query {
        offset.lerp.tick(dt);
        if !offset.lerp.is_active() {
            commands.entity(entity).remove::<VisualOffset>();
        }
    }
}

/// Detects when the local player tile position changes and triggers smooth
/// viewport scrolling, smooth floor-perspective transitions, and the player
/// walk animation. Runs purely client-side.
pub fn detect_player_movement(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    mut view_scroll: ResMut<ViewScrollOffset>,
    mut floor_transition: ResMut<FloorTransitionOffset>,
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
        // Animate single-tile xy steps; skip xy teleports (portal jumps).
        let xy_step = (dx != 0 || dy != 0) && dx.abs() <= 1 && dy.abs() <= 1;
        if xy_step {
            let displacement = Vec2::new(
                dx as f32 * world_config.tile_size,
                dy as f32 * world_config.tile_size,
            );
            view_scroll.lerp.push(displacement, 0.18);

            if let Ok(entity) = player_query.single() {
                commands.entity(entity).insert(JustMoved { dx, dy });
            }
        }
        // Smooth the floor-perspective shift on any z change (stair-climb,
        // half-block stack step). The visual player_z lags behind by `-dz`
        // and decays back to 0 over the same duration as an xy step.
        if dz != 0 && dz.abs() <= 4 {
            floor_transition.lerp.push(-dz as f32, 0.18);
        }
    }

    *last_tile = Some(new_tile);
}
