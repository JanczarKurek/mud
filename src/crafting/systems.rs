//! Server-side systems for crafting: stash mutation, recipe learning,
//! crafting execution. All systems run in `CraftingSystemSet::Process` (a
//! subset of `game::CommandIntercept`) and are gated by `simulation_active`.

use bevy::prelude::*;

use crate::crafting::recipes::RecipeDefinitions;
use crate::crafting::stash::CharacterStash;
use crate::game::commands::GameCommand;
use crate::game::helpers::is_near_player;
use crate::game::resources::{
    ChatLogState, GameEvent, InventoryState, PendingGameCommands, PendingGameEvents,
};
use crate::player::components::{Player, PlayerId, PlayerIdentity};
use crate::player::progression::{PendingXpGrant, PendingXpGrants};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};

/// Drains `GameCommand::StashMutate` from `PendingGameCommands`, applying
/// each mutation to the acting player's `CharacterStash`. `None` values
/// delete the key. Unknown players (no matching `PlayerIdentity`) silently
/// drop the mutation.
pub fn process_stash_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut players: Query<(&PlayerIdentity, &mut CharacterStash), With<Player>>,
) {
    if pending_commands.commands.is_empty() {
        return;
    }
    let drained: Vec<_> = std::mem::take(&mut pending_commands.commands);
    let mut remaining = Vec::with_capacity(drained.len());

    for queued in drained {
        let GameCommand::StashMutate { ref key, ref value } = queued.command else {
            remaining.push(queued);
            continue;
        };

        let acting = queued
            .player_id
            .or_else(|| players.iter().next().map(|(identity, _)| identity.id));
        let Some(PlayerId(target_id)) = acting else {
            // No player to address — drop silently. Matches existing
            // unaddressed-command posture (e.g. `SetHome`).
            continue;
        };

        let mut applied = false;
        for (identity, mut stash) in players.iter_mut() {
            if identity.id.0 != target_id {
                continue;
            }
            match value {
                Some(json) => stash.set(key.clone(), json.clone()),
                None => {
                    stash.delete(key);
                }
            }
            applied = true;
            break;
        }

        if !applied {
            bevy::log::warn!(
                "StashMutate dropped: no player entity for id {target_id} (key={key})"
            );
        }
    }

    pending_commands.commands = remaining;
}

type CraftPlayerData<'a> = (
    &'a PlayerIdentity,
    &'a CharacterStash,
    &'a SpaceResident,
    &'a TilePosition,
    &'a mut InventoryState,
    &'a mut ChatLogState,
);

/// Drains `GameCommand::CraftItem`. For each:
/// 1. Validate the player knows the recipe.
/// 2. Validate they have all input quantities in their backpack.
/// 3. If the recipe declares a station, validate an adjacent matching
///    object exists in the same space.
/// 4. Consume inputs, queue `GiveItem` for each output, push narrator +
///    `ItemCrafted` event, award XP if specified.
///
/// Failure cases push a clear narrator line; nothing is consumed.
#[allow(clippy::too_many_arguments)]
pub fn process_craft_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut pending_events: ResMut<PendingGameEvents>,
    mut xp_grants: ResMut<PendingXpGrants>,
    recipe_defs: Res<RecipeDefinitions>,
    object_defs: Res<crate::world::object_definitions::OverworldObjectDefinitions>,
    world_objects: Query<(&OverworldObject, &SpaceResident, &TilePosition), Without<Player>>,
    mut players: Query<CraftPlayerData, With<Player>>,
) {
    if pending_commands.commands.is_empty() {
        return;
    }
    let drained: Vec<_> = std::mem::take(&mut pending_commands.commands);
    let mut remaining = Vec::with_capacity(drained.len());
    let mut deferred_gives: Vec<crate::game::resources::QueuedGameCommand> = Vec::new();

    'commands: for queued in drained {
        let GameCommand::CraftItem { ref recipe_id } = queued.command else {
            remaining.push(queued);
            continue;
        };

        let acting = queued.player_id.or_else(|| {
            players
                .iter()
                .next()
                .map(|(identity, _, _, _, _, _)| identity.id)
        });
        let Some(PlayerId(target_id)) = acting else {
            continue;
        };

        let Some(recipe) = recipe_defs.get(recipe_id) else {
            bevy::log::warn!("CraftItem: unknown recipe `{recipe_id}` (dropped)");
            continue;
        };

        for (identity, stash, space_resident, tile_position, mut inventory, mut chat_log) in
            players.iter_mut()
        {
            if identity.id.0 != target_id {
                continue;
            }

            if !stash.learned_recipes().contains(recipe_id) {
                chat_log
                    .lines
                    .push("You don't know that recipe.".to_owned());
                continue 'commands;
            }

            // All-or-nothing input check — gather any shortfall before
            // mutating inventory so partial consumption is impossible.
            for input in &recipe.inputs {
                let have = backpack_count(&inventory, &input.type_id);
                if have < input.count {
                    let item_name = object_defs
                        .get(&input.type_id)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| input.type_id.clone());
                    chat_log.lines.push(format!(
                        "You need {} more {}.",
                        input.count - have,
                        item_name
                    ));
                    continue 'commands;
                }
            }

            if let Some(station_type) = recipe.station.as_ref() {
                let player_space = space_resident.space_id;
                let player_tile = *tile_position;
                let station_present = world_objects.iter().any(|(obj, res, tile)| {
                    res.space_id == player_space
                        && &obj.definition_id == station_type
                        && is_near_player(&player_tile, tile)
                });
                if !station_present {
                    let station_name = object_defs
                        .get(station_type)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| station_type.clone());
                    chat_log
                        .lines
                        .push(format!("You need a {} nearby to craft this.", station_name));
                    continue 'commands;
                }
            }

            for input in &recipe.inputs {
                remove_from_backpack(&mut inventory, &input.type_id, input.count);
            }

            for output in &recipe.outputs {
                deferred_gives.push(crate::game::resources::QueuedGameCommand {
                    player_id: Some(identity.id),
                    command: GameCommand::GiveItem {
                        type_id: output.type_id.clone(),
                        count: output.count,
                    },
                });
            }

            chat_log.lines.push(format!("You craft a {}.", recipe.name));
            pending_events.events.push(GameEvent::ItemCrafted {
                recipe_id: recipe_id.clone(),
            });

            if recipe.xp_award > 0 {
                xp_grants.grants.push(PendingXpGrant {
                    player_id: identity.id,
                    amount: recipe.xp_award,
                });
            }

            break;
        }
    }

    pending_commands.commands = remaining;
    pending_commands.commands.extend(deferred_gives);
}

fn backpack_count(inventory: &InventoryState, type_id: &str) -> u32 {
    inventory
        .backpack_slots
        .iter()
        .flatten()
        .filter(|stack| stack.type_id == type_id)
        .map(|stack| stack.quantity)
        .sum()
}

fn remove_from_backpack(inventory: &mut InventoryState, type_id: &str, mut remaining: u32) {
    for slot in inventory.backpack_slots.iter_mut() {
        if remaining == 0 {
            break;
        }
        let Some(stack) = slot.as_mut() else { continue };
        if stack.type_id != type_id {
            continue;
        }
        if stack.quantity <= remaining {
            remaining -= stack.quantity;
            *slot = None;
        } else {
            stack.quantity -= remaining;
            remaining = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::commands::GameCommand;
    use crate::game::resources::PendingGameCommands;
    use crate::player::components::PlayerIdentity;

    fn build_test_app() -> App {
        let mut app = App::new();
        app.insert_resource(PendingGameCommands::default());
        app.add_systems(Update, process_stash_commands);
        app
    }

    #[test]
    fn applies_set_to_addressed_player() {
        let mut app = build_test_app();
        let entity = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(42)),
                CharacterStash::default(),
            ))
            .id();

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(42),
                GameCommand::StashMutate {
                    key: "foo".to_owned(),
                    value: Some(serde_json::json!("bar")),
                },
            );
        app.update();

        let stash = app.world().entity(entity).get::<CharacterStash>().unwrap();
        assert_eq!(stash.get("foo"), Some(&serde_json::json!("bar")));
    }

    #[test]
    fn craft_command_consumes_inputs_and_queues_outputs() {
        use crate::crafting::recipes::RecipeDefinitions;
        use crate::game::resources::{PendingGameEvents, QueuedGameCommand};
        use crate::player::components::{ChatLog, Inventory, InventoryStack};
        use crate::player::progression::PendingXpGrants;
        use crate::world::components::TilePosition;
        use crate::world::map_layout::ObjectProperties;
        use crate::world::object_definitions::OverworldObjectDefinitions;

        let mut app = App::new();
        app.insert_resource(PendingGameCommands::default());
        app.insert_resource(PendingGameEvents::default());
        app.insert_resource(PendingXpGrants::default());
        app.insert_resource(RecipeDefinitions::load_from_disk());
        app.insert_resource(OverworldObjectDefinitions::load_from_disk());
        app.add_systems(Update, process_craft_commands);

        // Player with 2 arrows in slot 0; knows `bolt_from_arrows` (no
        // station required).
        let mut stash = CharacterStash::default();
        stash.add_learned_recipe("bolt_from_arrows");
        let mut inventory = Inventory::default();
        inventory.backpack_slots[0] = Some(InventoryStack::item(
            "arrow".to_owned(),
            ObjectProperties::new(),
            2,
        ));
        let entity = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(9)),
                stash,
                inventory,
                ChatLog::default(),
                crate::world::components::SpaceResident {
                    space_id: crate::world::components::SpaceId(0),
                },
                TilePosition::ground(0, 0),
            ))
            .id();

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(9),
                GameCommand::CraftItem {
                    recipe_id: "bolt_from_arrows".to_owned(),
                },
            );
        app.update();

        // Inputs consumed.
        let inv = app.world().entity(entity).get::<Inventory>().unwrap();
        assert!(
            inv.backpack_slots[0].is_none(),
            "arrows consumed from slot 0"
        );
        // GiveItem queued for the bolt.
        let pending = app.world().resource::<PendingGameCommands>();
        let give_count = pending
            .commands
            .iter()
            .filter(|c: &&QueuedGameCommand| {
                matches!(&c.command, GameCommand::GiveItem { type_id, count } if type_id == "bolt" && *count == 1)
            })
            .count();
        assert_eq!(give_count, 1, "exactly one bolt grant queued");
        // ItemCrafted event emitted.
        let events = &app.world().resource::<PendingGameEvents>().events;
        assert!(events.iter().any(
            |e| matches!(e, GameEvent::ItemCrafted { recipe_id } if recipe_id == "bolt_from_arrows")
        ));
        // XP grant queued.
        let xp = app.world().resource::<PendingXpGrants>();
        assert_eq!(xp.grants.len(), 1);
        assert_eq!(xp.grants[0].amount, 5);
    }

    #[test]
    fn craft_command_rejects_when_inputs_missing() {
        use crate::crafting::recipes::RecipeDefinitions;
        use crate::game::resources::PendingGameEvents;
        use crate::player::components::{ChatLog, Inventory};
        use crate::player::progression::PendingXpGrants;
        use crate::world::components::TilePosition;
        use crate::world::object_definitions::OverworldObjectDefinitions;

        let mut app = App::new();
        app.insert_resource(PendingGameCommands::default());
        app.insert_resource(PendingGameEvents::default());
        app.insert_resource(PendingXpGrants::default());
        app.insert_resource(RecipeDefinitions::load_from_disk());
        app.insert_resource(OverworldObjectDefinitions::load_from_disk());
        app.add_systems(Update, process_craft_commands);

        let mut stash = CharacterStash::default();
        stash.add_learned_recipe("bolt_from_arrows");
        let entity = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(9)),
                stash,
                Inventory::default(),
                ChatLog::default(),
                crate::world::components::SpaceResident {
                    space_id: crate::world::components::SpaceId(0),
                },
                TilePosition::ground(0, 0),
            ))
            .id();

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(9),
                GameCommand::CraftItem {
                    recipe_id: "bolt_from_arrows".to_owned(),
                },
            );
        app.update();

        let inv = app.world().entity(entity).get::<Inventory>().unwrap();
        assert!(inv.backpack_slots.iter().all(|slot| slot.is_none()));
        assert!(app
            .world()
            .resource::<PendingGameCommands>()
            .commands
            .is_empty());
        assert!(app
            .world()
            .resource::<PendingGameEvents>()
            .events
            .is_empty());
        let chat = app.world().entity(entity).get::<ChatLog>().unwrap();
        assert!(chat.lines.iter().any(|line| line.contains("more Arrow")));
    }

    #[test]
    fn none_value_deletes_key() {
        let mut app = build_test_app();
        let entity = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(7)),
                CharacterStash {
                    entries: [(String::from("doomed"), serde_json::json!(1))]
                        .into_iter()
                        .collect(),
                },
            ))
            .id();

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(7),
                GameCommand::StashMutate {
                    key: "doomed".to_owned(),
                    value: None,
                },
            );
        app.update();

        let stash = app.world().entity(entity).get::<CharacterStash>().unwrap();
        assert!(!stash.has("doomed"));
    }
}
