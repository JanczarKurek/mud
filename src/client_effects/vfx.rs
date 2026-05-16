//! Client-side spawner for one-shot visual effects.
//!
//! Drains `GameUiEvent::VfxSpawn` events, looks up the named effect in
//! `VfxDefinitions`, and spawns a presentation-only entity carrying the
//! frame-cycling animation plus a `Ttl` that despawns it when the play head
//! reaches the end. For `FollowObject` anchors, the entity also carries an
//! `AttachedToObject` so the shared `sync_attached_object_visuals` system
//! lerps it alongside the target.

use bevy::prelude::*;

use crate::game::resources::{ClientGameState, GameUiEvent, PendingGameUiEvents, VfxAnchor};
use crate::world::animation::build_animated_sprite_components;
use crate::world::attached::AttachedToObject;
use crate::world::components::{ViewPosition, WorldVisual};
use crate::world::object_definitions::AnimationClipDef;
use crate::world::ttl::Ttl;
use crate::world::vfx::VfxDefinitions;
use crate::world::WorldConfig;

const VFX_Z_INDEX: f32 = 0.9;
const VFX_FOLLOWER_Z_BUMP: f32 = 0.05;

#[derive(Component)]
pub struct Vfx;

pub fn consume_vfx_events(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    world_config: Res<WorldConfig>,
    client_state: Res<ClientGameState>,
    vfx_definitions: Res<VfxDefinitions>,
    mut commands: Commands,
) {
    let events = std::mem::take(&mut pending_ui_events.events);
    for event in events {
        let GameUiEvent::VfxSpawn {
            definition_id,
            anchor,
        } = event
        else {
            pending_ui_events.events.push(event);
            continue;
        };

        let Some(def) = vfx_definitions.get(&definition_id) else {
            continue;
        };

        let Some((view_position, attached)) = resolve_anchor(&anchor, &client_state) else {
            // No way to render a follow-anchor whose target is gone and
            // whose space is unknown — drop silently.
            continue;
        };

        let (mut animated, mut sprite) = build_animated_sprite_components(
            &def.animation,
            &asset_server,
            &mut texture_atlas_layouts,
        );
        // Force the "play" clip if present; build_animated_sprite_components
        // defaults to "idle".
        if let Some(play) = def.animation.clips.get("play") {
            apply_play_clip(&mut animated, play, def.animation.sheet_columns);
        }

        let scale = def.scale.unwrap_or(1.0);
        let pixel_w = def.animation.frame_width as f32 * scale;
        let pixel_h = def.animation.frame_height as f32 * scale;
        sprite.custom_size = Some(Vec2::new(pixel_w, pixel_h));

        let sprite_height_tiles = (pixel_h / world_config.tile_size).max(0.5);

        let mut entity = commands.spawn((
            Vfx,
            view_position,
            WorldVisual {
                z_index: VFX_Z_INDEX,
                y_sort: true,
                sprite_height: sprite_height_tiles,
                rotation_by_facing: false,
                display_height: 0.0,
                stack_order: 0,
                hide_when_inside_facing: None,
            },
            Transform::default(),
            animated,
            sprite,
        ));

        if !def.looping {
            entity.insert(Ttl {
                remaining_seconds: def.resolved_duration_seconds(),
            });
        }

        if let Some(attached) = attached {
            entity.insert(attached);
        }
    }
}

fn resolve_anchor(
    anchor: &VfxAnchor,
    client_state: &ClientGameState,
) -> Option<(ViewPosition, Option<AttachedToObject>)> {
    match anchor {
        VfxAnchor::Tile { space_id, tile } => Some((
            ViewPosition {
                space_id: *space_id,
                tile: *tile,
            },
            None,
        )),
        VfxAnchor::FollowObject {
            object_id,
            offset_pixels,
        } => {
            let initial = lookup_object_view(*object_id, client_state)
                .or_else(|| client_player_view(client_state))?;
            Some((
                initial,
                Some(AttachedToObject {
                    object_id: *object_id,
                    offset_pixels: Vec2::new(offset_pixels[0], offset_pixels[1]),
                    z_offset: VFX_FOLLOWER_Z_BUMP,
                }),
            ))
        }
    }
}

fn lookup_object_view(object_id: u64, client_state: &ClientGameState) -> Option<ViewPosition> {
    if client_state.local_player_object_id == Some(object_id) {
        return client_player_view(client_state);
    }
    if let Some(state) = client_state.world_objects.get(&object_id) {
        return Some(ViewPosition {
            space_id: state.position.space_id,
            tile: state.tile_position,
        });
    }
    for remote in client_state.remote_players.values() {
        if remote.object_id == object_id {
            return Some(ViewPosition {
                space_id: remote.position.space_id,
                tile: remote.tile_position,
            });
        }
    }
    None
}

fn client_player_view(client_state: &ClientGameState) -> Option<ViewPosition> {
    let pos = client_state.player_position?;
    let tile = client_state.player_tile_position?;
    Some(ViewPosition {
        space_id: pos.space_id,
        tile,
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
    animated.looping = clip.looping;
}
