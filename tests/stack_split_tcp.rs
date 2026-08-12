//! End-to-end partial-stack moves over the TCP wire protocol.
//!
//! These cover the command combinations the cursor-carry UI can now produce.
//! `TakeFromStack` used to be reachable only as "ground pile / inventory slot
//! → *backpack slot*", because its one caller always aimed at the first empty
//! backpack slot. Carrying a stack on the cursor made "→ *world tile*"
//! reachable too, and the world-object source silently did nothing: the client
//! cleared the carry, the server dropped the command on the floor, and the
//! items never left the original pile.

mod common;

use bevy::prelude::*;
use common::{
    boot_server, player_position, pump, register_and_enter_world, server_addr, unique_test_path,
    TestClient,
};
use mud2::game::commands::{GameCommand, ItemDestination, ItemReference, ItemSlotRef};
use mud2::network::protocol::ClientMessage;
use mud2::player::components::{Inventory, Player};
use mud2::world::components::{OverworldObject, SpaceId, SpaceResident, TilePosition};

const STACKABLE: &str = "gold_coin";

/// Every ground pile of `STACKABLE` in `space`, as `(object_id, tile, quantity)`.
fn ground_piles(app: &mut App, space: SpaceId) -> Vec<(u64, TilePosition, u32)> {
    let mut objects = app.world_mut().query::<(
        &OverworldObject,
        &TilePosition,
        &SpaceResident,
        Option<&mud2::world::components::Quantity>,
    )>();
    let mut piles: Vec<_> = objects
        .iter(app.world())
        .filter(|(object, _, resident, _)| {
            resident.space_id == space && object.definition_id == STACKABLE
        })
        .map(|(object, tile, _, quantity)| (object.object_id, *tile, quantity.map_or(1, |q| q.0)))
        .collect();
    piles.sort_by_key(|(id, _, _)| *id);
    piles
}

fn first_empty_backpack_slot(app: &mut App) -> usize {
    let mut inventories = app.world_mut().query_filtered::<&Inventory, With<Player>>();
    inventories
        .single(app.world())
        .expect("player inventory")
        .backpack_slots
        .iter()
        .position(Option::is_none)
        .expect("an empty backpack slot")
}

fn backpack_slot_of(app: &mut App, type_id: &str) -> Option<(usize, u32)> {
    let mut inventories = app.world_mut().query_filtered::<&Inventory, With<Player>>();
    let inventory = inventories.single(app.world()).expect("player inventory");
    inventory
        .backpack_slots
        .iter()
        .enumerate()
        .find_map(|(index, slot)| {
            slot.as_ref()
                .filter(|stack| stack.type_id == type_id)
                .map(|stack| (index, stack.quantity))
        })
}

/// Put `count` of `STACKABLE` on the ground next to the player and return
/// `(object_id, tile, player_tile)`.
fn seed_ground_pile(
    app: &mut App,
    client: &mut TestClient,
    count: u32,
) -> (u64, TilePosition, TilePosition, SpaceId) {
    client.send(ClientMessage::Command(GameCommand::GiveItem {
        type_id: STACKABLE.to_owned(),
        count,
    }));
    pump(app, client, 20);

    let (slot, quantity) = backpack_slot_of(app, STACKABLE).expect("granted coins are in the pack");
    assert_eq!(quantity, count, "the whole grant landed in one stack");

    let (player_tile, space) = player_position(app);
    let drop_tile = TilePosition {
        x: player_tile.x + 1,
        ..player_tile
    };
    client.send(ClientMessage::Command(GameCommand::MoveItem {
        source: ItemReference::Slot(ItemSlotRef::Backpack(slot)),
        destination: ItemDestination::WorldTile(drop_tile),
    }));
    pump(app, client, 20);

    let piles = ground_piles(app, space);
    assert_eq!(piles.len(), 1, "one pile on the ground, got {piles:?}");
    assert_eq!(piles[0].2, count, "the pile holds everything dropped");
    (piles[0].0, piles[0].1, player_tile, space)
}

/// The bug: taking part of a ground pile and setting it down on another tile
/// was a silent server-side no-op.
#[test]
fn taking_part_of_a_ground_pile_onto_another_tile_splits_it() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let mut client = TestClient::connect(server_addr(&app));
    register_and_enter_world(&mut app, &mut client, "split_bot", "Splitty");

    let (pile_id, pile_tile, player_tile, space) = seed_ground_pile(&mut app, &mut client, 10);

    // Set 4 of them down on the player's other side, away from the source pile.
    let target = TilePosition {
        x: player_tile.x - 1,
        ..player_tile
    };
    client.send(ClientMessage::Command(GameCommand::TakeFromStack {
        source: ItemReference::WorldObject(pile_id),
        amount: 4,
        destination: ItemDestination::WorldTile(target),
    }));
    pump(&mut app, &mut client, 20);

    let piles = ground_piles(&mut app, space);
    assert_eq!(piles.len(), 2, "the pile should have split, got {piles:?}");

    let source = piles
        .iter()
        .find(|(id, _, _)| *id == pile_id)
        .expect("the original pile survives with the remainder");
    assert_eq!(source.2, 6, "10 - 4 left behind");
    assert_eq!(source.1, pile_tile, "the remainder does not move");

    let split = piles
        .iter()
        .find(|(id, _, _)| *id != pile_id)
        .expect("a new pile for the taken part");
    assert_eq!(split.2, 4, "exactly the amount taken");

    let total: u32 = piles.iter().map(|(_, _, q)| q).sum();
    assert_eq!(total, 10, "splitting must not create or destroy items");
}

/// The same command aimed at a backpack slot — the path that already worked,
/// kept here so a fix to one branch can't quietly break the other.
#[test]
fn taking_part_of_a_ground_pile_into_the_backpack_leaves_the_remainder() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let mut client = TestClient::connect(server_addr(&app));
    register_and_enter_world(&mut app, &mut client, "split_bot", "Splitty");

    let (pile_id, _, _, space) = seed_ground_pile(&mut app, &mut client, 10);
    assert!(
        backpack_slot_of(&mut app, STACKABLE).is_none(),
        "the pack is empty again after dropping the coins"
    );

    let slot = first_empty_backpack_slot(&mut app);
    client.send(ClientMessage::Command(GameCommand::TakeFromStack {
        source: ItemReference::WorldObject(pile_id),
        amount: 3,
        destination: ItemDestination::Slot(ItemSlotRef::Backpack(slot)),
    }));
    pump(&mut app, &mut client, 20);

    assert_eq!(
        backpack_slot_of(&mut app, STACKABLE),
        Some((slot, 3)),
        "3 coins in the pack"
    );
    let piles = ground_piles(&mut app, space);
    assert_eq!(piles.len(), 1, "still one pile, got {piles:?}");
    assert_eq!(piles[0].2, 7, "10 - 3 left on the ground");
}
