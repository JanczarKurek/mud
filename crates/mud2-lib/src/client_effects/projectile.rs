use bevy::prelude::*;

use crate::game::resources::{ClientGameState, GameUiEvent, PendingGameUiEvents};
use crate::world::components::TilePosition;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::resources::ViewScrollOffset;
use crate::world::WorldConfig;

/// Default flight time for cosmetic-only projectiles (ranged physical attacks).
/// Spell missiles override this with a distance-derived `duration_seconds`.
pub const PROJECTILE_DURATION_SECONDS: f32 = 0.25;
const PROJECTILE_Z: f32 = 900.0;

#[derive(Component)]
pub struct Projectile {
    pub from_tile: TilePosition,
    pub to_tile: TilePosition,
    pub elapsed: f32,
    pub duration: f32,
    /// When set, the missile homes — its endpoint re-reads this object's
    /// current tile each frame (matches the server's locked damage target).
    pub target_object_id: Option<u64>,
}

pub fn consume_projectile_events(
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    asset_server: Res<AssetServer>,
    world_config: Res<WorldConfig>,
    definitions: Res<OverworldObjectDefinitions>,
    mut commands: Commands,
) {
    let events = std::mem::take(&mut pending_ui_events.events);
    for event in events {
        match event {
            GameUiEvent::ProjectileFired {
                from_tile,
                to_tile,
                sprite_definition_id,
                duration_seconds,
                target_object_id,
            } => {
                let Some(definition) = definitions.get(&sprite_definition_id) else {
                    continue;
                };
                let size = definition.render.sprite_pixel_size(world_config.tile_size) * 0.6;
                let sprite = match &definition.render.sprite_path {
                    Some(path) => {
                        let mut sprite = Sprite::from_image(asset_server.load(path));
                        sprite.custom_size = Some(size);
                        sprite
                    }
                    None => Sprite::from_color(definition.debug_color(), size),
                };
                commands.spawn((
                    sprite,
                    Transform::from_xyz(0.0, 0.0, PROJECTILE_Z),
                    Projectile {
                        from_tile,
                        to_tile,
                        elapsed: 0.0,
                        duration: duration_seconds.max(f32::EPSILON),
                        target_object_id,
                    },
                ));
            }
            other => pending_ui_events.events.push(other),
        }
    }
}

pub fn advance_projectiles(
    time: Res<Time>,
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    view_scroll: Res<ViewScrollOffset>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut Projectile, &mut Transform)>,
) {
    let Some(player_tile) = client_state.player_tile_position else {
        return;
    };
    let tile_size = world_config.tile_size;
    for (entity, mut projectile, mut transform) in &mut query {
        projectile.elapsed += time.delta_secs();
        let t = (projectile.elapsed / projectile.duration).clamp(0.0, 1.0);
        // Homing missiles re-read the target's current tile so the bolt curves
        // to follow a fleeing target; fall back to the fixed aim tile if the
        // target is gone (despawned / out of view).
        let to_tile = projectile
            .target_object_id
            .and_then(|id| current_object_tile(id, &client_state))
            .unwrap_or(projectile.to_tile);
        let from = Vec2::new(
            (projectile.from_tile.x - player_tile.x) as f32 * tile_size,
            (projectile.from_tile.y - player_tile.y) as f32 * tile_size,
        );
        let to = Vec2::new(
            (to_tile.x - player_tile.x) as f32 * tile_size,
            (to_tile.y - player_tile.y) as f32 * tile_size,
        );
        let pos = from.lerp(to, t) + view_scroll.lerp.current;
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        if t >= 1.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Current tile of an object as the client knows it — local player, a world
/// object, or a remote player. Mirrors `vfx::lookup_object_view` but returns
/// just the tile (projectiles position relative to the local player's tile).
fn current_object_tile(object_id: u64, client_state: &ClientGameState) -> Option<TilePosition> {
    if client_state.local_player_object_id == Some(object_id) {
        return client_state.player_tile_position;
    }
    if let Some(state) = client_state.world_objects.get(&object_id) {
        return Some(state.tile_position);
    }
    client_state
        .remote_players
        .values()
        .find(|remote| remote.object_id == object_id)
        .map(|remote| remote.tile_position)
}
