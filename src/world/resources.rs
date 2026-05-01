use std::collections::HashMap;

use bevy::prelude::*;

/// Pixel offset added to all non-player entity translations each frame,
/// driven by the local player's most recent tile move. Lerps to zero over
/// the movement duration to produce a Tibia-style smooth viewport scroll.
#[derive(Resource, Default, Clone, Debug)]
pub struct ViewScrollOffset {
    pub current: Vec2,
    pub elapsed: f32,
    pub duration: f32,
}

use crate::player::components::PlayerId;
use crate::world::components::SpaceId;
use crate::world::map_layout::{SpaceLightingDef, SpacePermanence};

#[derive(Resource, Default)]
pub struct ClientWorldProjectionState {
    pub entities: HashMap<u64, Entity>,
    pub active_space_id: Option<SpaceId>,
}

#[derive(Resource, Default)]
pub struct ClientRemotePlayerProjectionState {
    pub entities: HashMap<PlayerId, Entity>,
}

#[derive(Clone, Debug)]
pub struct RuntimeSpace {
    pub id: SpaceId,
    pub authored_id: String,
    pub width: i32,
    pub height: i32,
    pub fill_floor_type: String,
    pub permanence: SpacePermanence,
    pub instance_owner: Option<PortalInstanceKey>,
    pub lighting: SpaceLightingDef,
}

impl RuntimeSpace {
    pub const fn contains(&self, tile_position: crate::world::components::TilePosition) -> bool {
        tile_position.x >= 0
            && tile_position.y >= 0
            && tile_position.x < self.width
            && tile_position.y < self.height
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PortalInstanceKey {
    pub source_space_id: SpaceId,
    pub portal_id: String,
}

#[derive(Resource, Default)]
pub struct SpaceManager {
    pub next_space_id: u64,
    pub spaces: HashMap<SpaceId, RuntimeSpace>,
    pub persistent_spaces_by_authored_id: HashMap<String, SpaceId>,
    pub portal_instances: HashMap<PortalInstanceKey, SpaceId>,
}

impl SpaceManager {
    pub fn allocate_space_id(&mut self) -> SpaceId {
        let space_id = SpaceId(self.next_space_id);
        self.next_space_id += 1;
        space_id
    }

    pub fn insert_space(&mut self, runtime_space: RuntimeSpace) {
        if runtime_space.permanence.is_persistent() {
            self.persistent_spaces_by_authored_id
                .insert(runtime_space.authored_id.clone(), runtime_space.id);
        }
        if let Some(instance_owner) = &runtime_space.instance_owner {
            self.portal_instances
                .insert(instance_owner.clone(), runtime_space.id);
        }
        self.spaces.insert(runtime_space.id, runtime_space);
    }

    pub fn get(&self, space_id: SpaceId) -> Option<&RuntimeSpace> {
        self.spaces.get(&space_id)
    }

    pub fn persistent_space_id(&self, authored_id: &str) -> Option<SpaceId> {
        self.persistent_spaces_by_authored_id
            .get(authored_id)
            .copied()
    }

    pub fn portal_instance(&self, key: &PortalInstanceKey) -> Option<SpaceId> {
        self.portal_instances.get(key).copied()
    }

    pub fn remove_space(&mut self, space_id: SpaceId) -> Option<RuntimeSpace> {
        let runtime_space = self.spaces.remove(&space_id)?;
        if runtime_space.permanence.is_persistent() {
            self.persistent_spaces_by_authored_id
                .remove(&runtime_space.authored_id);
        }
        if let Some(instance_owner) = &runtime_space.instance_owner {
            self.portal_instances.remove(instance_owner);
        }
        Some(runtime_space)
    }
}
