use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};
use mud2::game::commands::{GameCommand, MoveDelta};
use mud2::network::protocol::{ClientMessage, ServerMessage};
use mud2::network::resources::TcpServerState;
use mud2::player::components::Player;

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
    });
    app.update();

    let addr = server_addr(&app);
    let mut client_one = TestClient::connect(addr);
    let mut client_two = TestClient::connect(addr);

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
    assert_eq!(updated_two.player_tile_position, Some(player_two_start));
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
fn disconnecting_client_removes_its_player_from_the_server() {
    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: None,
    });
    app.update();

    let addr = server_addr(&app);
    let mut client = TestClient::connect(addr);
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
