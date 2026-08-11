//! End-to-end ground-item pickup in an *ephemeral* space over the TCP wire
//! protocol. Reproduces the playtest flow that broke in the proving grounds:
//! a scripted client authenticates, walks through the overworld portal (which
//! lazily instantiates the dungeon), walks to the potion, and picks it up —
//! then leaves, re-enters (fresh instance, same authored object ids), and
//! repeats; finally disconnects inside the instance and reconnects, which must
//! not resume the character into a dead ephemeral space id.

mod common;

use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use common::{
    boot_server, login_and_enter_world, player_position, pump, register_and_enter_world,
    server_addr, step, unique_test_path, walk, TestClient,
};
use mud2::game::commands::{GameCommand, ItemDestination, ItemReference, ItemSlotRef};
use mud2::network::protocol::ClientMessage;
use mud2::player::components::{Inventory, Player};
use mud2::world::components::{OverworldObject, SpaceId, SpaceResident, TilePosition};
use mud2::world::resources::SpaceManager;

fn find_object_in_space(
    app: &mut App,
    space: SpaceId,
    definition_id: &str,
) -> Option<(u64, TilePosition)> {
    let mut objects = app
        .world_mut()
        .query::<(&OverworldObject, &TilePosition, &SpaceResident)>();
    objects
        .iter(app.world())
        .find(|(object, _, resident)| {
            resident.space_id == space && object.definition_id == definition_id
        })
        .map(|(object, tile, _)| (object.object_id, *tile))
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

fn backpack_contains(app: &mut App, type_id: &str) -> bool {
    let mut inventories = app.world_mut().query_filtered::<&Inventory, With<Player>>();
    inventories
        .single(app.world())
        .expect("player inventory")
        .backpack_slots
        .iter()
        .flatten()
        .any(|stack| stack.type_id == type_id)
}

/// Walk from the proving-grounds arrival tile (14,12) into the fence pen and
/// onto the potion at (17,2). The fence spans x=14..=20 at y=3; the pen is
/// entered by walking around its west end via (13,3).
fn walk_arrival_to_potion(app: &mut App, client: &mut TestClient) {
    walk(
        app,
        client,
        &[
            (0, -1, 8), // (14,12) -> (14,4)
            (-1, 0, 1), // -> (13,4)
            (0, -1, 2), // -> (13,2)
            (1, 0, 4),  // -> (17,2), the potion tile
        ],
    );
}

/// Teleport the player (server-side setup only) to the tile just west of the
/// overworld proving-grounds arch, then walk through the portal for real.
fn enter_proving_grounds(app: &mut App, client: &mut TestClient) -> SpaceId {
    let overworld = {
        let world = app.world();
        let manager = world.resource::<SpaceManager>();
        manager
            .persistent_space_id("overworld")
            .expect("overworld space")
    };
    {
        let mut players = app
            .world_mut()
            .query_filtered::<(&mut TilePosition, &mut SpaceResident), With<Player>>();
        let (mut tile, mut resident) = players
            .single_mut(app.world_mut())
            .expect("exactly one connected player");
        *tile = TilePosition {
            x: 118,
            y: 65,
            z: 0,
        };
        resident.space_id = overworld;
    }
    pump(app, client, 5);

    // Real portal traversal: one step east onto the arch at (119,65).
    step(app, client, 1, 0);
    let (tile, space) = player_position(app);
    assert_ne!(space, overworld, "portal should have moved the player");
    assert_eq!((tile.x, tile.y), (14, 12), "proving grounds arrival tile");
    space
}

fn pick_up_potion(app: &mut App, client: &mut TestClient, space: SpaceId) {
    let (potion_id, potion_tile) =
        find_object_in_space(app, space, "potion").expect("potion in proving grounds");
    assert_eq!(
        (potion_tile.x, potion_tile.y),
        (17, 2),
        "authored placement"
    );

    walk_arrival_to_potion(app, client);
    let (tile, _) = player_position(app);
    assert_eq!((tile.x, tile.y), (17, 2), "standing on the potion tile");

    let slot = first_empty_backpack_slot(app);
    client.send(ClientMessage::Command(GameCommand::MoveItem {
        source: ItemReference::WorldObject(potion_id),
        destination: ItemDestination::Slot(ItemSlotRef::Backpack(slot)),
    }));
    pump(app, client, 20);

    assert!(
        backpack_contains(app, "potion"),
        "potion should be in the backpack after an adjacent pickup"
    );
    assert!(
        find_object_in_space(app, space, "potion").is_none(),
        "picked-up potion should leave the world"
    );
}

/// Fresh walk-in: portal entry instantiates the dungeon, adjacent pickup works.
#[test]
fn proving_grounds_pickup_first_visit() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let mut client = TestClient::connect(server_addr(&app));
    register_and_enter_world(&mut app, &mut client, "pickup_bot", "Grabby");

    let space = enter_proving_grounds(&mut app, &mut client);
    pick_up_potion(&mut app, &mut client, space);
}

/// Leave and re-enter: the second instance reuses the same authored object
/// ids in a fresh space id; pickup must still work.
#[test]
fn proving_grounds_pickup_after_reentry() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let mut client = TestClient::connect(server_addr(&app));
    register_and_enter_world(&mut app, &mut client, "pickup_bot", "Grabby");

    let first_space = enter_proving_grounds(&mut app, &mut client);
    // Step off the arrival arch and back onto it to ride pg_exit out.
    step(&mut app, &mut client, 1, 0);
    step(&mut app, &mut client, -1, 0);
    let (tile, space) = player_position(&mut app);
    assert_ne!(space, first_space, "pg_exit should return to the overworld");
    assert_eq!((tile.x, tile.y), (118, 65), "pg_exit landing tile");
    // Give cleanup a chance to tear the empty instance down.
    pump(&mut app, &mut client, 10);

    let second_space = enter_proving_grounds(&mut app, &mut client);
    assert_ne!(
        second_space, first_space,
        "ephemeral space ids are never reused within a session"
    );
    pick_up_potion(&mut app, &mut client, second_space);
}

/// Disconnect inside the instance, reconnect: the character must not resume
/// into the (now dead) ephemeral space id.
#[test]
fn reconnect_inside_ephemeral_space_resumes_in_live_space() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let mut client = TestClient::connect(server_addr(&app));
    let character_id = register_and_enter_world(&mut app, &mut client, "pickup_bot", "Grabby");

    enter_proving_grounds(&mut app, &mut client);
    walk_arrival_to_potion(&mut app, &mut client);

    // Hard-drop the connection with the player standing in the dungeon; pump
    // so the server notices, saves the character, and tears the instance down.
    drop(client);
    for _ in 0..30 {
        app.update();
        thread::sleep(Duration::from_millis(5));
    }

    let mut client = TestClient::connect(server_addr(&app));
    login_and_enter_world(&mut app, &mut client, "pickup_bot", character_id);

    let (_, space) = player_position(&mut app);
    let live = app.world().resource::<SpaceManager>().get(space).is_some();
    assert!(
        live,
        "reconnected character resumed into dead space {space:?} — \
         saved ephemeral space ids must fall back to a live space on login"
    );

    // And the world must still be fully usable: enter the dungeon and pick
    // the potion up again.
    let space = enter_proving_grounds(&mut app, &mut client);
    pick_up_potion(&mut app, &mut client, space);
}
