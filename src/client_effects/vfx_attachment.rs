//! Sticky buff overlays for the local player.
//!
//! Reads `ClientGameState.active_effects` (kept up to date by the
//! `PlayerEffectsChanged` event) and spawns/despawns looping VFX entities
//! that follow the local player. Each `EffectKind` maps to at most one
//! overlay; recasting an active effect leaves the existing overlay in place
//! rather than spawning a duplicate.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::client_effects::vfx::Vfx;
use crate::game::resources::ClientGameState;
use crate::magic::resources::EffectKind;
use crate::world::animation::build_animated_sprite_components;
use crate::world::attached::AttachedToObject;
use crate::world::components::{ViewPosition, WorldVisual};
use crate::world::object_definitions::AnimationClipDef;
use crate::world::vfx::VfxDefinitions;
use crate::world::WorldConfig;

const ATTACHMENT_Z_INDEX: f32 = 0.95;

#[derive(Component, Clone, Copy, Debug)]
pub struct VfxAttachment {
    pub effect_kind: EffectKind,
}

#[derive(Resource, Default)]
pub struct VfxAttachmentRegistry {
    map: HashMap<EffectKind, Entity>,
}

pub fn project_active_effects_to_attachments(
    client_state: Res<ClientGameState>,
    vfx_definitions: Res<VfxDefinitions>,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    world_config: Res<WorldConfig>,
    mut registry: Local<VfxAttachmentRegistry>,
    mut commands: Commands,
) {
    let Some(player_object_id) = client_state.local_player_object_id else {
        // No local player yet — nothing to attach to.
        return;
    };

    let Some(player_pos) = client_state.player_position else {
        return;
    };
    let Some(player_tile) = client_state.player_tile_position else {
        return;
    };
    let initial_view = ViewPosition {
        space_id: player_pos.space_id,
        tile: player_tile,
    };

    let mut wanted: HashMap<EffectKind, ()> = HashMap::new();
    for effect in &client_state.active_effects {
        wanted.insert(effect.kind, ());
    }

    // Despawn overlays for effects no longer active.
    registry.map.retain(|kind, entity| {
        if wanted.contains_key(kind) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });

    // Spawn overlays for newly active effects.
    for kind in wanted.keys() {
        if registry.map.contains_key(kind) {
            continue;
        }
        let Some(definition_id) = definition_id_for_effect(*kind) else {
            continue;
        };
        let Some(def) = vfx_definitions.get(definition_id) else {
            continue;
        };
        let (mut animated, mut sprite) = build_animated_sprite_components(
            &def.animation,
            &asset_server,
            &mut texture_atlas_layouts,
        );
        if let Some(play) = def.animation.clips.get("play") {
            apply_play_clip(&mut animated, play, def.animation.sheet_columns);
            animated.looping = true;
        }
        let scale = def.scale.unwrap_or(1.0);
        let pixel_w = def.animation.frame_width as f32 * scale;
        let pixel_h = def.animation.frame_height as f32 * scale;
        sprite.custom_size = Some(Vec2::new(pixel_w, pixel_h));
        let sprite_height_tiles = (pixel_h / world_config.tile_size).max(0.5);

        let entity = commands
            .spawn((
                Vfx,
                VfxAttachment { effect_kind: *kind },
                AttachedToObject::at(player_object_id),
                initial_view,
                WorldVisual {
                    z_index: ATTACHMENT_Z_INDEX,
                    y_sort: true,
                    sprite_height: sprite_height_tiles,
                    rotation_by_facing: false,
                },
                Transform::default(),
                animated,
                sprite,
            ))
            .id();
        registry.map.insert(*kind, entity);
    }
}

fn definition_id_for_effect(kind: EffectKind) -> Option<&'static str> {
    Some(match kind {
        EffectKind::Glimmer => "glimmer_aura",
        EffectKind::Haste => "haste_streaks",
        EffectKind::Shield => "shield_bubble",
        EffectKind::Bless => "bless_aura",
        EffectKind::Slow => "slow_drag",
        EffectKind::Sleep => "sleep_zs",
    })
}

fn apply_play_clip(
    animated: &mut crate::world::animation::AnimatedSprite,
    clip: &AnimationClipDef,
    atlas_columns: u32,
) {
    animated.current_clip = "play".to_owned();
    animated.frame_index = 0;
    animated.frame_timer = 0.0;
    animated.frame_count = clip.frame_count.max(1);
    animated.seconds_per_frame = if clip.fps > 0.0 { 1.0 / clip.fps } else { 1.0 };
    animated.atlas_columns = atlas_columns;
    animated.clip_row = clip.row;
    animated.clip_start_col = clip.start_col;
    animated.looping = true;
}
