//! Integration test for the admin Python REPL.
//!
//! Spins up a `HeadlessServer` `App` with an admin UNIX-socket listener,
//! connects as a client, exercises the REPL: simple expression evaluation,
//! multi-line `def`, `world.spawn`, and `world.attach_player`.
//!
//! Gated `#[ignore]` like `multiplayer_transport.rs` because it does real
//! socket I/O against the running app. Run with `cargo test --test
//! admin_repl -- --ignored`.

#![cfg(unix)]

use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mud2::app::plugin::{AppRuntime, GameAppPlugin};
use mud2::network::AdminListenArgs;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn unique_temp(prefix: &str, suffix: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "mud2-admin-{}-{}-{}{}",
        std::process::id(),
        prefix,
        id,
        suffix
    ))
}

fn build_app(socket_path: PathBuf) -> App {
    let mut app = App::new();
    app.add_plugins(GameAppPlugin {
        runtime: AppRuntime::HeadlessServer,
        server_addr: None,
        bind_addr: Some("127.0.0.1:0".to_owned()),
        save_path: Some(unique_temp("save", "-world.json")),
        db_path: Some(unique_temp("db", ".db")),
        asset_cache_dir: None,
        server_tls: None,
        client_tls: None,
        admin_socket: Some(AdminListenArgs {
            socket_path,
            mode: 0o600,
        }),
    });
    app.update();
    app
}

fn pump(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
        thread::sleep(Duration::from_millis(5));
    }
}

struct ReplClient {
    stream: UnixStream,
    buffer: Vec<u8>,
}

impl ReplClient {
    fn connect(socket: &PathBuf) -> Self {
        let stream = UnixStream::connect(socket).expect("connect to admin socket");
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        Self {
            stream,
            buffer: Vec::new(),
        }
    }

    fn send(&mut self, line: &str) {
        self.stream.write_all(line.as_bytes()).unwrap();
        if !line.ends_with('\n') {
            self.stream.write_all(b"\n").unwrap();
        }
        self.stream.flush().unwrap();
    }

    fn drain_some(&mut self) -> &[u8] {
        let mut chunk = [0u8; 4096];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
                Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    break
                }
                Err(err) => panic!("admin REPL read error: {err}"),
            }
        }
        &self.buffer
    }

    /// Pump the app and drain the socket until `predicate` is satisfied with
    /// the buffer contents (compared as a UTF-8 string). On success returns
    /// the matched buffer and clears it. Times out after 3s.
    fn wait_for<F>(&mut self, app: &mut App, predicate: F) -> String
    where
        F: Fn(&str) -> bool,
    {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            pump(app, 2);
            let snapshot = self.drain_some();
            let s = std::str::from_utf8(snapshot).unwrap_or("");
            if predicate(s) {
                let owned = s.to_owned();
                self.buffer.clear();
                return owned;
            }
            if Instant::now() >= deadline {
                panic!(
                    "admin REPL: timed out waiting; buffer = {:?}",
                    std::str::from_utf8(snapshot).unwrap_or("<non-utf8>")
                );
            }
        }
    }
}

#[test]
#[ignore = "requires unix socket bind support"]
fn admin_repl_evaluates_expression_via_displayhook() {
    let socket_path = unique_temp("sock", ".sock");
    let mut app = build_app(socket_path.clone());

    let mut client = ReplClient::connect(&socket_path);
    client.wait_for(&mut app, |s| s.contains(">>> "));

    client.send("1+1");
    let output = client.wait_for(&mut app, |s| s.contains("2\n") && s.contains(">>> "));
    assert!(
        output.contains("2"),
        "expected `2` from `1+1` evaluation; got {output:?}"
    );
}

#[test]
#[ignore = "requires unix socket bind support"]
fn admin_repl_handles_multi_line_def() {
    let socket_path = unique_temp("sock", ".sock");
    let mut app = build_app(socket_path.clone());

    let mut client = ReplClient::connect(&socket_path);
    client.wait_for(&mut app, |s| s.contains(">>> "));

    client.send("def f():");
    client.wait_for(&mut app, |s| s.contains("... "));
    client.send("    return 42");
    // Blank line forces flush of the pending block.
    client.send("");
    client.wait_for(&mut app, |s| s.contains(">>> "));

    client.send("f()");
    let output = client.wait_for(&mut app, |s| s.contains("42\n") && s.contains(">>> "));
    assert!(
        output.contains("42"),
        "expected `42` from `f()`; got {output:?}"
    );
}

#[test]
#[ignore = "requires unix socket bind support"]
fn admin_repl_can_introspect_world_state() {
    // Read-side proof that the snapshot/bindings pipeline is hooked up:
    // `world.object_types()` must return something non-empty (the bundled
    // object registry has dozens of entries), and `world.spaces()` must list
    // at least one space (the bootstrap overworld). These verbs don't need
    // a live caller to work, so they're a clean end-to-end check that the
    // listener / compile / execute / response path is all wired up.
    let socket_path = unique_temp("sock", ".sock");
    let mut app = build_app(socket_path.clone());

    let mut client = ReplClient::connect(&socket_path);
    client.wait_for(&mut app, |s| s.contains(">>> "));

    client.send("print(len(world.object_types()))");
    let types_resp = client.wait_for(&mut app, |s| s.contains(">>> "));
    let count_line = types_resp
        .lines()
        .find(|line| line.chars().all(|c| c.is_ascii_digit()) && !line.is_empty())
        .expect("expected a numeric line in object_types output");
    let count: usize = count_line.parse().expect("numeric line should parse");
    assert!(
        count > 0,
        "expected world.object_types() to be non-empty in headless server; got {count}"
    );

    client.send("print(len(world.spaces()))");
    let spaces_resp = client.wait_for(&mut app, |s| s.contains(">>> "));
    assert!(
        spaces_resp
            .lines()
            .any(|line| line.trim().parse::<usize>().is_ok_and(|n| n > 0)),
        "expected at least one space in headless server; got {spaces_resp:?}"
    );
}

#[test]
#[ignore = "requires unix socket bind support"]
fn admin_repl_attach_player_changes_caller_state() {
    let socket_path = unique_temp("sock", ".sock");
    let mut app = build_app(socket_path.clone());

    let mut client = ReplClient::connect(&socket_path);
    client.wait_for(&mut app, |s| s.contains(">>> "));

    // No caller attached yet — `world.player()` should be None.
    client.send("print(world.player())");
    let unattached = client.wait_for(&mut app, |s| s.contains(">>> "));
    assert!(
        unattached.contains("None"),
        "expected `None` from world.player() before attach; got {unattached:?}"
    );

    // Attach to player id 0 (LOCAL_ACCOUNT_ID; may not have a live player but
    // attach itself should not error).
    client.send("world.attach_player(0)");
    client.wait_for(&mut app, |s| s.contains(">>> "));

    // Confirm caller_player_id is now reported as 0.
    client.send("print(world.caller_player_id())");
    let attached = client.wait_for(&mut app, |s| s.contains(">>> "));
    assert!(
        attached.contains('0'),
        "expected caller_player_id to be 0 after attach; got {attached:?}"
    );
}
