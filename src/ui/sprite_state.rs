//! Client-side: swap a projected world object's `Sprite` (and its
//! `AnimatedSprite`, if any) when its replicated `state` changes. Pure
//! presentation — never touches authoritative components.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::world::animation::{build_animated_sprite_components, AnimatedSprite};
use crate::world::components::ClientProjectedWorldObject;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::setup::sprite_for_definition_state;
use crate::world::WorldConfig;

/// Watches `ClientGameState.world_objects[id].state` for transitions and
/// rebuilds the projected entity's sprite + (optional) `AnimatedSprite` to
/// match the new state's overrides. Tracks the last-seen state per object in
/// a `Local` map so it only acts on real transitions.
pub fn sync_object_state_visuals(
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    definitions: Res<OverworldObjectDefinitions>,
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    mut commands: Commands,
    projected_query: Query<(Entity, &ClientProjectedWorldObject)>,
    mut last_states: Local<HashMap<u64, Option<String>>>,
) {
    for (entity, projected) in &projected_query {
        let object_id = projected.object_id;
        let new_state = client_state
            .world_objects
            .get(&object_id)
            .and_then(|object| object.state.clone());

        let previous = last_states.get(&object_id).cloned();
        if previous.as_ref() == Some(&new_state) {
            continue;
        }
        let first_observation = previous.is_none();
        last_states.insert(object_id, new_state.clone());

        // First-time observation: skip the initial swap (the spawn path
        // already picked the right sprite via `sprite_for_definition_state`).
        if first_observation {
            continue;
        }

        let Some(definition) = definitions.get(&projected.definition_id) else {
            continue;
        };
        let state_ref = new_state.as_deref();

        if let Some(sheet) = definition.animation_for_state(state_ref) {
            let (animated, sprite) =
                build_animated_sprite_components(sheet, &asset_server, &mut texture_atlas_layouts);
            commands.entity(entity).insert((animated, sprite));
        } else {
            let sprite =
                sprite_for_definition_state(&asset_server, definition, &world_config, state_ref);
            commands
                .entity(entity)
                .remove::<AnimatedSprite>()
                .insert(sprite);
        }
    }

    // Drop entries for objects no longer projected so the cache doesn't grow
    // unbounded.
    let known_ids: std::collections::HashSet<u64> =
        projected_query.iter().map(|(_, p)| p.object_id).collect();
    last_states.retain(|id, _| known_ids.contains(id));
}
