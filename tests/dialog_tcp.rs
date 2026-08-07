//! End-to-end NPC dialogue over the TCP wire protocol: a scripted client
//! authenticates, creates + selects a character, is teleported next to a
//! dialog NPC, and exercises Talk → line → advance → end against a real
//! `HeadlessServer` app.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};
use mud2::dialog::components::DialogNode;
use mud2::game::commands::GameCommand;
use mud2::game::resources::GameUiEvent;
use mud2::network::protocol::{ClientMessage, ServerMessage};
use mud2::network::resources::TcpServerState;
use mud2::player::classes::Class;
use mud2::player::components::{AttributeSet, Player, PlayerAppearance};
use mud2::world::components::{OverworldObject, SpaceResident, TilePosition};

static NEXT_DB_ID: AtomicU64 = AtomicU64::new(0);

fn unique_test_path(suffix: &str) -> PathBuf {
    let id = NEXT_DB_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "mud2-dialog-{}-{}-{suffix}",
        std::process::id(),
        id
    ))
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

    /// Pump the server and collect messages until `pick` returns Some or the
    /// timeout elapses.
    fn wait_for<T>(
        &mut self,
        app: &mut App,
        what: &str,
        mut pick: impl FnMut(&ServerMessage) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            app.update();
            thread::sleep(Duration::from_millis(5));
            for message in self.read_messages() {
                if let Some(value) = pick(&message) {
                    return value;
                }
            }
        }
        panic!("timed out waiting for {what}");
    }
}

/// Pump the server for `ticks` frames, returning every UI event received.
fn collect_ui_events(app: &mut App, client: &mut TestClient, ticks: usize) -> Vec<GameUiEvent> {
    let mut collected = Vec::new();
    for _ in 0..ticks {
        app.update();
        thread::sleep(Duration::from_millis(5));
        for message in client.read_messages() {
            if let ServerMessage::UiEvents(events) = message {
                collected.extend(events);
            }
        }
    }
    collected
}

#[test]
fn talking_to_npc_over_tcp_round_trips() {
    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        debug: false,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: Some(unique_test_path("world.json")),
        db_path: Some(unique_test_path("accounts.db")),
        asset_cache_dir: None,
        server_tls: None,
        client_tls: None,
        admin_socket: None,
        embedded_extension: None,
    });
    app.update();

    let addr = app
        .world()
        .resource::<TcpServerState>()
        .listener
        .as_ref()
        .unwrap()
        .local_addr()
        .unwrap();
    let mut client = TestClient::connect(addr);

    // Auth + character flow.
    client.send(ClientMessage::Register {
        username: "dialog_bot".to_owned(),
        password: "secret123".to_owned(),
    });
    client.wait_for(&mut app, "AuthResult", |m| match m {
        ServerMessage::AuthResult { ok, reason } => {
            assert!(*ok, "register rejected: {reason:?}");
            Some(())
        }
        _ => None,
    });
    client.send(ClientMessage::CreateCharacter {
        name: "Talky".to_owned(),
        class: Class::Fighter,
        // 6 attributes at 10 + 2 each spends exactly the 12-point budget.
        attributes: AttributeSet {
            strength: 12,
            agility: 12,
            constitution: 12,
            willpower: 12,
            charisma: 12,
            focus: 12,
        },
        appearance: PlayerAppearance::default(),
    });
    let character_id = client.wait_for(&mut app, "CharacterCreateResult", |m| match m {
        ServerMessage::CharacterCreateResult {
            ok,
            character_id,
            reason,
        } => {
            assert!(*ok, "character create rejected: {reason:?}");
            Some(character_id.unwrap())
        }
        _ => None,
    });
    client.send(ClientMessage::SelectCharacter { character_id });
    client.wait_for(&mut app, "CharacterSelected", |m| match m {
        ServerMessage::CharacterSelected { .. } => Some(()),
        _ => None,
    });
    client.wait_for(&mut app, "AssetManifest", |m| match m {
        ServerMessage::AssetManifest(_) => Some(()),
        _ => None,
    });
    client.send(ClientMessage::SyncComplete);

    // Let the initial full-replay event stream flow.
    for _ in 0..10 {
        app.update();
        thread::sleep(Duration::from_millis(5));
        client.read_messages();
    }

    // Server-side: find a dialog NPC and teleport the player next to it.
    let (npc_object_id, npc_tile, npc_space) = {
        let mut npcs = app
            .world_mut()
            .query_filtered::<(&OverworldObject, &TilePosition, &SpaceResident), With<DialogNode>>(
            );
        let (object, tile, resident) = npcs
            .iter(app.world())
            .next()
            .expect("world should contain at least one NPC with a DialogNode");
        (object.object_id, *tile, resident.space_id)
    };
    {
        let mut players = app
            .world_mut()
            .query_filtered::<(&mut TilePosition, &mut SpaceResident), With<Player>>();
        let (mut tile, mut resident) = players
            .single_mut(app.world_mut())
            .expect("exactly one connected player");
        *tile = TilePosition {
            x: npc_tile.x + 1,
            y: npc_tile.y,
            z: npc_tile.z,
        };
        resident.space_id = npc_space;
    }

    // Talk. Exactly one DialogLine (or DialogOptions) must arrive — the
    // greeting delivered once, not once-per-delivery-path.
    client.send(ClientMessage::Command(GameCommand::TalkToNpc {
        npc_object_id,
    }));
    let events = collect_ui_events(&mut app, &mut client, 30);
    let lines: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, GameUiEvent::DialogLine { .. }))
        .collect();
    let session_id = events
        .iter()
        .find_map(|e| match e {
            GameUiEvent::DialogLine { session_id, .. }
            | GameUiEvent::DialogOptions { session_id, .. } => Some(*session_id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no dialog events after TalkToNpc; got {events:?}"));
    assert!(
        lines.len() <= 1,
        "greeting line was delivered {} times (dup delivery); events: {events:?}",
        lines.len()
    );

    // Advance until the dialog yields options or finishes; every advance must
    // produce a visible response.
    let mut saw_response_to_advance = false;
    for _ in 0..5 {
        client.send(ClientMessage::Command(GameCommand::DialogAdvance {
            session_id,
        }));
        let events = collect_ui_events(&mut app, &mut client, 30);
        if events.iter().any(|e| {
            matches!(
                e,
                GameUiEvent::DialogLine { .. }
                    | GameUiEvent::DialogOptions { .. }
                    | GameUiEvent::DialogClose { .. }
            )
        }) {
            saw_response_to_advance = true;
            break;
        }
    }
    assert!(
        saw_response_to_advance,
        "DialogAdvance produced no response — dialog is stuck"
    );

    // End the dialog; the server must confirm with DialogClose.
    client.send(ClientMessage::Command(GameCommand::DialogEnd {
        session_id,
    }));
    let events = collect_ui_events(&mut app, &mut client, 30);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameUiEvent::DialogClose { .. })),
        "DialogEnd did not produce DialogClose; got {events:?}"
    );
}
