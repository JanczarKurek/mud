use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::game::commands::{GameCommand, MoveDelta};
use crate::game::resources::{ClientGameState, InventoryState, PendingGameCommands};
use crate::player::components::{AttributeSet, BaseStats, DerivedStats, Player, VitalStats};
use crate::scripting::resources::PythonConsoleState;
use crate::world::components::TilePosition;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::object_registry::ObjectRegistry;
use crate::world::WorldConfig;

pub fn refresh_derived_player_stats(
    object_registry: Res<ObjectRegistry>,
    definitions: Res<OverworldObjectDefinitions>,
    mut player_query: Query<
        (
            &BaseStats,
            &InventoryState,
            &mut DerivedStats,
            &mut VitalStats,
        ),
        With<Player>,
    >,
) {
    for (base_stats, inventory_state, mut derived_stats, mut vital_stats) in &mut player_query {
        let mut attributes = base_stats.attributes;
        let mut max_health = base_stats.max_health;
        let mut max_mana = base_stats.max_mana;
        let mut storage_slots = base_stats.storage_slots;

        for (_, equipped_item) in &inventory_state.equipment_slots {
            let Some(object_id) = equipped_item else {
                continue;
            };
            let Some(type_id) = object_registry.type_id(*object_id) else {
                continue;
            };
            let Some(definition) = definitions.get(type_id) else {
                continue;
            };

            attributes.add_assign(AttributeSet {
                strength: definition.stats.strength,
                agility: definition.stats.agility,
                constitution: definition.stats.constitution,
                willpower: definition.stats.willpower,
                charisma: definition.stats.charisma,
                focus: definition.stats.focus,
            });
            max_health += definition.stats.max_health;
            max_mana += definition.stats.max_mana;
            storage_slots += definition.stats.storage_slots;
        }

        let effective_base = BaseStats {
            attributes,
            max_health,
            max_mana,
            storage_slots,
        };
        *derived_stats = DerivedStats::from_base(&effective_base);

        vital_stats.max_health = derived_stats.max_health as f32;
        vital_stats.max_mana = derived_stats.max_mana as f32;
        vital_stats.health = vital_stats.health.clamp(0.0, vital_stats.max_health);
        vital_stats.mana = vital_stats.mana.clamp(0.0, vital_stats.max_mana);
    }
}

pub fn move_player_on_grid(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    console_state: Option<Res<PythonConsoleState>>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }

    let Some(delta) = movement_direction(&keyboard_input) else {
        return;
    };

    pending_commands.push(GameCommand::MovePlayer { delta });
}

pub fn sync_player_client_state(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    mut player_query: Query<(&mut TilePosition, &mut VitalStats), With<Player>>,
) {
    let Ok((mut tile_position, mut vital_stats)) = player_query.single_mut() else {
        return;
    };

    if let Some(client_tile_position) = client_state.player_tile_position {
        *tile_position = client_tile_position;
    } else {
        *tile_position = TilePosition::new(world_config.map_width / 2, world_config.map_height / 2);
    }

    if let Some(client_vitals) = client_state.player_vitals {
        vital_stats.health = client_vitals.health;
        vital_stats.max_health = client_vitals.max_health;
        vital_stats.mana = client_vitals.mana;
        vital_stats.max_mana = client_vitals.max_mana;
    }
}

fn movement_direction(keyboard_input: &ButtonInput<KeyCode>) -> Option<MoveDelta> {
    if keyboard_input.pressed(KeyCode::ArrowUp) || keyboard_input.pressed(KeyCode::KeyW) {
        Some(MoveDelta { x: 0, y: 1 })
    } else if keyboard_input.pressed(KeyCode::ArrowDown) || keyboard_input.pressed(KeyCode::KeyS) {
        Some(MoveDelta { x: 0, y: -1 })
    } else if keyboard_input.pressed(KeyCode::ArrowLeft) || keyboard_input.pressed(KeyCode::KeyA) {
        Some(MoveDelta { x: -1, y: 0 })
    } else if keyboard_input.pressed(KeyCode::ArrowRight) || keyboard_input.pressed(KeyCode::KeyD) {
        Some(MoveDelta { x: 1, y: 0 })
    } else {
        None
    }
}
