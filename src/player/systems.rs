use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;

use crate::combat::components::AttackProfile;
use crate::combat::damage_expr::DamageExpr;
use crate::game::commands::{GameCommand, MoveDelta, RotationDirection};
use crate::game::resources::{ClientGameState, InventoryState, PendingGameCommands};
use crate::player::classes::Class;
use crate::player::components::{
    AttributeSet, BaseStats, CurrentCarryWeight, DefenseStats, DerivedStats, Encumbered,
    MaxCarryWeight, Player, PlayerIdentity, VitalStats, WeaponDamage,
};
use crate::player::progression::Experience;
use crate::scripting::resources::PythonConsoleState;
use crate::ui::settings::model::{Action, Keybindings, MovementBindings, MovementDir};
use crate::world::components::{
    DisplayedVitalStats, Facing, SpaceResident, TilePosition, ViewPosition,
};
use crate::world::object_definitions::{
    AttackProfileKindDef, EquipmentSlot, OverworldObjectDefinitions,
};
use crate::world::WorldConfig;

pub fn refresh_derived_player_stats(
    definitions: Res<OverworldObjectDefinitions>,
    mut commands: Commands,
    mut player_query: Query<
        (
            Entity,
            &BaseStats,
            &InventoryState,
            &mut DerivedStats,
            &mut VitalStats,
            &mut AttackProfile,
            &mut WeaponDamage,
            &mut DefenseStats,
            Option<&mut MaxCarryWeight>,
            Option<&mut CurrentCarryWeight>,
            Has<Encumbered>,
            Option<&Class>,
            Option<&Experience>,
        ),
        With<Player>,
    >,
) {
    for (
        entity,
        base_stats,
        inventory_state,
        mut derived_stats,
        mut vital_stats,
        mut attack_profile,
        mut weapon_damage,
        mut defense_stats,
        max_carry,
        current_carry,
        was_encumbered,
        class,
        experience,
    ) in &mut player_query
    {
        let mut attributes = base_stats.attributes;
        let mut max_health = base_stats.max_health;
        let mut max_mana = base_stats.max_mana;
        let mut storage_slots = base_stats.storage_slots;
        let mut equipped_weapon_def_id: Option<String> = None;
        let mut armor_total: i32 = 0;
        let mut block_total: i32 = 0;
        let mut dodge_total: i32 = 0;
        let mut block_chance_total: i32 = 0;

        for (slot, equipped_item) in &inventory_state.equipment_slots {
            let Some(item) = equipped_item else {
                continue;
            };
            let Some(definition) = definitions.get(&item.type_id) else {
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
            dodge_total += definition.dodge_bonus;

            match slot {
                EquipmentSlot::Weapon => {
                    equipped_weapon_def_id = Some(item.type_id.clone());
                }
                EquipmentSlot::Armor
                | EquipmentSlot::Helmet
                | EquipmentSlot::Legs
                | EquipmentSlot::Boots => {
                    armor_total += definition.armor;
                }
                EquipmentSlot::Shield => {
                    block_total += definition.block;
                    block_chance_total += definition.block_chance;
                }
                _ => {}
            }
        }

        let effective_base = BaseStats {
            attributes,
            max_health,
            max_mana,
            storage_slots,
        };
        let resolved_class = class.copied().unwrap_or_default();
        let resolved_level = experience.map(|e| e.level).unwrap_or(1);
        let new_derived =
            DerivedStats::from_base_with_class(&effective_base, resolved_class, resolved_level);

        // On level-up, max_* grew. Top up current vitals by the delta so the
        // player feels the progression bump rather than just seeing the bar
        // ratio shrink (mirrors `progression.md` §4.3 step 1 — a level-up
        // heals).
        let prev_max_health = derived_stats.max_health;
        let prev_max_mana = derived_stats.max_mana;
        let health_delta = (new_derived.max_health - prev_max_health).max(0) as f32;
        let mana_delta = (new_derived.max_mana - prev_max_mana).max(0) as f32;

        *derived_stats = new_derived;

        vital_stats.max_health = derived_stats.max_health as f32;
        vital_stats.max_mana = derived_stats.max_mana as f32;
        vital_stats.health = (vital_stats.health + health_delta).clamp(0.0, vital_stats.max_health);
        vital_stats.mana = (vital_stats.mana + mana_delta).clamp(0.0, vital_stats.max_mana);

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
                if let Some(profile_def) = definition.attack_profile.as_ref() {
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

        let next_defense = DefenseStats {
            armor: armor_total,
            block: block_total,
            dodge_bonus: dodge_total,
            block_chance: block_chance_total,
        };
        if *defense_stats != next_defense {
            *defense_stats = next_defense;
        }

        // Carry weight: cap derives from final (post-equipment) STR; current
        // weight is the sum of all carried items including nested pouch
        // contents.
        let next_max = MaxCarryWeight::from_strength(derived_stats.attributes.strength);
        let next_current = CurrentCarryWeight(inventory_state.total_weight(&definitions));
        match max_carry {
            Some(mut existing) if *existing != next_max => *existing = next_max,
            None => {
                commands.entity(entity).insert(next_max);
            }
            _ => {}
        }
        match current_carry {
            Some(mut existing) if *existing != next_current => *existing = next_current,
            None => {
                commands.entity(entity).insert(next_current);
            }
            _ => {}
        }
        let now_encumbered = next_current.0 > next_max.soft_cap;
        if now_encumbered && !was_encumbered {
            commands.entity(entity).insert(Encumbered);
        } else if !now_encumbered && was_encumbered {
            commands.entity(entity).remove::<Encumbered>();
        }
    }
}

pub fn move_player_on_grid(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    keybindings: Res<Keybindings>,
    console_state: Option<Res<PythonConsoleState>>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }

    let Some(delta) = movement_direction(&keyboard_input, &keybindings.movement) else {
        return;
    };

    let climb =
        keyboard_input.pressed(KeyCode::ShiftLeft) || keyboard_input.pressed(KeyCode::ShiftRight);
    pending_commands.push(GameCommand::MovePlayer { delta, climb });
}

/// `H` (no modifier) sets the player's respawn point to their current tile.
/// Triggered by `just_pressed` so a held key doesn't spam the queue. Suppressed
/// while the Python console is focused so chord-style input there isn't
/// hijacked.
pub fn set_home_on_keypress(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    keybindings: Res<Keybindings>,
    console_state: Option<Res<PythonConsoleState>>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }
    // The default binding carries no modifiers, and `just_pressed` enforces
    // exact modifier state — so a held Ctrl/Alt/Shift still suppresses this
    // exactly as the old explicit modifier guard did.
    if keybindings.just_pressed(Action::SetHome, &keyboard_input) {
        pending_commands.push(GameCommand::SetHome);
    }
}

/// Ctrl+Q rotates a nearby rotatable object counter-clockwise, Ctrl+E clockwise.
/// Picks the rotatable object within Chebyshev-1 of the local player, tie-broken
/// by Manhattan distance then object_id so the choice is deterministic across
/// frames. Silent no-op if no rotatable object is adjacent.
pub fn rotate_nearby_object_on_shortcut(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    keybindings: Res<Keybindings>,
    console_state: Option<Res<PythonConsoleState>>,
    client_state: Res<ClientGameState>,
    mut pending_commands: ResMut<PendingGameCommands>,
) {
    if console_state.as_ref().is_some_and(|state| state.is_open) {
        return;
    }

    let rotation = if keybindings.just_pressed(Action::RotateCcw, &keyboard_input) {
        RotationDirection::CounterClockwise
    } else if keybindings.just_pressed(Action::RotateCw, &keyboard_input) {
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

fn movement_direction(
    keyboard_input: &ButtonInput<KeyCode>,
    movement: &MovementBindings,
) -> Option<MoveDelta> {
    if keyboard_input.pressed(KeyCode::ControlLeft) || keyboard_input.pressed(KeyCode::ControlRight)
    {
        return None;
    }

    // Same accumulate-and-clamp algorithm as before (opposite keys cancel,
    // diagonals add both axes); only the key source moved into the
    // remappable `MovementBindings`.
    let mut x = 0i32;
    let mut y = 0i32;

    if movement.any_pressed(MovementDir::Up, keyboard_input) {
        y += 1;
    }
    if movement.any_pressed(MovementDir::Down, keyboard_input) {
        y -= 1;
    }
    if movement.any_pressed(MovementDir::Right, keyboard_input) {
        x += 1;
    }
    if movement.any_pressed(MovementDir::Left, keyboard_input) {
        x -= 1;
    }
    if movement.any_pressed(MovementDir::UpRight, keyboard_input) {
        x += 1;
        y += 1;
    }
    if movement.any_pressed(MovementDir::UpLeft, keyboard_input) {
        x -= 1;
        y += 1;
    }
    if movement.any_pressed(MovementDir::DownRight, keyboard_input) {
        x += 1;
        y -= 1;
    }
    if movement.any_pressed(MovementDir::DownLeft, keyboard_input) {
        x -= 1;
        y -= 1;
    }

    let x = x.clamp(-1, 1);
    let y = y.clamp(-1, 1);

    if x == 0 && y == 0 {
        None
    } else {
        Some(MoveDelta { x, y })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::resources::ClientVitalStats;
    use crate::player::components::{EquippedItem, Inventory, PlayerIdentity};
    use crate::world::components::{SpaceId, SpacePosition};
    use crate::world::object_definitions::OverworldObjectDefinition;
    use std::collections::HashMap;

    fn equipment_def(slot: &str, armor: i32, block: i32) -> OverworldObjectDefinition {
        let yaml = format!(
            r#"
name: Test Item
description: ""
colliding: false
movable: true
storable: true
equipment_slot: {slot}
armor: {armor}
block: {block}
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
"#
        );
        serde_yaml::from_str(&yaml).expect("definition parses")
    }

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
                PlayerIdentity::new(crate::player::components::PlayerId(0)),
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

    fn spawn_test_player(app: &mut App, inventory: Inventory) -> Entity {
        let base_stats = BaseStats::default();
        let derived_stats = DerivedStats::from_base(&base_stats);
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(crate::player::components::PlayerId(0)),
                inventory,
                base_stats,
                derived_stats,
                VitalStats::full(
                    derived_stats.max_health as f32,
                    derived_stats.max_mana as f32,
                ),
                crate::combat::components::AttackProfile::melee(),
                WeaponDamage::default(),
                DefenseStats::default(),
            ))
            .id()
    }

    #[test]
    fn armor_sums_across_defensive_slots() {
        let mut definitions = HashMap::new();
        definitions.insert("test_armor".to_owned(), equipment_def("armor", 3, 0));
        definitions.insert("test_helmet".to_owned(), equipment_def("helmet", 1, 0));
        definitions.insert("test_legs".to_owned(), equipment_def("legs", 2, 0));
        definitions.insert("test_boots".to_owned(), equipment_def("boots", 1, 0));
        definitions.insert("test_shield".to_owned(), equipment_def("shield", 0, 3));

        let mut inventory = Inventory::default();
        inventory.restore_equipment_item(EquipmentSlot::Armor, EquippedItem::new("test_armor"));
        inventory.restore_equipment_item(EquipmentSlot::Helmet, EquippedItem::new("test_helmet"));
        inventory.restore_equipment_item(EquipmentSlot::Legs, EquippedItem::new("test_legs"));
        inventory.restore_equipment_item(EquipmentSlot::Boots, EquippedItem::new("test_boots"));
        inventory.restore_equipment_item(EquipmentSlot::Shield, EquippedItem::new("test_shield"));

        let mut app = App::new();
        app.insert_resource(OverworldObjectDefinitions::new_for_test(definitions));
        let entity = spawn_test_player(&mut app, inventory);
        app.add_systems(Update, refresh_derived_player_stats);
        app.update();

        let defense = app.world().entity(entity).get::<DefenseStats>().unwrap();
        assert_eq!(defense.armor, 7);
        assert_eq!(defense.block, 3);
    }

    #[test]
    fn non_defensive_slots_do_not_contribute_armor() {
        // A weapon with armor: 5 (shouldn't happen via YAML in practice) is
        // ignored — only Armor/Helmet/Legs/Boots count toward armor, and only
        // Shield counts toward block.
        let mut definitions = HashMap::new();
        definitions.insert("bad_weapon".to_owned(), equipment_def("weapon", 5, 0));
        definitions.insert("bad_ring".to_owned(), equipment_def("ring", 5, 5));

        let mut inventory = Inventory::default();
        inventory.restore_equipment_item(EquipmentSlot::Weapon, EquippedItem::new("bad_weapon"));
        inventory.restore_equipment_item(EquipmentSlot::Ring, EquippedItem::new("bad_ring"));

        let mut app = App::new();
        app.insert_resource(OverworldObjectDefinitions::new_for_test(definitions));
        let entity = spawn_test_player(&mut app, inventory);
        app.add_systems(Update, refresh_derived_player_stats);
        app.update();

        let defense = app.world().entity(entity).get::<DefenseStats>().unwrap();
        assert_eq!(defense.armor, 0);
        assert_eq!(defense.block, 0);
    }
}
