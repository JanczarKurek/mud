//! End-to-end ground-item pickup in an *ephemeral* space over the TCP wire
//! protocol. Reproduces the playtest flow that broke in the proving grounds:
//! a scripted client authenticates, walks through the overworld portal (which
//! lazily instantiates the dungeon), walks to the potion, and picks it up —
//! then leaves, re-enters (fresh instance, same authored object ids), and
//! repeats; finally disconnects inside the instance and reconnects, which must
//! not resume the character into a dead ephemeral space id.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};
use mud2::game::commands::{GameCommand, ItemDestination, ItemReference, ItemSlotRef, MoveDelta};
use mud2::network::protocol::{ClientMessage, ServerMessage};
use mud2::network::resources::TcpServerState;
use mud2::player::classes::Class;
use mud2::player::components::{AttributeSet, Inventory, Player, PlayerAppearance};
use mud2::world::components::{OverworldObject, SpaceId, SpaceResident, TilePosition};
use mud2::world::resources::SpaceManager;

static NEXT_DB_ID: AtomicU64 = AtomicU64::new(0);

fn unique_test_path(suffix: &str) -> PathBuf {
    let id = NEXT_DB_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "mud2-pickup-{}-{}-{suffix}",
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

fn pump(app: &mut App, client: &mut TestClient, ticks: usize) {
    for _ in 0..ticks {
        app.update();
        thread::sleep(Duration::from_millis(5));
        client.read_messages();
    }
}

fn player_position(app: &mut App) -> (TilePosition, SpaceId) {
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
fn step(app: &mut App, client: &mut TestClient, dx: i32, dy: i32) {
    let (from, from_space) = player_position(app);
    let deadline = Instant::now() + Duration::from_secs(10);
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

/// Walk an axis-aligned path expressed as (dx, dy) runs.
fn walk(app: &mut App, client: &mut TestClient, runs: &[(i32, i32, usize)]) {
    for &(dx, dy, count) in runs {
        for _ in 0..count {
            step(app, client, dx, dy);
        }
    }
}

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

fn boot_server(save: PathBuf, db: PathBuf) -> App {
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
    });
    app.update();
    app
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

fn auth_and_enter(
    app: &mut App,
    client: &mut TestClient,
    register: bool,
    character_id: Option<i64>,
) -> i64 {
    if register {
        client.send(ClientMessage::Register {
            username: "pickup_bot".to_owned(),
            password: "secret123".to_owned(),
        });
    } else {
        client.send(ClientMessage::Login {
            username: "pickup_bot".to_owned(),
            password: "secret123".to_owned(),
        });
    }
    client.wait_for(app, "AuthResult", |m| match m {
        ServerMessage::AuthResult { ok, reason } => {
            assert!(*ok, "auth rejected: {reason:?}");
            Some(())
        }
        _ => None,
    });
    let character_id = match character_id {
        Some(id) => id,
        None => {
            client.send(ClientMessage::CreateCharacter {
                name: "Grabby".to_owned(),
                class: Class::Fighter,
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
    };
    client.send(ClientMessage::SelectCharacter { character_id });
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
    character_id
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
        *tile = TilePosition { x: 46, y: 25, z: 0 };
        resident.space_id = overworld;
    }
    pump(app, client, 5);

    // Real portal traversal: one step east onto the arch at (47,25).
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
    auth_and_enter(&mut app, &mut client, true, None);

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
    auth_and_enter(&mut app, &mut client, true, None);

    let first_space = enter_proving_grounds(&mut app, &mut client);
    // Step off the arrival arch and back onto it to ride pg_exit out.
    step(&mut app, &mut client, 1, 0);
    step(&mut app, &mut client, -1, 0);
    let (tile, space) = player_position(&mut app);
    assert_ne!(space, first_space, "pg_exit should return to the overworld");
    assert_eq!((tile.x, tile.y), (46, 25), "pg_exit landing tile");
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
    let character_id = auth_and_enter(&mut app, &mut client, true, None);

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
    auth_and_enter(&mut app, &mut client, false, Some(character_id));

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
