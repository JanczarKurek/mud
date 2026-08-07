//! Client-side spawner for floating speech bubbles.
//!
//! Drains `GameUiEvent::SpeechBubble`, spawns a world-space text node + a
//! sprite backdrop attached to the speaker via the shared
//! `AttachedToObject` follower, and despawns after a short TTL. Mirrors the
//! one-shot pattern from `vfx.rs`; the bubble is presentation-only and
//! never round-trips back to the server.

use std::collections::HashMap;

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

/// Display cap on bubble text length, in characters. A proxy for ~3 lines at
/// the 128 px wrap width / font-11 (~20 chars per line); overflow is cut and
/// suffixed with `...`. Server `/say` already caps at 200 chars, but NPC
/// barks are unbounded, so this is the uniform display-side guard.
const BUBBLE_MAX_CHARS: usize = 64;

/// Most simultaneous bubbles kept alive per speaker. A 4th message despawns
/// the oldest immediately (older ones also expire on their own TTL).
const MAX_BUBBLES_PER_SPEAKER: usize = 3;

/// Vertical gap between stacked bubbles for the same speaker, in pixels.
const BUBBLE_STACK_GAP: f32 = 4.0;

/// Live bubbles per speaker `object_id`, ordered oldest -> newest. Drives the
/// vertical restack so rapid messages don't overlap at a single head offset.
#[derive(Resource, Default)]
pub struct SpeechBubbleStacks(pub HashMap<u64, Vec<Entity>>);

#[derive(Component)]
pub struct SpeechBubble {
    pub child_text: Entity,
    pub resize_pending: bool,
    /// Rendered backdrop height in pixels — seeded from the initial size and
    /// updated once text layout resolves, so the restack can pack tightly.
    pub height_px: f32,
}

pub fn consume_speech_bubble_events(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    mut stacks: ResMut<SpeechBubbleStacks>,
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
        let text = truncate_bubble_text(&text);

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
                wall_corner: None,
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
                height_px: BUBBLE_INITIAL_SIZE.y,
            },
        ));
        parent.add_child(text_entity);
        let bubble_entity = parent.id();

        // Register into the speaker's stack; a 4th bubble evicts the oldest
        // immediately so at most MAX_BUBBLES_PER_SPEAKER stay on screen.
        let stack = stacks.0.entry(speaker_object_id).or_default();
        stack.push(bubble_entity);
        while stack.len() > MAX_BUBBLES_PER_SPEAKER {
            let oldest = stack.remove(0);
            commands.entity(oldest).despawn();
        }
    }
}

/// Cap bubble text to `BUBBLE_MAX_CHARS`, suffixing `...` (ASCII — the default
/// font lacks a `…` glyph). Keeps rapid or verbose lines from ballooning into
/// tall multi-line bubbles.
fn truncate_bubble_text(text: &str) -> String {
    if text.chars().count() <= BUBBLE_MAX_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(BUBBLE_MAX_CHARS).collect();
    format!("{}...", kept.trim_end())
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
        let sized = layout.size + BUBBLE_PADDING * 2.0;
        sprite.custom_size = Some(sized);
        bubble.height_px = sized.y;
        bubble.resize_pending = false;
    }
}

/// Reflows each speaker's live bubbles into a vertical stack so multiple
/// messages in quick succession don't overlap at a single head offset. Prunes
/// entries whose entity despawned (via TTL or eviction), then packs the stack
/// newest-at-bottom (just above the head) with older bubbles lifted above it.
pub fn restack_speech_bubbles(
    mut stacks: ResMut<SpeechBubbleStacks>,
    world_config: Res<WorldConfig>,
    mut bubble_q: Query<(&SpeechBubble, &mut AttachedToObject)>,
) {
    let base_lift = world_config.tile_size * BUBBLE_LIFT_TILES;
    stacks.0.retain(|_speaker, entities| {
        entities.retain(|entity| bubble_q.contains(*entity));
        if entities.is_empty() {
            return false;
        }
        // Walk newest -> oldest, tracking the top edge of the bubble just
        // placed below so each older bubble sits GAP above it.
        let mut prev_top: Option<f32> = None;
        for entity in entities.iter().rev() {
            let Ok((bubble, mut attached)) = bubble_q.get_mut(*entity) else {
                continue;
            };
            let half_height = bubble.height_px * 0.5;
            let center = match prev_top {
                None => base_lift,
                Some(top) => top + BUBBLE_STACK_GAP + half_height,
            };
            if (attached.offset_pixels.y - center).abs() > f32::EPSILON {
                attached.offset_pixels.y = center;
            }
            prev_top = Some(center + half_height);
        }
        true
    });
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
