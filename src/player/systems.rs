use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::combat::components::AttackProfile;
use crate::combat::damage_expr::DamageExpr;
use crate::game::commands::{GameCommand, MoveDelta, RotationDirection};
use crate::game::resources::{ClientGameState, InventoryState, PendingGameCommands};
use crate::player::components::{
    AttributeSet, BaseStats, DerivedStats, Player, PlayerIdentity, VitalStats, WeaponDamage,
};
use crate::scripting::resources::PythonConsoleState;
use crate::world::components::{
    DisplayedVitalStats, Facing, SpaceResident, TilePosition, ViewPosition,
};
use crate::world::object_definitions::{
    AttackProfileKindDef, EquipmentSlot, OverworldObjectDefinitions,
};
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
            &mut AttackProfile,
            &mut WeaponDamage,
        ),
        With<Player>,
    >,
) {
    for (
        base_stats,
        inventory_state,
        mut derived_stats,
        mut vital_stats,
        mut attack_profile,
        mut weapon_damage,
    ) in &mut player_query
    {
        let mut attributes = base_stats.attributes;
        let mut max_health = base_stats.max_health;
        let mut max_mana = base_stats.max_mana;
        let mut storage_slots = base_stats.storage_slots;
        let mut equipped_weapon_def_id: Option<String> = None;

        for (slot, equipped_item) in &inventory_state.equipment_slots {
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

            if *slot == EquipmentSlot::Weapon {
                equipped_weapon_def_id = Some(type_id.to_owned());
            }
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

        let mut next_profile = AttackProfile::melee();
        let mut next_damage = DamageExpr::melee_default();
        if let Some(def_id) = equipped_weapon_def_id {
            if let Some(definition) = definitions.get(&def_id) {
                if let Some(expr) = definition
                    .damage
                    .as_deref()
                    .and_then(|raw| DamageExpr::parse(raw).ok())
                {
                    next_damage = expr;
                }
                if let Some(profile_def) = definition.attack_profile {
                    match profile_def.kind {
                        AttackProfileKindDef::Melee => {
                            next_profile = AttackProfile::melee();
                        }
                        AttackProfileKindDef::Ranged => {
                            let base_range = definition.base_range_tiles.unwrap_or(4).max(1);
                            let agility_bonus = derived_stats.attributes.agility / 4;
                            next_profile =
                                AttackProfile::ranged((base_range + agility_bonus).max(1));
                        }
                    }
                }
            }
        }

        if *attack_profile != next_profile {
            *attack_profile = next_profile;
        }
        if weapon_damage.0 != next_damage {
            weapon_damage.0 = next_damage;
        }
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

/// Ctrl+Q rotates a nearby rotatable object counter-clockwise, Ctrl+E clockwise.
/// Picks the rotatable object within Chebyshev-1 of the local player, tie-broken
/// by Manhattan distance then object_id so the choice is deterministic across
/// frames. Silent no-op if no rotatable object is adjacent.
pub fn rotate_nearby_object_on_shortcut(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    console_state: Option<Res<PythonConsoleState>>,
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }

    let ctrl_held = keyboard_input.pressed(KeyCode::ControlLeft)
        || keyboard_input.pressed(KeyCode::ControlRight);
    if !ctrl_held {
        return;
    }

    let rotation = if keyboard_input.just_pressed(KeyCode::KeyQ) {
        RotationDirection::CounterClockwise
    } else if keyboard_input.just_pressed(KeyCode::KeyE) {
        RotationDirection::Clockwise
    } else {
        return;
    };

    let Some(player_tile) = client_state.player_tile_position else {
        return;
    };
    let Some(player_space) = client_state.player_position.map(|p| p.space_id) else {
        return;
    };

    let best = client_state
        .world_objects
        .values()
        .filter(|object| object.is_rotatable && object.position.space_id == player_space)
        .filter(|object| {
            let t = object.tile_position;
            t.z == player_tile.z
                && (t.x - player_tile.x).abs() <= 1
                && (t.y - player_tile.y).abs() <= 1
        })
        .min_by_key(|object| {
            let dx = (object.tile_position.x - player_tile.x).abs();
            let dy = (object.tile_position.y - player_tile.y).abs();
            (dx + dy, object.object_id)
        });

    let Some(target) = best else {
        return;
    };

    pending_commands.push(GameCommand::RotateObject {
        object_id: target.object_id,
        rotation,
    });
}

/// View-sync for the locally-simulated (authoritative) player. Copies authoritative
/// `VitalStats` into the presentation-only `DisplayedVitalStats` component. Runs in
/// EmbeddedClient mode where a `PlayerIdentity` is present on the single Player entity.
pub fn sync_authoritative_player_display(
    mut player_query: Query<
        (&VitalStats, &mut DisplayedVitalStats),
        (With<Player>, With<PlayerIdentity>),
    >,
) {
    let Ok((vital_stats, mut displayed_vitals)) = player_query.single_mut() else {
        return;
    };
    displayed_vitals.health = vital_stats.health;
    displayed_vitals.max_health = vital_stats.max_health;
    displayed_vitals.mana = vital_stats.mana;
    displayed_vitals.max_mana = vital_stats.max_mana;
}

/// Mirrors the authoritative `SpaceResident` + `TilePosition` onto the presentation-only
/// `ViewPosition` for the locally-simulated player in EmbeddedClient mode. In TcpClient
/// mode the projected player has no `PlayerIdentity`, so this system is a no-op and
/// `sync_projected_player_from_client_state` drives the view instead.
pub fn sync_authoritative_player_position_view(
    mut query: Query<
        (&SpaceResident, &TilePosition, &mut ViewPosition),
        (With<Player>, With<PlayerIdentity>),
    >,
) {
    for (space_resident, tile_position, mut view) in &mut query {
        view.space_id = space_resident.space_id;
        view.tile = *tile_position;
    }
}

/// View-sync for the projected (remote-authoritative) player in TcpClient mode.
/// Reads from `ClientGameState` — the single source of truth on the client — and
/// writes to view components only. Never touches the authoritative `VitalStats`,
/// `SpaceResident`, or `TilePosition` components, which are inert on a projected
/// entity (see EmbeddedClient Invariant in CLAUDE.md).
pub fn sync_projected_player_from_client_state(
    client_state: Res<ClientGameState>,
    world_config: Res<WorldConfig>,
    mut player_query: Query<
        (
            &mut ViewPosition,
            &mut DisplayedVitalStats,
            Option<&mut Facing>,
        ),
        (With<Player>, Without<PlayerIdentity>),
    >,
) {
    let Ok((mut view, mut displayed_vitals, facing)) = player_query.single_mut() else {
        return;
    };

    if let Some(client_position) = client_state.player_position {
        view.space_id = client_position.space_id;
        view.tile = client_position.tile_position;
    } else {
        view.tile = TilePosition::ground(world_config.map_width / 2, world_config.map_height / 2);
        view.space_id = world_config.current_space_id;
    }

    if let Some(client_vitals) = client_state.player_vitals {
        displayed_vitals.health = client_vitals.health;
        displayed_vitals.max_health = client_vitals.max_health;
        displayed_vitals.mana = client_vitals.mana;
        displayed_vitals.max_mana = client_vitals.max_mana;
    }

    if let (Some(mut facing), Some(direction)) = (facing, client_state.player_facing) {
        if facing.0 != direction {
            facing.0 = direction;
        }
    }
}

fn movement_direction(keyboard_input: &ButtonInput<KeyCode>) -> Option<MoveDelta> {
    if keyboard_input.pressed(KeyCode::ControlLeft) || keyboard_input.pressed(KeyCode::ControlRight)
    {
        return None;
    }
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
            fill_floor_type: "grass".to_owned(),
        }
    }

    #[test]
    fn authoritative_embedded_player_mirrors_position_and_vitals() {
        let mut app = App::new();
        app.insert_resource(default_world_config());
        app.insert_resource(ClientGameState {
            player_position: Some(SpacePosition::new(SpaceId(9), TilePosition::ground(7, 8))),
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
                TilePosition::ground(2, 3),
                ViewPosition {
                    space_id: SpaceId(0),
                    tile: TilePosition::ground(0, 0),
                },
                VitalStats {
                    health: 14.0,
                    max_health: 35.0,
                    mana: 4.0,
                    max_mana: 10.0,
                },
                DisplayedVitalStats::default(),
            ))
            .id();

        app.add_systems(
            Update,
            (
                sync_authoritative_player_display,
                sync_authoritative_player_position_view,
                sync_projected_player_from_client_state,
            ),
        );
        app.update();

        let entity_ref = app.world().entity(entity);
        let view = entity_ref.get::<ViewPosition>().unwrap();
        let vital_stats = entity_ref.get::<VitalStats>().unwrap();
        let displayed_vitals = entity_ref.get::<DisplayedVitalStats>().unwrap();

        // View mirrors authoritative position — ClientGameState is ignored for this entity.
        assert_eq!(view.space_id, SpaceId(1));
        assert_eq!(view.tile, TilePosition::ground(2, 3));
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
            player_position: Some(SpacePosition::new(SpaceId(5), TilePosition::ground(10, 11))),
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
                ViewPosition {
                    space_id: SpaceId(1),
                    tile: TilePosition::ground(0, 0),
                },
                VitalStats::full(1.0, 0.0),
                DisplayedVitalStats::default(),
            ))
            .id();

        app.add_systems(
            Update,
            (
                sync_authoritative_player_display,
                sync_authoritative_player_position_view,
                sync_projected_player_from_client_state,
            ),
        );
        app.update();

        let entity_ref = app.world().entity(entity);
        let view = entity_ref.get::<ViewPosition>().unwrap();
        let vital_stats = entity_ref.get::<VitalStats>().unwrap();
        let displayed_vitals = entity_ref.get::<DisplayedVitalStats>().unwrap();

        // Projected player reads ClientGameState and writes only to view components.
        assert_eq!(view.space_id, SpaceId(5));
        assert_eq!(view.tile, TilePosition::ground(10, 11));
        // Authoritative VitalStats on a projected entity is inert — presentation
        // layer reads DisplayedVitalStats instead.
        assert_eq!(vital_stats.health, 1.0);
        assert_eq!(vital_stats.max_health, 1.0);
        assert_eq!(displayed_vitals.health, 12.0);
        assert_eq!(displayed_vitals.max_health, 40.0);
        assert_eq!(displayed_vitals.mana, 6.0);
        assert_eq!(displayed_vitals.max_mana, 18.0);
    }
}
