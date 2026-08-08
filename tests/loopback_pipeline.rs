//! The in-process loopback transport end-to-end: one App runs both the
//! server systems (`TcpServerPlugin { bind_addr: None }` — no listener) and
//! the client systems (`TcpClientPlugin`), connected by
//! `network::loopback::connect_loopback`. The character flow, bootstrap event
//! stream, and gameplay commands all cross the real newline-framed serde_json
//! wire path — and thanks to the `network::sets` pipeline ordering, a command
//! pushed before an `App::update` is reflected in `ClientGameState` when that
//! same update returns.

mod common;

use bevy::prelude::*;
use common::unique_test_path;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};
use mud2::app::state::ClientAppState;
use mud2::game::commands::{GameCommand, MoveDelta};
use mud2::game::resources::{
    ClientGameState, ClientPendingCommands, GameUiEvent, PendingGameUiEvents,
};
use mud2::network::loopback::connect_loopback;
use mud2::network::protocol::{ClientMessage, ServerMessage};
use mud2::network::resources::{TcpClientConnection, TcpServerState};
use mud2::network::systems::{read_next_line, write_message};
use mud2::player::classes::Class;
use mud2::player::components::{Player, PlayerAppearance, RgbColor};
use mud2::world::components::TilePosition;

fn build_loopback_app() -> App {
    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        debug: false,
        server_addr: None,
        // Ephemeral port; the test never connects to it — all traffic rides
        // the loopback pipe. (`GameAppPlugin` maps `None` to the default port,
        // so `None` here would collide with a locally running server.)
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: Some(unique_test_path("world.json")),
        db_path: Some(unique_test_path("accounts.db")),
        asset_cache_dir: None,
        server_tls: None,
        client_tls: None,
        admin_socket: None,
        embedded_extension: None,
    });
    // The client half. `MinimalPlugins` has no `StatesPlugin`, and the client
    // systems gate on `ClientAppState`.
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.init_state::<ClientAppState>();
    app.add_plugins(mud2::network::TcpClientPlugin {
        server_addr: String::new(),
        tls: None,
    });
    app.update();
    app
}

/// Write a message on the client half of the pipe (what the pre-InGame
/// screens do on a real client).
fn client_send(app: &mut App, message: &ClientMessage) {
    let mut connection = app.world_mut().resource_mut::<TcpClientConnection>();
    let stream = connection.stream.as_mut().expect("loopback stream");
    let mut disconnected = false;
    assert!(
        write_message(stream, message, &mut disconnected),
        "loopback write failed"
    );
}

/// Drain every complete message currently on the client half of the pipe.
fn client_read(app: &mut App) -> Vec<ServerMessage> {
    let mut connection = app.world_mut().resource_mut::<TcpClientConnection>();
    let connection = &mut *connection;
    let stream = connection.stream.as_mut().expect("loopback stream");
    let mut out = Vec::new();
    let mut disconnected = false;
    while let Some(line) = read_next_line(stream, &mut connection.read_buffer, &mut disconnected) {
        out.push(serde_json::from_str(&line).expect("valid ServerMessage"));
    }
    out
}

/// Pump updates until `pick` claims a message (handshake helper — the client
/// systems are not running pre-InGame, so the test reads the pipe directly).
fn wait_for<T>(app: &mut App, what: &str, mut pick: impl FnMut(&ServerMessage) -> Option<T>) -> T {
    for _ in 0..200 {
        app.update();
        for message in client_read(app) {
            if let Some(value) = pick(&message) {
                return value;
            }
        }
    }
    panic!("loopback handshake: never saw {what}");
}

fn authoritative_tile(app: &mut App) -> TilePosition {
    let mut players = app
        .world_mut()
        .query_filtered::<&TilePosition, With<Player>>();
    *players.single(app.world()).expect("one spawned player")
}

#[test]
fn loopback_pipeline_full_flow_same_frame() {
    let mut app = build_loopback_app();

    // Connect the pipe: peer is born AwaitingCharacter on the local account.
    app.world_mut()
        .resource_scope(|world, mut server_state: Mut<TcpServerState>| {
            let mut connection = world.resource_mut::<TcpClientConnection>();
            connect_loopback(&mut server_state, &mut connection);
        });

    // Character flow over the real wire (Login/Register skipped by design).
    client_send(
        &mut app,
        &ClientMessage::CreateCharacter {
            name: "Loopy".to_owned(),
            class: Class::Fighter,
            attributes: common::budget_attributes(),
            appearance: PlayerAppearance::default(),
        },
    );
    let character_id = wait_for(&mut app, "CharacterCreateResult", |m| match m {
        ServerMessage::CharacterCreateResult {
            ok,
            character_id,
            reason,
        } => {
            assert!(*ok, "create rejected: {reason:?}");
            Some(character_id.unwrap())
        }
        _ => None,
    });
    client_send(
        &mut app,
        &ClientMessage::SelectCharacter {
            character_id,
            start_map: None,
        },
    );
    wait_for(&mut app, "CharacterSelected", |m| match m {
        ServerMessage::CharacterSelected { .. } => Some(()),
        _ => None,
    });
    wait_for(&mut app, "AssetManifest", |m| match m {
        ServerMessage::AssetManifest(entries) => {
            assert!(!entries.is_empty(), "manifest crossed the pipe");
            Some(())
        }
        _ => None,
    });
    client_send(&mut app, &ClientMessage::SyncComplete);

    // Hand over to the in-App client systems: enter InGame so the
    // NetClientSend/NetClientReceive systems and the simulation run.
    app.world_mut()
        .resource_mut::<NextState<ClientAppState>>()
        .set(ClientAppState::InGame);
    app.update(); // apply the state transition
    app.update(); // first synced frame: bootstrap events → ClientGameState

    let state = app.world().resource::<ClientGameState>();
    assert!(
        state.local_player_id.is_some(),
        "bootstrap identified the local player through the loopback wire"
    );
    let bootstrap_tile = state
        .player_tile_position
        .expect("bootstrap replicated the player position");
    assert_eq!(bootstrap_tile, authoritative_tile(&mut app));

    // Bytes really crossed the serde/framing boundary.
    let bytes_out = app
        .world()
        .resource::<TcpServerState>()
        .peers
        .values()
        .map(|peer| peer.throughput.bytes_out)
        .sum::<u64>();
    assert!(bytes_out > 0, "server wrote bytes through the pipe");

    // Let the spawn-time movement cooldown expire (it ticks on real time).
    for _ in 0..10 {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // The same-frame guarantee: one command push + ONE update = the client
    // state already shows the move (send → ingest → simulate → diff → fold
    // all ordered within a single frame by `network::sets`).
    let before = authoritative_tile(&mut app);
    app.world_mut()
        .resource_mut::<ClientPendingCommands>()
        .push(GameCommand::MovePlayer {
            delta: MoveDelta { x: 1, y: 0 },
            climb: false,
        });
    app.update();

    let after = authoritative_tile(&mut app);
    assert_ne!(before, after, "the move landed on the authority");
    let state = app.world().resource::<ClientGameState>();
    assert_eq!(
        state.player_tile_position,
        Some(after),
        "ClientGameState reflects the move within the same App::update"
    );
}

/// Class and appearance must reach the client as replicated state: the sprite
/// layers key off `ClientGameState.appearance` (the authoritative
/// `PlayerAppearance` lives on the server-side entity, which carries no
/// sprite), and the per-class sheet is chosen from `ClientGameState.class`.
#[test]
fn loopback_replicates_class_and_appearance() {
    let mut app = build_loopback_app();

    app.world_mut()
        .resource_scope(|world, mut server_state: Mut<TcpServerState>| {
            let mut connection = world.resource_mut::<TcpClientConnection>();
            connect_loopback(&mut server_state, &mut connection);
        });

    let chosen = PlayerAppearance {
        hair: RgbColor::new(200, 30, 40),
        torso: RgbColor::new(20, 180, 90),
        trousers: RgbColor::new(60, 70, 210),
    };
    client_send(
        &mut app,
        &ClientMessage::CreateCharacter {
            name: "Prism".to_owned(),
            class: Class::Wizard,
            attributes: common::budget_attributes(),
            appearance: chosen,
        },
    );
    let character_id = wait_for(&mut app, "CharacterCreateResult", |m| match m {
        ServerMessage::CharacterCreateResult {
            ok,
            character_id,
            reason,
        } => {
            assert!(*ok, "create rejected: {reason:?}");
            Some(character_id.unwrap())
        }
        _ => None,
    });
    client_send(
        &mut app,
        &ClientMessage::SelectCharacter {
            character_id,
            start_map: None,
        },
    );
    wait_for(&mut app, "CharacterSelected", |m| match m {
        ServerMessage::CharacterSelected { .. } => Some(()),
        _ => None,
    });
    client_send(&mut app, &ClientMessage::SyncComplete);

    app.world_mut()
        .resource_mut::<NextState<ClientAppState>>()
        .set(ClientAppState::InGame);
    app.update();
    app.update();

    let state = app.world().resource::<ClientGameState>();
    assert_eq!(
        state.class,
        Some(Class::Wizard),
        "class crossed the wire into ClientGameState"
    );
    assert_eq!(
        state.appearance,
        Some(chosen),
        "the character's chosen colors crossed the wire into ClientGameState"
    );

    // The diff is a diff: a steady frame re-emits neither event.
    let before = app.world().resource::<ClientGameState>().clone();
    app.update();
    let after = app.world().resource::<ClientGameState>();
    assert_eq!(before.appearance, after.appearance);
    assert_eq!(before.class, after.class);
}

/// Drain every `ReplOutput` UI event the loopback client has received. The
/// test app has no `UiPlugin`, so the inbox accumulates until we take it.
fn drain_repl_outputs(app: &mut App) -> Vec<(Vec<String>, Option<String>, bool)> {
    let mut ui_events = app.world_mut().resource_mut::<PendingGameUiEvents>();
    std::mem::take(&mut ui_events.events)
        .into_iter()
        .filter_map(|event| match event {
            GameUiEvent::ReplOutput {
                lines,
                error,
                incomplete,
            } => Some((lines, error, incomplete)),
            _ => None,
        })
        .collect()
}

fn exec_python(app: &mut App, code: &str) -> Vec<(Vec<String>, Option<String>, bool)> {
    app.world_mut()
        .resource_mut::<ClientPendingCommands>()
        .push(GameCommand::AdminExec {
            code: code.to_owned(),
        });
    let mut outputs = Vec::new();
    for _ in 0..10 {
        app.update();
        outputs.extend(drain_repl_outputs(app));
        if !outputs.is_empty() {
            break;
        }
    }
    outputs
}

/// The in-game Python console pipeline over the wire: the loopback (admin)
/// peer executes Python and gets output back; a plain TCP peer is rejected
/// until an admin grants its account the flag (picked up on reconnect).
#[test]
fn loopback_repl_admin_gate_and_execution() {
    let mut app = build_loopback_app();

    app.world_mut()
        .resource_scope(|world, mut server_state: Mut<TcpServerState>| {
            let mut connection = world.resource_mut::<TcpClientConnection>();
            connect_loopback(&mut server_state, &mut connection);
        });
    client_send(
        &mut app,
        &ClientMessage::CreateCharacter {
            name: "Console".to_owned(),
            class: Class::Wizard,
            attributes: common::budget_attributes(),
            appearance: PlayerAppearance::default(),
        },
    );
    let character_id = wait_for(&mut app, "CharacterCreateResult", |m| match m {
        ServerMessage::CharacterCreateResult { character_id, .. } => Some(character_id.unwrap()),
        _ => None,
    });
    client_send(
        &mut app,
        &ClientMessage::SelectCharacter {
            character_id,
            start_map: None,
        },
    );
    wait_for(&mut app, "AssetManifest", |m| match m {
        ServerMessage::AssetManifest(_) => Some(()),
        _ => None,
    });
    client_send(&mut app, &ClientMessage::SyncComplete);
    app.world_mut()
        .resource_mut::<NextState<ClientAppState>>()
        .set(ClientAppState::InGame);
    app.update();
    app.update();

    // Loopback peer is admin by construction: expressions evaluate.
    let outputs = exec_python(&mut app, "6 * 7");
    assert!(
        outputs
            .iter()
            .any(|(lines, error, _)| error.is_none() && lines.iter().any(|l| l.contains("42"))),
        "expected 42 from the REPL, got {outputs:?}"
    );

    // Multi-line block: first line reports incomplete, the closing blank
    // line executes it.
    let outputs = exec_python(&mut app, "def sq(x):");
    assert!(
        outputs.iter().any(|(_, _, incomplete)| *incomplete),
        "block opener should report incomplete, got {outputs:?}"
    );
    exec_python(&mut app, "    return x * x");
    exec_python(&mut app, "");
    let outputs = exec_python(&mut app, "sq(9)");
    assert!(
        outputs
            .iter()
            .any(|(lines, error, _)| error.is_none() && lines.iter().any(|l| l.contains("81"))),
        "expected 81 from the buffered block, got {outputs:?}"
    );

    // A plain TCP account is not an admin: AdminExec is rejected.
    let addr = common::server_addr(&app);
    let mut tcp = common::TestClient::connect(addr);
    let bot_character_id =
        common::register_and_enter_world(&mut app, &mut tcp, "repl_bot", "NoAdmin");
    tcp.send(ClientMessage::Command(GameCommand::AdminExec {
        code: "1 + 1".to_owned(),
    }));
    let rejected = common::collect_ui_events(&mut app, &mut tcp, 10);
    assert!(
        rejected.iter().any(|event| matches!(
            event,
            GameUiEvent::ReplOutput { error: Some(err), .. } if err.contains("not authorized")
        )),
        "non-admin AdminExec must be rejected, got {rejected:?}"
    );

    // The loopback admin grants the flag; the TCP account picks it up on its
    // next login (the peer's admin bit is loaded at character select).
    let outputs = exec_python(&mut app, "world.grant_admin(\"repl_bot\")");
    assert!(
        outputs
            .iter()
            .any(|(lines, error, _)| error.is_none()
                && lines.iter().any(|l| l.contains("granted"))),
        "grant_admin should confirm, got {outputs:?}"
    );

    drop(tcp);
    for _ in 0..10 {
        app.update();
    }
    let mut tcp = common::TestClient::connect(addr);
    common::auth(&mut app, &mut tcp, "repl_bot", "secret123", false);
    common::enter_world(&mut app, &mut tcp, bot_character_id);
    tcp.send(ClientMessage::Command(GameCommand::AdminExec {
        code: "3 + 4".to_owned(),
    }));
    let accepted = common::collect_ui_events(&mut app, &mut tcp, 10);
    assert!(
        accepted.iter().any(|event| matches!(
            event,
            GameUiEvent::ReplOutput { lines, error: None, .. } if lines.iter().any(|l| l.contains("7"))
        )),
        "granted account should execute Python, got {accepted:?}"
    );
}
