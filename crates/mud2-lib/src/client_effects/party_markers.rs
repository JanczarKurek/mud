//! Over-head party markers: a green diamond above each visible party member
//! and a gold one above the party's shared focus target. Mirrors the
//! spawn/update/despawn shape of `awareness.rs`.
//!
//! Purely presentation: reads `ClientGameState.party` and the rendered
//! positions of remote-player / world-object sprites. Members outside the
//! interest radius have no sprite — no world marker; the party panel and
//! minimap cover them.
//!
//! Markers are rotated colored quads, not text — the UI font is Latin-1
//! only, so there are no diamond/star glyphs to borrow.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::world::components::{ClientProjectedWorldObject, ClientRemotePlayerVisual};

/// Above the darkness quad (999.0), just under the awareness glyphs (999.6)
/// so a focused, spotted NPC shows both without z-fighting.
const MARKER_Z: f32 = 999.5;

/// Lift above the target's tile center, in tile units. Slightly lower than
/// the awareness glyph so both fit over one head.
const MARKER_LIFT_TILES: f32 = 1.0;

const MEMBER_MARKER_SIZE: f32 = 7.0;
const FOCUS_MARKER_SIZE: f32 = 9.0;
const MEMBER_MARKER_COLOR: Color = Color::srgb(0.35, 0.85, 0.45);
const FOCUS_MARKER_COLOR: Color = Color::srgb(1.0, 0.80, 0.25);

/// Marker entity per target object id, updated in place. The focus marker
/// uses a reserved synthetic key so it can coexist with a member marker on
/// the same object (a focused *player* is unusual but legal).
#[derive(Resource, Default)]
pub struct PartyMarkers {
    map: HashMap<MarkerKey, Entity>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum MarkerKey {
    Member(u64),
    Focus,
}

/// Tag on a party marker quad.
#[derive(Component)]
pub struct PartyMarker;

pub fn sync_party_markers(
    client_state: Res<ClientGameState>,
    world_config: Res<crate::world::WorldConfig>,
    mut markers: ResMut<PartyMarkers>,
    remote_q: Query<(&ClientRemotePlayerVisual, &Transform), Without<PartyMarker>>,
    object_q: Query<(&ClientProjectedWorldObject, &Transform), Without<PartyMarker>>,
    mut marker_q: Query<&mut Transform, With<PartyMarker>>,
    mut commands: Commands,
) {
    let mut wanted: Vec<(MarkerKey, Vec3, f32, Color)> = Vec::new();

    if let Some(party) = client_state.party.as_ref() {
        // Rendered positions of remote-player sprites, keyed by object id.
        let remote_rendered: HashMap<u64, Vec3> = remote_q
            .iter()
            .map(|(visual, tf)| (visual.object_id, tf.translation))
            .collect();
        let lift = world_config.tile_size * (0.5 + MARKER_LIFT_TILES);
        let local_id = client_state.local_player_id;

        for member in &party.members {
            if Some(member.player_id) == local_id {
                continue;
            }
            let Some(base) = member
                .object_id
                .and_then(|object_id| remote_rendered.get(&object_id))
            else {
                continue;
            };
            wanted.push((
                MarkerKey::Member(member.object_id.unwrap_or_default()),
                Vec3::new(base.x, base.y + lift, MARKER_Z),
                MEMBER_MARKER_SIZE,
                MEMBER_MARKER_COLOR,
            ));
        }

        if let Some(focus_id) = party.focus_target {
            // The focus target can be any world object (usually an NPC) or,
            // in principle, a player.
            let base = object_q
                .iter()
                .find(|(proj, _)| proj.object_id == focus_id)
                .map(|(_, tf)| tf.translation)
                .or_else(|| remote_rendered.get(&focus_id).copied());
            if let Some(base) = base {
                wanted.push((
                    MarkerKey::Focus,
                    Vec3::new(base.x, base.y + lift, MARKER_Z),
                    FOCUS_MARKER_SIZE,
                    FOCUS_MARKER_COLOR,
                ));
            }
        }
    }

    let mut seen: std::collections::HashSet<MarkerKey> = std::collections::HashSet::new();
    for (key, pos, size, color) in wanted {
        seen.insert(key);
        if let Some(&entity) = markers.map.get(&key) {
            if let Ok(mut transform) = marker_q.get_mut(entity) {
                if transform.translation != pos {
                    transform.translation = pos;
                }
            }
        } else {
            let entity = commands
                .spawn((
                    Sprite::from_color(color, Vec2::splat(size)),
                    Transform::from_translation(pos)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
                    PartyMarker,
                ))
                .id();
            markers.map.insert(key, entity);
        }
    }

    // Despawn markers whose target left the party, despawned, or walked out
    // of the interest radius.
    markers.map.retain(|key, entity| {
        if seen.contains(key) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });
}
