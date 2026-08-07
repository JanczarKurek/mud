//! Debug overlay: a persistent box over each NPC's head showing the full state
//! of its AI (FSM state, target, perception/detect range, heard noise). Toggled
//! with **Shift+F7** or the **Debug ▸ NPC AI** menu entry. Built for inspecting
//! behaviors like stealth detection where the visible result ("did it see me?")
//! hides a lot of state.
//!
//! Reads the **authoritative** NPC components (`AiState`, `AiMemory`,
//! `CombatTarget`, `HostileBehavior`) directly — so it only shows anything in
//! **EmbeddedClient** mode, where the server and client share one `App`. In
//! TcpClient mode those components live on the remote server, so the overlay is
//! silently empty (debug NPC AI in embedded mode). This is a read-only
//! diagnostic: it never writes `ClientGameState` or authoritative state, so it
//! doesn't affect the EmbeddedClient invariant.
//!
//! Positioning: the box tracks the NPC's *rendered* position (read from the
//! projected visual's `Transform`) and is placed at an absolute z **above the
//! darkness quad** (`darkness::OVERLAY_Z = 999`), so debug boxes stay fully
//! readable at night / in shadow — they're a debug aid, not part of the lit
//! scene. The backdrop is sized to its text so nothing overflows.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::sprite::{Anchor, SpriteImageMode};
use bevy::text::Justify;

use crate::combat::components::CombatTarget;
use crate::npc::components::{AiMemory, AiState, HostileBehavior, Npc};
use crate::npc::routine::{RoutinePhase, RoutineState};
use crate::npc::social::ConversationRegistry;
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::components::{
    ClientProjectedWorldObject, OverworldObject, SpaceResident, TilePosition,
};
use crate::world::noise::NoiseField;

/// Absolute render z for the boxes. Above the darkness quad
/// (`world::darkness` `OVERLAY_Z` = 999.0) and below the 2D camera far plane
/// (default 1000.0), so the boxes draw on top of darkness and never dim.
const OVERLAY_Z: f32 = 999.5;

/// Vertical lift above the NPC's tile center, in tile units.
const OVERLAY_LIFT_TILES: f32 = 1.7;

/// Text size for the box. Glyph/line metrics below are derived from it so the
/// backdrop can be sized without waiting on Bevy's text layout.
const OVERLAY_FONT_SIZE: f32 = 10.0;

/// Upper-bound glyph advance and line height for the (proportional) default
/// font, used to size the backdrop without waiting on text layout. Deliberately
/// generous (an over-estimate) so the text never clips — a slightly roomy debug
/// box is fine, an overflowing one is not.
const GLYPH_WIDTH: f32 = OVERLAY_FONT_SIZE * 0.7;
const LINE_HEIGHT: f32 = OVERLAY_FONT_SIZE * 1.5;

/// Padding around the text inside the backdrop, in pixels.
const OVERLAY_PADDING: Vec2 = Vec2::new(8.0, 5.0);

/// Toggle state + the per-NPC box entities, keyed by NPC object id.
#[derive(Resource, Default)]
pub struct AiDebugOverlay {
    pub enabled: bool,
    labels: HashMap<u64, OverlayHandles>,
}

#[derive(Clone, Copy)]
struct OverlayHandles {
    backdrop: Entity,
    text: Entity,
}

/// Marker on the backdrop sprite. Used to scope the position/size query and to
/// keep the box's `Transform` out of the projected-objects query.
#[derive(Component)]
pub struct AiDebugLabel;

/// Shift+F7 flips the overlay. (Plain F7 is the archetype-histogram dump in
/// `diagnostics`; the Debug menu has an equivalent entry.) Cleanup of the boxes
/// when turning off is handled by `sync_ai_debug_overlay`.
pub fn toggle_ai_debug_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: ResMut<AiDebugOverlay>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if shift && keys.just_pressed(KeyCode::F7) {
        overlay.enabled = !overlay.enabled;
        info!(
            "Diagnostics: NPC AI overlay = {}",
            if overlay.enabled { "ON" } else { "OFF" }
        );
    }
}

/// Spawn/update/despawn one box per NPC carrying its live AI state. When the
/// overlay is off, despawns any remaining boxes. The box tracks the NPC's
/// rendered position and is sized to its text.
pub fn sync_ai_debug_overlay(
    time: Res<Time>,
    mut overlay: ResMut<AiDebugOverlay>,
    noise_field: Option<Res<NoiseField>>,
    world_config: Res<crate::world::WorldConfig>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    conversations: Option<Res<ConversationRegistry>>,
    npc_q: Query<
        (
            Entity,
            &OverworldObject,
            &SpaceResident,
            &TilePosition,
            &AiState,
            &AiMemory,
            Option<&CombatTarget>,
            Option<&HostileBehavior>,
            Option<&RoutineState>,
        ),
        With<Npc>,
    >,
    label_q: Query<&OverworldObject>,
    projected_q: Query<(&ClientProjectedWorldObject, &Transform), Without<AiDebugLabel>>,
    mut text_q: Query<&mut Text2d>,
    mut backdrop_q: Query<(&mut Sprite, &mut Transform), With<AiDebugLabel>>,
    mut commands: Commands,
) {
    // Off: tear down any boxes and bail.
    if !overlay.enabled {
        if !overlay.labels.is_empty() {
            for handles in overlay.labels.values() {
                commands.entity(handles.backdrop).despawn();
            }
            overlay.labels.clear();
        }
        return;
    }

    let elapsed = time.elapsed_secs();

    // Rendered positions of NPC visuals, keyed by object id, so the box tracks
    // the smoothed on-screen position (and we read an authoritative-free z base).
    let rendered: HashMap<u64, Vec3> = projected_q
        .iter()
        .map(|(proj, tf)| (proj.object_id, tf.translation))
        .collect();
    let lift = world_config.tile_size * (0.5 + OVERLAY_LIFT_TILES);

    let mut seen: HashSet<u64> = HashSet::new();
    for (entity, object, resident, tile, state, memory, target, hostile, routine) in &npc_q {
        let object_id = object.object_id;
        seen.insert(object_id);

        let heard = noise_field
            .as_deref()
            .and_then(|field| field.loudest_audible(resident.space_id, *tile));
        let lines = build_overlay_lines(
            &object.definition_id,
            object_id,
            state,
            memory,
            target,
            hostile,
            routine,
            conversations
                .as_deref()
                .is_some_and(|c| c.is_conversing(entity)),
            heard,
            elapsed,
            &label_q,
        );
        let content = lines.join("\n");
        let size = box_size(&lines);
        let pos = rendered
            .get(&object_id)
            .map(|base| Vec3::new(base.x, base.y + lift, OVERLAY_Z));

        if let Some(handles) = overlay.labels.get(&object_id).copied() {
            if let Ok(mut text) = text_q.get_mut(handles.text) {
                if text.0 != content {
                    text.0 = content;
                }
            }
            if let Ok((mut sprite, mut transform)) = backdrop_q.get_mut(handles.backdrop) {
                if sprite.custom_size != Some(size) {
                    sprite.custom_size = Some(size);
                }
                if let Some(pos) = pos {
                    if transform.translation != pos {
                        transform.translation = pos;
                    }
                }
            }
        } else if let Some(pos) = pos {
            // Only spawn once the NPC has a rendered position to anchor to.
            let text_entity = commands
                .spawn((
                    Text2d::new(content),
                    TextFont {
                        font_size: OVERLAY_FONT_SIZE,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                    TextLayout::new_with_justify(Justify::Left),
                    Anchor::CENTER,
                    Transform::from_xyz(0.0, 0.0, 0.01),
                ))
                .id();
            let backdrop = commands
                .spawn((
                    Sprite {
                        image: theme.panel_frame.clone(),
                        image_mode: SpriteImageMode::Sliced(theme.panel_frame_slicer.clone()),
                        custom_size: Some(size),
                        color: Color::srgba(0.08, 0.10, 0.16, 0.92),
                        ..default()
                    },
                    Transform::from_translation(pos),
                    AiDebugLabel,
                ))
                .add_child(text_entity)
                .id();
            overlay.labels.insert(
                object_id,
                OverlayHandles {
                    backdrop,
                    text: text_entity,
                },
            );
        }
    }

    // Despawn boxes for NPCs that are gone (died, despawned, left the world).
    overlay.labels.retain(|object_id, handles| {
        if seen.contains(object_id) {
            true
        } else {
            commands.entity(handles.backdrop).despawn();
            false
        }
    });
}

/// Backdrop size from the laid-out text dimensions (longest line × glyph width,
/// line count × line height) plus padding. Generous by design so text fits.
fn box_size(lines: &[String]) -> Vec2 {
    let max_chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    Vec2::new(
        max_chars as f32 * GLYPH_WIDTH + OVERLAY_PADDING.x * 2.0,
        lines.len() as f32 * LINE_HEIGHT + OVERLAY_PADDING.y * 2.0,
    )
}

fn build_overlay_lines(
    definition_id: &str,
    object_id: u64,
    state: &AiState,
    memory: &AiMemory,
    target: Option<&CombatTarget>,
    hostile: Option<&HostileBehavior>,
    routine: Option<&RoutineState>,
    conversing: bool,
    heard: Option<TilePosition>,
    elapsed: f32,
    label_q: &Query<&OverworldObject>,
) -> Vec<String> {
    let mut lines = vec![
        format!("{definition_id}#{object_id}"),
        format!("st: {}", format_ai_state(state, elapsed)),
    ];
    if let Some(target) = target {
        lines.push(format!("tgt: {}", entity_label(target.entity, label_q)));
    }
    // Routine ("life agenda") state — only present on hand-placed NPCs, and
    // only interesting while they actually have an active goal.
    if let Some(line) = routine.and_then(format_routine) {
        lines.push(line);
    }
    if conversing {
        lines.push("chat".to_string());
    }
    if let Some(hostile) = hostile {
        lines.push(format!(
            "per:{} det:{} los:{}",
            hostile.perception,
            hostile.detect_distance_tiles,
            if hostile.requires_line_of_sight {
                "y"
            } else {
                "n"
            }
        ));
    }
    if let Some(noise) = heard {
        lines.push(format!("hear:({},{})", noise.x, noise.y));
    }
    if memory.contact_grace_until > elapsed {
        lines.push(format!(
            "grace:{:.1}s",
            memory.contact_grace_until - elapsed
        ));
    }
    lines
}

/// Compact one-line routine summary, or `None` when the NPC has no active
/// agenda (idle, no pose) — keeps the box clutter-free for plain mobs.
fn format_routine(routine: &RoutineState) -> Option<String> {
    if routine.phase == RoutinePhase::Idle
        && routine.active_activity.is_none()
        && routine.active_pose.is_none()
    {
        return None;
    }
    let phase = match routine.phase {
        RoutinePhase::Idle => "idle",
        RoutinePhase::Traveling => "go",
        RoutinePhase::Dwelling => "do",
    };
    // Schedule activities have a name; patrols just have a waypoint index.
    let what = routine
        .active_activity
        .clone()
        .unwrap_or_else(|| format!("wp{}", routine.waypoint_index));
    let pose = routine
        .active_pose
        .as_deref()
        .map(|p| format!(" [{p}]"))
        .unwrap_or_default();
    Some(format!("rt: {phase} {what}{pose}"))
}

fn format_ai_state(state: &AiState, elapsed: f32) -> String {
    match state {
        AiState::Wander => "Wander".to_string(),
        AiState::Alert {
            last_seen,
            expires_at_seconds,
        } => format!(
            "Alert->({},{}) {:.1}s",
            last_seen.x,
            last_seen.y,
            (expires_at_seconds - elapsed).max(0.0)
        ),
        AiState::Pursue { .. } => "Pursue".to_string(),
        AiState::Engage { .. } => "Engage".to_string(),
        AiState::Flee {
            expires_at_seconds, ..
        } => format!("Flee {:.1}s", (expires_at_seconds - elapsed).max(0.0)),
    }
}

/// Resolve a target entity to a readable `def#id`, falling back to the raw
/// entity if it carries no `OverworldObject` (shouldn't happen for NPC targets).
fn entity_label(entity: Entity, label_q: &Query<&OverworldObject>) -> String {
    match label_q.get(entity) {
        Ok(object) => format!("{}#{}", object.definition_id, object.object_id),
        Err(_) => format!("{entity:?}"),
    }
}
