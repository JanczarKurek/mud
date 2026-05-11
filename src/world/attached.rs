//! ECS-level "attached to a world object" relationship.
//!
//! An entity carrying `AttachedToObject { object_id, ... }` has its rendered
//! `Transform.translation` overwritten each frame from the target object's
//! rendered transform. The follower inherits the target's smoothing for free
//! — `VisualOffset` for projected world objects / NPCs, `camera_follow` for
//! the local player. The system runs *after* `sync_tile_transforms`,
//! `sync_player_z`, and `camera_follow`, so every target has its final
//! transform written by the time the follower copies it.
//!
//! The component is intentionally generic — VFX attachments are the first
//! consumer, but any future "follow this object" visual (held-weapon
//! overlays, hover markers, status icons floating above heads) can attach
//! without writing a new system.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::player::components::Player;
use crate::world::components::{ClientProjectedWorldObject, ClientRemotePlayerVisual};

#[derive(Component, Clone, Copy, Debug)]
pub struct AttachedToObject {
    pub object_id: u64,
    /// Pixel offset from the target's rendered position.
    pub offset_pixels: Vec2,
    /// Z bump on top of the target's z. A small positive value sorts the
    /// follower just in front; 0.0 sorts together with the target.
    pub z_offset: f32,
}

impl AttachedToObject {
    pub fn at(object_id: u64) -> Self {
        Self {
            object_id,
            offset_pixels: Vec2::ZERO,
            z_offset: 0.05,
        }
    }
}

pub fn sync_attached_object_visuals(
    client_state: Res<ClientGameState>,
    projected_q: Query<(&ClientProjectedWorldObject, &Transform), Without<AttachedToObject>>,
    remote_q: Query<
        (&ClientRemotePlayerVisual, &Transform),
        (Without<AttachedToObject>, Without<ClientProjectedWorldObject>),
    >,
    local_player_q: Query<
        &Transform,
        (
            With<Player>,
            Without<AttachedToObject>,
            Without<ClientProjectedWorldObject>,
            Without<ClientRemotePlayerVisual>,
        ),
    >,
    mut follower_q: Query<(&AttachedToObject, &mut Transform), Without<Player>>,
) {
    let mut targets: HashMap<u64, Vec3> = HashMap::new();
    for (proj, tf) in &projected_q {
        targets.insert(proj.object_id, tf.translation);
    }
    for (rem, tf) in &remote_q {
        targets.insert(rem.object_id, tf.translation);
    }
    if let Some(local_id) = client_state.local_player_object_id {
        if let Ok(tf) = local_player_q.single() {
            targets.insert(local_id, tf.translation);
        }
    }

    for (attach, mut tf) in &mut follower_q {
        let Some(target_translation) = targets.get(&attach.object_id) else {
            continue;
        };
        let new = Vec3::new(
            target_translation.x + attach.offset_pixels.x,
            target_translation.y + attach.offset_pixels.y,
            target_translation.z + attach.z_offset,
        );
        if tf.translation != new {
            tf.translation = new;
        }
    }
}
