use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::game::commands::{GameCommand, MoveDelta};
use crate::game::resources::{ClientGameState, InventoryState, PendingGameCommands};
use crate::player::components::{
    AttributeSet, BaseStats, DerivedStats, Player, PlayerIdentity, VitalStats,
};
use crate::scripting::resources::PythonConsoleState;
use crate::world::components::{DisplayedVitalStats, SpaceResident, TilePosition};
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
    mut player_query: Query<
        (
            &mut SpaceResident,
            &mut TilePosition,
            &mut VitalStats,
            &mut DisplayedVitalStats,
            Option<&PlayerIdentity>,
        ),
        With<Player>,
    >,
) {
    let Ok((
        mut space_resident,
        mut tile_position,
        mut vital_stats,
        mut displayed_vitals,
        player_identity,
    )) = player_query.single_mut()
    else {
        return;
    };

    let is_projected_client_player = player_identity.is_none();

    if is_projected_client_player {
        if let Some(client_position) = client_state.player_position {
            space_resident.space_id = client_position.space_id;
            *tile_position = client_position.tile_position;
        } else {
            *tile_position =
                TilePosition::new(world_config.map_width / 2, world_config.map_height / 2);
            space_resident.space_id = world_config.current_space_id;
        }

        if let Some(client_vitals) = client_state.player_vitals {
            vital_stats.health = client_vitals.health;
            vital_stats.max_health = client_vitals.max_health;
            vital_stats.mana = client_vitals.mana;
            vital_stats.max_mana = client_vitals.max_mana;
        }
    }

    if let Some(client_vitals) = client_state
        .player_vitals
        .filter(|_| is_projected_client_player)
    {
        displayed_vitals.health = client_vitals.health;
        displayed_vitals.max_health = client_vitals.max_health;
        displayed_vitals.mana = client_vitals.mana;
        displayed_vitals.max_mana = client_vitals.max_mana;
    } else {
        displayed_vitals.health = vital_stats.health;
        displayed_vitals.max_health = vital_stats.max_health;
        displayed_vitals.mana = vital_stats.mana;
        displayed_vitals.max_mana = vital_stats.max_mana;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::resources::ClientVitalStats;
    use crate::player::components::PlayerIdentity;
    use crate::world::components::{SpaceId, SpacePosition};

    fn default_world_config() -> WorldConfig {
        WorldConfig {
            current_space_id: SpaceId(1),
            map_width: 32,
            map_height: 24,
            tile_size: 48.0,
            fill_object_type: "grass".to_owned(),
        }
    }

    #[test]
    fn authoritative_embedded_player_keeps_authoritative_vitals() {
        let mut app = App::new();
        app.insert_resource(default_world_config());
        app.insert_resource(ClientGameState {
            player_position: Some(SpacePosition::new(SpaceId(9), TilePosition::new(7, 8))),
            player_vitals: Some(ClientVitalStats {
                health: 99.0,
                max_health: 120.0,
                mana: 20.0,
                max_mana: 30.0,
            }),
            ..default()
        });
        let entity = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity {
                    id: crate::player::components::PlayerId(0),
                },
                SpaceResident {
                    space_id: SpaceId(1),
                },
                TilePosition::new(2, 3),
                VitalStats {
                    health: 14.0,
                    max_health: 35.0,
                    mana: 4.0,
                    max_mana: 10.0,
                },
                DisplayedVitalStats::default(),
            ))
            .id();

        app.add_systems(Update, sync_player_client_state);
        app.update();

        let entity_ref = app.world().entity(entity);
        let space_resident = entity_ref.get::<SpaceResident>().unwrap();
        let tile_position = entity_ref.get::<TilePosition>().unwrap();
        let vital_stats = entity_ref.get::<VitalStats>().unwrap();
        let displayed_vitals = entity_ref.get::<DisplayedVitalStats>().unwrap();

        assert_eq!(space_resident.space_id, SpaceId(1));
        assert_eq!(*tile_position, TilePosition::new(2, 3));
        assert_eq!(vital_stats.health, 14.0);
        assert_eq!(vital_stats.max_health, 35.0);
        assert_eq!(displayed_vitals.health, 14.0);
        assert_eq!(displayed_vitals.max_health, 35.0);
    }

    #[test]
    fn projected_client_player_tracks_client_state() {
        let mut app = App::new();
        app.insert_resource(default_world_config());
        app.insert_resource(ClientGameState {
            player_position: Some(SpacePosition::new(SpaceId(5), TilePosition::new(10, 11))),
            player_vitals: Some(ClientVitalStats {
                health: 12.0,
                max_health: 40.0,
                mana: 6.0,
                max_mana: 18.0,
            }),
            ..default()
        });
        let entity = app
            .world_mut()
            .spawn((
                Player,
                SpaceResident {
                    space_id: SpaceId(1),
                },
                TilePosition::new(0, 0),
                VitalStats::full(1.0, 0.0),
                DisplayedVitalStats::default(),
            ))
            .id();

        app.add_systems(Update, sync_player_client_state);
        app.update();

        let entity_ref = app.world().entity(entity);
        let space_resident = entity_ref.get::<SpaceResident>().unwrap();
        let tile_position = entity_ref.get::<TilePosition>().unwrap();
        let vital_stats = entity_ref.get::<VitalStats>().unwrap();
        let displayed_vitals = entity_ref.get::<DisplayedVitalStats>().unwrap();

        assert_eq!(space_resident.space_id, SpaceId(5));
        assert_eq!(*tile_position, TilePosition::new(10, 11));
        assert_eq!(vital_stats.health, 12.0);
        assert_eq!(vital_stats.max_health, 40.0);
        assert_eq!(displayed_vitals.mana, 6.0);
        assert_eq!(displayed_vitals.max_mana, 18.0);
    }
}
