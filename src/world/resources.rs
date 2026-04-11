use std::collections::HashMap;

use bevy::prelude::*;

use crate::player::components::PlayerId;

#[derive(Resource, Default)]
pub struct ClientWorldProjectionState {
    pub entities: HashMap<u64, Entity>,
}

#[derive(Resource, Default)]
pub struct ClientRemotePlayerProjectionState {
    pub entities: HashMap<PlayerId, Entity>,
}
