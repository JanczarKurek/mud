//! Shared harness for the TCP end-to-end tests: a scripted wire client, a
//! headless in-process server app pumped from the test thread, and the full
//! auth → character → asset-sync handshake.
//!
//! Every e2e test file uses this via `mod common;` — keep drifted copies out
//! of the individual test files (the in-crate unit tests learned the same
//! lesson with `mud2::test_support`).
//!
//! `TestClient` behaves like a real client: every `Events` message is folded
//! into a cumulative [`ClientGameState`] (`client.state`), UI events buffer in
//! `client.ui_events`, and control messages (auth/character/asset/ping) queue
//! until a `wait_for` consumes them — so a message is never lost just because
//! it arrived in the same read batch as the one a caller was waiting for.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};
use mud2::game::commands::{GameCommand, MoveDelta};
use mud2::game::projection::apply_event_to_state;
use mud2::game::resources::{ClientGameState, GameUiEvent};
use mud2::network::protocol::{ClientMessage, ServerMessage};
use mud2::network::resources::TcpServerState;
use mud2::player::classes::Class;
use mud2::player::components::{AttributeSet, Player, PlayerAppearance};
use mud2::world::components::{SpaceId, SpaceResident, TilePosition};

static NEXT_PATH_ID: AtomicU64 = AtomicU64::new(0);

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// A unique per-test temp path; `suffix` names the artifact ("world.json",
/// "accounts.db"). Uniqueness comes from pid + a process-wide counter, so
/// tests in one binary and across binaries never collide.
pub fn unique_test_path(suffix: &str) -> PathBuf {
    let id = NEXT_PATH_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("mud2-e2e-{}-{}-{suffix}", std::process::id(), id))
}

/// Boot a `HeadlessServer` app on an OS-assigned loopback port with isolated
/// save/db paths. One `update()` has already run (listener bound).
pub fn boot_server(save: PathBuf, db: PathBuf) -> App {
    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        debug: false,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: Some(save),
        db_path: Some(db),
        asset_cache_dir: None,
        server_tls: None,
        client_tls: None,
        admin_socket: None,
        embedded_extension: None,
        autopilot: None,
    });
    app.update();
    app
}

pub fn server_addr(app: &App) -> std::net::SocketAddr {
    app.world()
        .resource::<TcpServerState>()
        .listener
        .as_ref()
        .unwrap()
        .local_addr()
        .unwrap()
}

/// A scripted wire client: raw `TcpStream`, newline-delimited JSON.
pub struct TestClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    /// Cumulative fold of every `Events` message received — the client's
    /// replicated view of the world, exactly as a real client would hold it.
    pub state: ClientGameState,
    /// UI events received and not yet consumed by `collect_ui_events`.
    pub ui_events: Vec<GameUiEvent>,
    /// Control messages not yet consumed by a `wait_for`.
    control: VecDeque<ServerMessage>,
}

impl TestClient {
    pub fn connect(addr: std::net::SocketAddr) -> Self {
        let writer = TcpStream::connect(addr).unwrap();
        writer
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        writer
            .set_write_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let reader = BufReader::new(writer.try_clone().unwrap());
        Self {
            writer,
            reader,
            state: ClientGameState::default(),
            ui_events: Vec::new(),
            control: VecDeque::new(),
        }
    }

    pub fn send(&mut self, message: ClientMessage) {
        let mut payload = serde_json::to_vec(&message).unwrap();
        payload.push(b'\n');
        self.writer.write_all(&payload).unwrap();
        self.writer.flush().unwrap();
    }

    /// Drain everything currently readable without blocking: fold `Events`
    /// into `state`, buffer `UiEvents`, queue the rest for `wait_for`.
    pub fn poll(&mut self) {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let message: ServerMessage = serde_json::from_str(trimmed).unwrap();
                    match message {
                        ServerMessage::Events(events) => {
                            for event in events {
                                apply_event_to_state(&mut self.state, event);
                            }
                        }
                        ServerMessage::UiEvents(events) => self.ui_events.extend(events),
                        other => self.control.push_back(other),
                    }
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
    }

    /// Pump the server until `pick` claims a queued control message or the
    /// timeout elapses. Messages `pick` declines stay queued for later waits.
    pub fn wait_for<T>(
        &mut self,
        app: &mut App,
        what: &str,
        mut pick: impl FnMut(&ServerMessage) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            let mut index = 0;
            while index < self.control.len() {
                if let Some(value) = pick(&self.control[index]) {
                    self.control.remove(index);
                    return value;
                }
                index += 1;
            }
            if Instant::now() >= deadline {
                panic!(
                    "timed out waiting for {what}; queued control: {:?}",
                    self.control
                );
            }
            app.update();
            thread::sleep(Duration::from_millis(5));
            self.poll();
        }
    }
}

/// Pump the server for `ticks` frames, folding the client's inbound traffic.
pub fn pump(app: &mut App, client: &mut TestClient, ticks: usize) {
    for _ in 0..ticks {
        app.update();
        thread::sleep(Duration::from_millis(5));
        client.poll();
    }
}

/// Pump the server for `ticks` frames with no client to drain.
pub fn pump_server(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
        thread::sleep(Duration::from_millis(5));
    }
}

/// Pump the server for `ticks` frames, returning every UI event received
/// *during* those ticks (previously buffered UI events are discarded first).
pub fn collect_ui_events(app: &mut App, client: &mut TestClient, ticks: usize) -> Vec<GameUiEvent> {
    client.ui_events.clear();
    pump(app, client, ticks);
    std::mem::take(&mut client.ui_events)
}

/// An `AttributeSet` that spends exactly the creation point budget
/// (6 attributes at 10 + 2 each = the 12-point budget).
pub fn budget_attributes() -> AttributeSet {
    AttributeSet {
        strength: 12,
        agility: 12,
        constitution: 12,
        willpower: 12,
        charisma: 12,
        focus: 12,
    }
}

/// `Register` or `Login` and wait for a successful `AuthResult`.
pub fn auth(
    app: &mut App,
    client: &mut TestClient,
    username: &str,
    password: &str,
    register: bool,
) {
    if register {
        client.send(ClientMessage::Register {
            username: username.to_owned(),
            password: password.to_owned(),
        });
    } else {
        client.send(ClientMessage::Login {
            username: username.to_owned(),
            password: password.to_owned(),
        });
    }
    client.wait_for(app, "AuthResult", |m| match m {
        ServerMessage::AuthResult { ok, reason } => {
            assert!(*ok, "auth rejected: {reason:?}");
            Some(())
        }
        _ => None,
    });
}

/// Create a character on the authed account and return its id.
pub fn create_character(app: &mut App, client: &mut TestClient, name: &str) -> i64 {
    client.send(ClientMessage::CreateCharacter {
        name: name.to_owned(),
        class: Class::Fighter,
        attributes: budget_attributes(),
        appearance: PlayerAppearance::default(),
    });
    client.wait_for(app, "CharacterCreateResult", |m| match m {
        ServerMessage::CharacterCreateResult {
            ok,
            character_id,
            reason,
        } => {
            assert!(*ok, "character create rejected: {reason:?}");
            Some(character_id.unwrap())
        }
        _ => None,
    })
}

/// Select the character and complete the asset-sync handshake (no assets are
/// fetched — tests hash-match the repo's bundled assets), then let the initial
/// full-replay event stream fold into `client.state`.
pub fn enter_world(app: &mut App, client: &mut TestClient, character_id: i64) {
    client.send(ClientMessage::SelectCharacter {
        character_id,
        start_map: None,
    });
    client.wait_for(app, "CharacterSelected", |m| match m {
        ServerMessage::CharacterSelected { .. } => Some(()),
        _ => None,
    });
    client.wait_for(app, "AssetManifest", |m| match m {
        ServerMessage::AssetManifest(_) => Some(()),
        _ => None,
    });
    client.send(ClientMessage::SyncComplete);
    pump(app, client, 10);
}

/// The whole front door in one call: register a fresh account, create a
/// character, enter the world. Returns the character id (for reconnects).
pub fn register_and_enter_world(
    app: &mut App,
    client: &mut TestClient,
    username: &str,
    character_name: &str,
) -> i64 {
    auth(app, client, username, "secret123", true);
    let character_id = create_character(app, client, character_name);
    enter_world(app, client, character_id);
    character_id
}

/// Log back in to an existing account and re-enter with a known character.
pub fn login_and_enter_world(
    app: &mut App,
    client: &mut TestClient,
    username: &str,
    character_id: i64,
) {
    auth(app, client, username, "secret123", false);
    enter_world(app, client, character_id);
}

/// Authoritative position of the single connected player (server-side peek).
pub fn player_position(app: &mut App) -> (TilePosition, SpaceId) {
    let mut players = app
        .world_mut()
        .query_filtered::<(&TilePosition, &SpaceResident), With<Player>>();
    let (tile, resident) = players
        .single(app.world())
        .expect("exactly one connected player");
    (*tile, resident.space_id)
}

/// Drive the player one tile via a real wire command, resending while the
/// movement cooldown swallows attempts. Panics if the step never lands.
pub fn step(app: &mut App, client: &mut TestClient, dx: i32, dy: i32) {
    let (from, from_space) = player_position(app);
    let deadline = Instant::now() + WAIT_TIMEOUT;
    while Instant::now() < deadline {
        client.send(ClientMessage::Command(GameCommand::MovePlayer {
            delta: MoveDelta { x: dx, y: dy },
            climb: false,
        }));
        pump(app, client, 4);
        let (now, now_space) = player_position(app);
        if now_space != from_space {
            // Stepping onto a portal teleports; the caller asserts the result.
            return;
        }
        if now.x != from.x || now.y != from.y {
            assert_eq!(
                (now.x, now.y),
                (from.x + dx, from.y + dy),
                "step landed on an unexpected tile"
            );
            return;
        }
    }
    panic!("step ({dx},{dy}) from ({},{}) never landed", from.x, from.y);
}

/// Walk an axis-aligned path expressed as (dx, dy, count) runs.
pub fn walk(app: &mut App, client: &mut TestClient, runs: &[(i32, i32, usize)]) {
    for &(dx, dy, count) in runs {
        for _ in 0..count {
            step(app, client, dx, dy);
        }
    }
}

/// Pump until the client's cumulative replicated state satisfies `predicate`,
/// then return a clone of it.
pub fn wait_for_snapshot<F>(app: &mut App, client: &mut TestClient, predicate: F) -> ClientGameState
where
    F: Fn(&ClientGameState) -> bool,
{
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        client.poll();
        if predicate(&client.state) {
            return client.state.clone();
        }
        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for matching snapshot; latest={:?}",
                client.state
            );
        }
        pump(app, client, 2);
    }
}
