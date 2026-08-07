//! Over-head NPC awareness markers — the player-facing payoff of the stealth
//! Perception/Stealth contest. When the player (while sneaking) succeeds a
//! Perception read of a hostile NPC, the server replicates that NPC's awareness
//! in `ClientWorldObjectState.awareness`; this system renders it as a colored
//! glyph above the NPC's head:
//!
//! - `z` (green)  — Unaware: hasn't noticed you.
//! - `?` (yellow) — Searching: suspicious / investigating.
//! - `!` (red)    — Alerted: it sees you.
//!
//! Purely presentation: reads `ClientGameState` (never authoritative state), so
//! it works in every runtime mode. Drawn at an absolute z above the darkness
//! quad so the marker stays readable at night.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::Justify;

use crate::game::resources::{ClientGameState, NpcAwareness};
use crate::world::components::ClientProjectedWorldObject;

/// Absolute render z — above the darkness quad (`world::darkness` = 999.0),
/// below the 2D camera far plane (1000.0), so markers never dim.
const MARKER_Z: f32 = 999.6;

/// Lift above the NPC's tile center, in tile units.
const MARKER_LIFT_TILES: f32 = 1.2;

const MARKER_FONT_SIZE: f32 = 18.0;

/// Marker entity per NPC object id, so we update in place rather than respawn.
#[derive(Resource, Default)]
pub struct AwarenessMarkers {
    map: HashMap<u64, Entity>,
}

/// Tag on a marker glyph entity.
#[derive(Component)]
pub struct AwarenessMarker;

/// Spawn/update/despawn the over-head markers from the replicated per-NPC
/// awareness. Tracks each NPC's rendered position so the glyph follows it.
pub fn sync_awareness_markers(
    client_state: Res<ClientGameState>,
    world_config: Res<crate::world::WorldConfig>,
    mut markers: ResMut<AwarenessMarkers>,
    projected_q: Query<(&ClientProjectedWorldObject, &Transform), Without<AwarenessMarker>>,
    mut marker_q: Query<(&mut Text2d, &mut TextColor, &mut Transform), With<AwarenessMarker>>,
    mut commands: Commands,
) {
    // Rendered NPC positions, keyed by object id.
    let rendered: HashMap<u64, Vec3> = projected_q
        .iter()
        .map(|(proj, tf)| (proj.object_id, tf.translation))
        .collect();
    let lift = world_config.tile_size * (0.5 + MARKER_LIFT_TILES);

    let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for (object_id, object) in &client_state.world_objects {
        let Some(awareness) = object.awareness else {
            continue;
        };
        // Only place a marker once the NPC has a rendered position to anchor to.
        let Some(base) = rendered.get(object_id) else {
            continue;
        };
        seen.insert(*object_id);
        let glyph = marker_glyph(awareness);
        let color = marker_color(awareness);
        let pos = Vec3::new(base.x, base.y + lift, MARKER_Z);

        if let Some(&entity) = markers.map.get(object_id) {
            if let Ok((mut text, mut text_color, mut transform)) = marker_q.get_mut(entity) {
                if text.0 != glyph {
                    text.0 = glyph.to_string();
                }
                if text_color.0 != color {
                    text_color.0 = color;
                }
                if transform.translation != pos {
                    transform.translation = pos;
                }
            }
        } else {
            let entity = commands
                .spawn((
                    Text2d::new(glyph.to_string()),
                    TextFont {
                        font_size: MARKER_FONT_SIZE,
                        ..default()
                    },
                    TextColor(color),
                    TextLayout::new_with_justify(Justify::Center),
                    Transform::from_translation(pos),
                    AwarenessMarker,
                ))
                .id();
            markers.map.insert(*object_id, entity);
        }
    }

    // Despawn markers whose NPC is gone or no longer revealed.
    markers.map.retain(|object_id, entity| {
        if seen.contains(object_id) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });
}

fn marker_glyph(awareness: NpcAwareness) -> &'static str {
    match awareness {
        NpcAwareness::Unaware => "z",
        NpcAwareness::Searching => "?",
        NpcAwareness::Alerted => "!",
    }
}

fn marker_color(awareness: NpcAwareness) -> Color {
    match awareness {
        NpcAwareness::Unaware => Color::srgb(0.45, 0.85, 0.5),
        NpcAwareness::Searching => Color::srgb(1.0, 0.84, 0.2),
        NpcAwareness::Alerted => Color::srgb(1.0, 0.27, 0.2),
    }
}
