use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::player::components::{MovementCooldown, Player};
use crate::scripting::resources::PythonConsoleState;
use crate::world::components::{Collider, TilePosition};
use crate::world::WorldConfig;

pub fn move_player_on_grid(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    world_config: Res<WorldConfig>,
    console_state: Option<Res<PythonConsoleState>>,
    collider_query: Query<&TilePosition, (With<Collider>, Without<Player>)>,
    mut player_query: Query<(&mut TilePosition, &mut MovementCooldown), With<Player>>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }

    let Ok((mut tile_position, mut movement_cooldown)) = player_query.single_mut() else {
        return;
    };

    movement_cooldown.remaining_seconds =
        (movement_cooldown.remaining_seconds - time.delta_secs()).max(0.0);

    let Some(delta) = movement_direction(&keyboard_input) else {
        return;
    };

    if movement_cooldown.remaining_seconds > 0.0 {
        return;
    }

    step_player(&mut tile_position, delta, &world_config, &collider_query);
    movement_cooldown.remaining_seconds = movement_cooldown.step_interval_seconds;
}

fn movement_direction(keyboard_input: &ButtonInput<KeyCode>) -> Option<IVec2> {
    if keyboard_input.pressed(KeyCode::ArrowUp) || keyboard_input.pressed(KeyCode::KeyW) {
        Some(IVec2::new(0, 1))
    } else if keyboard_input.pressed(KeyCode::ArrowDown) || keyboard_input.pressed(KeyCode::KeyS) {
        Some(IVec2::new(0, -1))
    } else if keyboard_input.pressed(KeyCode::ArrowLeft) || keyboard_input.pressed(KeyCode::KeyA) {
        Some(IVec2::new(-1, 0))
    } else if keyboard_input.pressed(KeyCode::ArrowRight) || keyboard_input.pressed(KeyCode::KeyD) {
        Some(IVec2::new(1, 0))
    } else {
        None
    }
}

fn step_player(
    tile_position: &mut TilePosition,
    delta: IVec2,
    world_config: &WorldConfig,
    collider_query: &Query<&TilePosition, (With<Collider>, Without<Player>)>,
) {
    let target_position = TilePosition::new(
        (tile_position.x + delta.x).clamp(0, world_config.map_width - 1),
        (tile_position.y + delta.y).clamp(0, world_config.map_height - 1),
    );

    let blocked = collider_query
        .iter()
        .any(|collider_position| *collider_position == target_position);

    if blocked {
        return;
    }

    *tile_position = target_position;
}
