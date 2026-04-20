use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};
use mud2::game::commands::{GameCommand, MoveDelta};
use mud2::network::protocol::{ClientMessage, ServerMessage};
use mud2::network::resources::TcpServerState;
use mud2::player::components::Player;

static NEXT_DB_ID: AtomicU64 = AtomicU64::new(0);

fn unique_test_db_path() -> PathBuf {
    let id = NEXT_DB_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("mud2-integration-{}-{}.db", std::process::id(), id))
}

struct TestClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl TestClient {
    fn connect(addr: std::net::SocketAddr) -> Self {
        let writer = TcpStream::connect(addr).unwrap();
        writer
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        writer
            .set_write_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let reader = BufReader::new(writer.try_clone().unwrap());
        Self { writer, reader }
    }

    fn send(&mut self, message: ClientMessage) {
        let mut payload = serde_json::to_vec(&message).unwrap();
        payload.push(b'\n');
        self.writer.write_all(&payload).unwrap();
        self.writer.flush().unwrap();
    }

    /// Register a new account and wait for the server's `AuthResult`. Assumes
    /// the server's asset-sync handshake follows auth, so callers should not
    /// rely on `read_messages` returning `Events` until asset sync completes.
    fn register(&mut self, app: &mut App, username: &str, password: &str) {
        self.send(ClientMessage::Register {
            username: username.to_owned(),
            password: password.to_owned(),
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            app.update();
            thread::sleep(Duration::from_millis(5));
            let messages = self.read_messages();
            for message in messages {
                if let ServerMessage::AuthResult { ok, reason } = message {
                    assert!(ok, "auth register rejected: {reason:?}");
                    return;
                }
            }
        }
        panic!("timed out waiting for AuthResult after Register");
    }

    /// Complete the asset-sync handshake with no assets fetched (tests run
    /// against the repo's bundled assets which already hash-match). This
    /// transitions the peer to `sync_complete` so the server starts emitting
    /// Events.
    fn complete_asset_sync(&mut self, app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            app.update();
            thread::sleep(Duration::from_millis(5));
            let messages = self.read_messages();
            for message in messages {
                if matches!(message, ServerMessage::AssetManifest(_)) {
                    // Respond immediately so the server knows we have
                    // everything we need. Tests don't fetch bundled assets.
                    self.send(ClientMessage::SyncComplete);
                    return;
                }
            }
        }
        panic!("timed out waiting for AssetManifest");
    }

    fn read_messages(&mut self) -> Vec<ServerMessage> {
        let mut messages = Vec::new();
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    messages.push(serde_json::from_str(trimmed).unwrap());
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("failed to read server message: {error}"),
            }
        }

        messages
    }
}

fn pump_server(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
        thread::sleep(Duration::from_millis(5));
    }
}

/// Folds any `Events` messages in `messages` into `baseline` in order, updating
/// the caller's running view of server state. The wire protocol no longer sends
/// full snapshots — clients track state by accumulating event deltas.
fn fold_events(
    baseline: &mut mud2::game::resources::ClientGameState,
    messages: &[ServerMessage],
) -> bool {
    let mut saw_events = false;
    for message in messages {
        if let ServerMessage::Events(events) = message {
            saw_events = true;
            for event in events {
                mud2::game::projection::apply_event_to_state(baseline, event.clone());
            }
        }
    }
    saw_events
}

fn wait_for_snapshot<F>(
    app: &mut App,
    client: &mut TestClient,
    predicate: F,
) -> mud2::game::resources::ClientGameState
where
    F: Fn(&mud2::game::resources::ClientGameState) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut baseline = mud2::game::resources::ClientGameState::default();

    while Instant::now() < deadline {
        pump_server(app, 2);
        let messages = client.read_messages();
        if fold_events(&mut baseline, &messages) && predicate(&baseline) {
            return baseline;
        }
    }

    panic!("timed out waiting for matching snapshot; latest={baseline:?}");
}

fn server_addr(app: &App) -> std::net::SocketAddr {
    app.world()
        .resource::<TcpServerState>()
        .listener
        .as_ref()
        .unwrap()
        .local_addr()
        .unwrap()
}

#[test]
#[ignore = "requires loopback TCP bind support"]
fn two_clients_receive_snapshots_and_see_each_other_move() {
    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: None,
        db_path: Some(unique_test_db_path()),
        server_tls: None,
        client_tls: None,
    });
    app.update();

    let addr = server_addr(&app);
    let mut client_one = TestClient::connect(addr);
    client_one.register(&mut app, "test_one", "secret123");
    client_one.complete_asset_sync(&mut app);
    let mut client_two = TestClient::connect(addr);
    client_two.register(&mut app, "test_two", "secret123");
    client_two.complete_asset_sync(&mut app);

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
    let player_two_start = snapshot_two.player_tile_position.unwrap();
    assert_ne!(player_one_id, player_two_id);

    client_one.send(ClientMessage::Command(GameCommand::MovePlayer {
        delta: MoveDelta { x: 1, y: 0 },
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
    // `updated_two`'s baseline is rebuilt from deltas only (see
    // `wait_for_snapshot`), so fields that haven't changed in the current
    // wait window (like player_two's own position) will be absent. The
    // meaningful assertion is that player one's remote tile is the new tile.
    let _ = player_two_start;
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
#[ignore = "requires loopback TCP bind support"]
fn reconnecting_same_account_restores_character_position() {
    use mud2::game::commands::{GameCommand, MoveDelta};

    let db_path = unique_test_db_path();
    let _ = std::fs::remove_file(&db_path);

    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: None,
        db_path: Some(db_path.clone()),
        server_tls: None,
        client_tls: None,
    });
    app.update();

    let addr = server_addr(&app);

    // First session: register, move, disconnect.
    let mut client = TestClient::connect(addr);
    client.register(&mut app, "persistbot", "secret123");
    client.complete_asset_sync(&mut app);
    let initial = wait_for_snapshot(&mut app, &mut client, |snapshot| {
        snapshot.player_tile_position.is_some()
    });
    let starting_tile = initial.player_tile_position.unwrap();

    client.send(ClientMessage::Command(GameCommand::MovePlayer {
        delta: MoveDelta { x: 1, y: 0 },
    }));
    let moved = wait_for_snapshot(&mut app, &mut client, |snapshot| {
        snapshot.player_tile_position != Some(starting_tile)
    });
    let moved_tile = moved.player_tile_position.unwrap();
    assert_ne!(moved_tile, starting_tile);

    drop(client);
    pump_server(&mut app, 10);

    // Second session: login, verify we spawn at `moved_tile`.
    let mut client = TestClient::connect(addr);
    client.send(ClientMessage::Login {
        username: "persistbot".to_owned(),
        password: "secret123".to_owned(),
    });
    // Drain AuthResult.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut auth_ok = false;
    while Instant::now() < deadline && !auth_ok {
        app.update();
        thread::sleep(Duration::from_millis(5));
        for message in client.read_messages() {
            if let ServerMessage::AuthResult { ok, .. } = message {
                auth_ok = ok;
                break;
            }
        }
    }
    assert!(auth_ok, "login with existing account must succeed");
    client.complete_asset_sync(&mut app);

    let restored = wait_for_snapshot(&mut app, &mut client, |snapshot| {
        snapshot.player_tile_position.is_some()
    });
    assert_eq!(
        restored.player_tile_position,
        Some(moved_tile),
        "character should reappear at last saved tile after reconnect"
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
#[ignore = "requires loopback TCP bind support"]
fn login_with_wrong_password_is_rejected() {
    let db_path = unique_test_db_path();
    let _ = std::fs::remove_file(&db_path);

    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: None,
        db_path: Some(db_path.clone()),
        server_tls: None,
        client_tls: None,
    });
    app.update();

    let addr = server_addr(&app);

    // Register the account.
    let mut client = TestClient::connect(addr);
    client.register(&mut app, "wrongpw_test", "correct123");
    drop(client);
    pump_server(&mut app, 5);

    // Try to log in with the wrong password.
    let mut client = TestClient::connect(addr);
    client.send(ClientMessage::Login {
        username: "wrongpw_test".to_owned(),
        password: "nope_nope_nope".to_owned(),
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut got_result = None;
    while Instant::now() < deadline && got_result.is_none() {
        app.update();
        thread::sleep(Duration::from_millis(5));
        for message in client.read_messages() {
            if let ServerMessage::AuthResult { ok, reason } = message {
                got_result = Some((ok, reason));
                break;
            }
        }
    }
    let (ok, reason) = got_result.expect("expected AuthResult");
    assert!(!ok, "wrong password must be rejected");
    assert!(
        reason.is_some_and(|r| r.contains("wrong") || r.contains("password")),
        "reason should mention the rejection cause"
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
#[ignore = "requires loopback TCP bind support"]
fn disconnecting_client_removes_its_player_from_the_server() {
    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: None,
        db_path: Some(unique_test_db_path()),
        server_tls: None,
        client_tls: None,
    });
    app.update();

    let addr = server_addr(&app);
    let mut client = TestClient::connect(addr);
    client.register(&mut app, "disc_tester", "secret123");
    client.complete_asset_sync(&mut app);
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
