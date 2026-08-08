//! Multi-peer wire-protocol coverage against a real `HeadlessServer` app:
//! two clients seeing each other move, character position surviving a
//! reconnect, wrong-password rejection, and disconnect cleanup.

mod common;

use bevy::prelude::*;
use common::{
    boot_server, pump_server, register_and_enter_world, server_addr, unique_test_path,
    wait_for_snapshot, TestClient,
};
use mud2::game::commands::{GameCommand, MoveDelta};
use mud2::network::protocol::{ClientMessage, ServerMessage};
use mud2::network::resources::TcpServerState;
use mud2::player::components::Player;

#[test]
fn two_clients_receive_snapshots_and_see_each_other_move() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let addr = server_addr(&app);

    let mut client_one = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut client_one, "test_one", "One");
    let mut client_two = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut client_two, "test_two", "Two");

    let snapshot_one = wait_for_snapshot(&mut app, &mut client_one, |snapshot| {
        snapshot.local_player_id.is_some() && snapshot.player_tile_position.is_some()
    });
    let player_one_id = snapshot_one.local_player_id.unwrap();
    let player_one_start = snapshot_one.player_tile_position.unwrap();

    let snapshot_two = wait_for_snapshot(&mut app, &mut client_two, |snapshot| {
        snapshot.remote_players.contains_key(&player_one_id)
            && snapshot.player_tile_position.is_some()
    });
    let player_two_id = snapshot_two.local_player_id.unwrap();
    assert_ne!(player_one_id, player_two_id);

    client_one.send(ClientMessage::Command(GameCommand::MovePlayer {
        delta: MoveDelta { x: 1, y: 0 },
        climb: false,
    }));

    let updated_one = wait_for_snapshot(&mut app, &mut client_one, |snapshot| {
        snapshot.player_tile_position != Some(player_one_start)
    });
    let updated_two = wait_for_snapshot(&mut app, &mut client_two, |snapshot| {
        snapshot
            .remote_players
            .get(&player_one_id)
            .is_some_and(|remote| remote.tile_position == updated_one.player_tile_position.unwrap())
    });

    assert_ne!(updated_one.player_tile_position, Some(player_one_start));
    assert_eq!(
        updated_two
            .remote_players
            .get(&player_one_id)
            .unwrap()
            .tile_position,
        updated_one.player_tile_position.unwrap()
    );
}

#[test]
fn reconnecting_same_account_restores_character_position() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let addr = server_addr(&app);

    // First session: register, move, disconnect.
    let mut client = TestClient::connect(addr);
    let character_id = register_and_enter_world(&mut app, &mut client, "persistbot", "Persist");
    let initial = wait_for_snapshot(&mut app, &mut client, |snapshot| {
        snapshot.player_tile_position.is_some()
    });
    let starting_tile = initial.player_tile_position.unwrap();

    client.send(ClientMessage::Command(GameCommand::MovePlayer {
        delta: MoveDelta { x: 1, y: 0 },
        climb: false,
    }));
    let moved = wait_for_snapshot(&mut app, &mut client, |snapshot| {
        snapshot.player_tile_position != Some(starting_tile)
    });
    let moved_tile = moved.player_tile_position.unwrap();
    assert_ne!(moved_tile, starting_tile);

    drop(client);
    pump_server(&mut app, 10);

    // Second session: login, select the same character, verify the spawn tile.
    let mut client = TestClient::connect(addr);
    common::login_and_enter_world(&mut app, &mut client, "persistbot", character_id);

    let restored = wait_for_snapshot(&mut app, &mut client, |snapshot| {
        snapshot.player_tile_position.is_some()
    });
    assert_eq!(
        restored.player_tile_position,
        Some(moved_tile),
        "character should reappear at last saved tile after reconnect"
    );
}

#[test]
fn login_with_wrong_password_is_rejected() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let addr = server_addr(&app);

    // Register the account (auth alone is enough — no character needed).
    let mut client = TestClient::connect(addr);
    common::auth(&mut app, &mut client, "wrongpw_test", "correct123", true);
    drop(client);
    pump_server(&mut app, 5);

    // Try to log in with the wrong password.
    let mut client = TestClient::connect(addr);
    client.send(ClientMessage::Login {
        username: "wrongpw_test".to_owned(),
        password: "nope_nope_nope".to_owned(),
    });
    let (ok, reason) = client.wait_for(&mut app, "AuthResult", |m| match m {
        ServerMessage::AuthResult { ok, reason } => Some((*ok, reason.clone())),
        _ => None,
    });
    assert!(!ok, "wrong password must be rejected");
    assert!(
        reason.is_some_and(|r| r.contains("wrong") || r.contains("password")),
        "reason should mention the rejection cause"
    );
}

#[test]
fn disconnecting_client_removes_its_player_from_the_server() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let addr = server_addr(&app);

    let mut client = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut client, "disc_tester", "Blinky");
    let _ = wait_for_snapshot(&mut app, &mut client, |snapshot| {
        snapshot.local_player_id.is_some()
    });

    assert_eq!(app.world().resource::<TcpServerState>().peers.len(), 1);
    let player_count_before = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .iter(app.world())
        .count();
    assert_eq!(player_count_before, 1);

    drop(client);
    pump_server(&mut app, 8);

    assert!(app.world().resource::<TcpServerState>().peers.is_empty());
    let player_count_after = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .iter(app.world())
        .count();
    assert_eq!(player_count_after, 0);
}
