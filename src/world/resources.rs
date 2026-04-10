use std::collections::HashMap;

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct ClientWorldProjectionState {
    pub entities: HashMap<u64, Entity>,
}
