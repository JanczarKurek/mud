//! Client-side spawner for floating speech bubbles.
//!
//! Drains `GameUiEvent::SpeechBubble`, spawns a world-space text node + a
//! sprite backdrop attached to the speaker via the shared
//! `AttachedToObject` follower, and despawns after a short TTL. Mirrors the
//! one-shot pattern from `vfx.rs`; the bubble is presentation-only and
//! never round-trips back to the server.

use bevy::prelude::*;
use bevy::sprite::{Anchor, SpriteImageMode};
use bevy::text::{Justify, TextBounds, TextLayoutInfo};

use crate::game::resources::{
    ClientGameState, GameUiEvent, PendingGameUiEvents, SpeechBubbleStyle,
};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::attached::AttachedToObject;
use crate::world::components::{ViewPosition, WorldVisual};
use crate::world::ttl::Ttl;
use crate::world::WorldConfig;

/// Z bias on top of the speaker's z, so the bubble draws in front of the
/// sprite (and slightly above VFX, which sits at 0.9 in `WorldVisual`).
const BUBBLE_Z_INDEX: f32 = 0.95;
const BUBBLE_FOLLOWER_Z_BUMP: f32 = 0.1;

/// Vertical offset above the speaker's tile center, in tile units. Lifts
/// the bubble clear of head-height sprites.
const BUBBLE_LIFT_TILES: f32 = 1.1;

/// How long a bubble lingers before despawning. Long enough to read a
/// one-line bark, short enough that walking past three NPCs doesn't choke
/// the screen.
const BUBBLE_TTL_SECONDS: f32 = 3.5;

/// Maximum text width before the layout wraps to a new line, in pixels.
/// ~4 tiles of width at the default 32 px tile.
const BUBBLE_MAX_TEXT_WIDTH: f32 = 128.0;

/// Padding around the text inside the backdrop, in pixels. Generous enough
/// to keep the panel-frame border visually separated from the glyphs.
const BUBBLE_PADDING: Vec2 = Vec2::new(10.0, 6.0);

/// Initial backdrop size while we wait for text layout. Large enough that
/// the panel-frame's 8-px 9-slice corners don't crowd before resize.
const BUBBLE_INITIAL_SIZE: Vec2 = Vec2::new(32.0, 24.0);

#[derive(Component)]
pub struct SpeechBubble {
    pub child_text: Entity,
    pub resize_pending: bool,
}

pub fn consume_speech_bubble_events(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    mut commands: Commands,
) {
    let events = std::mem::take(&mut pending_ui_events.events);
    for event in events {
        let GameUiEvent::SpeechBubble {
            speaker_object_id,
            text,
            style,
        } = event
        else {
            pending_ui_events.events.push(event);
            continue;
        };

        let Some(view_position) = lookup_speaker_view(speaker_object_id, &client_state) else {
            continue;
        };

        let text_color = text_color_for_style(style, &palette);

        let text_entity = commands
            .spawn((
                Text2d::new(text),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(text_color),
                TextLayout::new_with_justify(Justify::Center),
                TextBounds::new_horizontal(BUBBLE_MAX_TEXT_WIDTH),
                Anchor::CENTER,
                Transform::from_xyz(0.0, 0.0, 0.01),
            ))
            .id();

        let mut parent = commands.spawn((
            Sprite {
                image: theme.panel_frame.clone(),
                image_mode: SpriteImageMode::Sliced(theme.panel_frame_slicer.clone()),
                custom_size: Some(BUBBLE_INITIAL_SIZE),
                color: backdrop_tint_for_style(style),
                ..default()
            },
            view_position,
            WorldVisual {
                z_index: BUBBLE_Z_INDEX,
                y_sort: true,
                sprite_height: 0.6,
                rotation_by_facing: false,
                block_size: 0,
                stack_order: 0,
                hide_when_inside_facing: None,
            },
            Transform::default(),
            AttachedToObject {
                object_id: speaker_object_id,
                offset_pixels: Vec2::new(0.0, world_config.tile_size * BUBBLE_LIFT_TILES),
                z_offset: BUBBLE_FOLLOWER_Z_BUMP,
            },
            Ttl {
                remaining_seconds: BUBBLE_TTL_SECONDS,
            },
            SpeechBubble {
                child_text: text_entity,
                resize_pending: true,
            },
        ));
        parent.add_child(text_entity);
    }
}

/// Once Bevy's text layout produces a non-zero `TextLayoutInfo.size`, snap
/// the backdrop sprite to that size + padding. Runs every frame but only
/// touches bubbles that haven't been sized yet.
pub fn resize_speech_bubble_backdrops(
    mut bubble_q: Query<(&mut Sprite, &mut SpeechBubble)>,
    text_q: Query<&TextLayoutInfo>,
) {
    for (mut sprite, mut bubble) in &mut bubble_q {
        if !bubble.resize_pending {
            continue;
        }
        let Ok(layout) = text_q.get(bubble.child_text) else {
            continue;
        };
        if layout.size.x <= 0.0 || layout.size.y <= 0.0 {
            continue;
        }
        sprite.custom_size = Some(layout.size + BUBBLE_PADDING * 2.0);
        bubble.resize_pending = false;
    }
}

/// Tint applied on top of the panel-frame texture. We keep the bubble
/// visually anchored in the same wood/gold family as the HUD panels and
/// only nudge the alpha and warmth per style — say and bark use the
/// natural panel color; mutters fade slightly to read as background
/// chatter.
fn backdrop_tint_for_style(style: SpeechBubbleStyle) -> Color {
    match style {
        SpeechBubbleStyle::Say => Color::srgba(1.0, 1.0, 1.0, 0.95),
        SpeechBubbleStyle::Bark => Color::srgba(1.0, 0.92, 0.85, 0.96),
        SpeechBubbleStyle::Mutter => Color::srgba(0.92, 0.92, 0.95, 0.75),
    }
}

fn text_color_for_style(style: SpeechBubbleStyle, palette: &Palette) -> Color {
    match style {
        SpeechBubbleStyle::Say => palette.text_primary,
        SpeechBubbleStyle::Bark => palette.text_accent,
        SpeechBubbleStyle::Mutter => palette.text_muted,
    }
}

fn lookup_speaker_view(object_id: u64, client_state: &ClientGameState) -> Option<ViewPosition> {
    if client_state.local_player_object_id == Some(object_id) {
        let pos = client_state.player_position?;
        let tile = client_state.player_tile_position?;
        return Some(ViewPosition {
            space_id: pos.space_id,
            tile,
        });
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
