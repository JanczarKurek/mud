use std::collections::HashMap;

use bevy::prelude::*;

use crate::world::map_layout::MapLayout;

#[derive(Resource, Default)]
pub struct ObjectRegistry {
    type_ids: HashMap<u64, String>,
    next_runtime_id: u64,
}

impl ObjectRegistry {
    pub fn from_map_layout(map_layout: &MapLayout) -> Self {
        let mut type_ids = HashMap::new();
        let mut max_id = 0;

        for object in &map_layout.resolved_objects {
            type_ids.insert(object.id, object.type_id.clone());
            max_id = max_id.max(object.id);
        }

        Self {
            type_ids,
            next_runtime_id: max_id + 1,
        }
    }

    pub fn type_id(&self, object_id: u64) -> Option<&str> {
        self.type_ids.get(&object_id).map(String::as_str)
    }

    pub fn allocate_runtime_id(&mut self, type_id: impl Into<String>) -> u64 {
        let object_id = self.next_runtime_id;
        self.next_runtime_id += 1;
        self.type_ids.insert(object_id, type_id.into());
        object_id
    }
}
