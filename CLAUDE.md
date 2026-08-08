# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Mud 2.0 is a Tibia-inspired multiplayer MUD built with **Bevy 0.18** (Rust game engine) using ECS architecture. It supports embedded single-player, TCP multiplayer with a headless server, and features grid-based movement, equipment, combat, magic, NPC AI, persistent world saves, and an embedded Python scripting console.

## Build & Run Commands

```bash
cargo run --bin mud2                            # Run game (embedded client+server)
cargo run --bin server                          # Headless TCP server
cargo run --bin mud2 -- --tcp-client            # TCP client (server picked on the title screen)
cargo check                                     # Always run after changes before reporting success
cargo check -p mud2-lib --no-default-features   # Thin-client config — keep this green too
cargo test                                      # Run tests (e2e suites in tests/ want -- --test-threads=1)
cargo fmt                                       # Format code
cargo clippy                                    # Lint (fix warnings before merging)
packaging/build-appimage.sh                     # Linux AppImage (run inside nix-shell)
packaging/build-windows.sh                      # Windows x86_64 zip via mingw cross-compile (run inside nix-shell)
```

### The `server-sim` Feature (thin client)

The authoritative world simulation is gated behind the default-on `server-sim`
Cargo feature of mud2-lib (mirrored by the root package; the root's `editor`
feature implies it). Building `--no-default-features` produces the **thin
client**: TcpClient-only (no embedded single-player, no headless server, no
editor), and drops rustpython, yarnspinner, sqlite, and argon2 entirely.
**Packaged builds (AppImage / Windows zip) are thin clients by design** — the
scripts' `--no-default-features` does this; add `--features editor` for an
offline-capable build. Rules when touching gated code:
- Wire-protocol types (`GameCommand`, `GameEvent`, `network/protocol.rs`,
  `ClientGameState`) must never be feature-gated — thin clients and full
  servers share one protocol.
- Never order client systems `.before(some_server_fn)` across the feature
  boundary; use an ungated `SystemSet` (see `PythonConsoleToggleSet` in
  `scripting/resources.rs`).
- `cargo test --no-default-features` is unsupported by design (tests exercise
  the sim); test with default features, and keep
  `cargo check -p mud2-lib --no-default-features` compiling.

## Architecture

### Workspace Layout
- **`crates/mud2-lib`** — the game library. Its lib *target* is still named `mud2`, so all `mud2::...` imports (bins, tests, editor) work unchanged. All source paths below live under `crates/mud2-lib/src/`.
- **`crates/mud2-editor`** — the in-game map editor plus the asset/floor viewer modules. Depends on mud2-lib; the lib never depends on it. The `mud2` binary plugs `EditorPlugin` in via `GameAppPlugin::embedded_extension`.
- **root package `mud2-bins`** — only the binaries (`mud2`, `server`, `asset_viewer`, `floor_viewer`, `gen_schemas`), so `cargo run --bin <x>` keeps working from the repo root. The headless server never compiles editor code.
- **`crates/bevy_terminal`** — leaf terminal-widget crate.
- `crates/mud2-lib/assets` and `crates/mud2-editor/assets` are symlinks to the repo-root `assets/` so unit tests (which run with the crate dir as CWD) can use the same relative asset paths as the game.

### Three Runtime Modes (configured in `app/plugin.rs`)
- **EmbeddedClient**: Single binary, shared memory client+server (default, for dev)
- **TcpClient**: Connects to remote server over TCP
- **HeadlessServer**: No graphics, listens for TCP connections

### Server-Authoritative Flow
1. Client input/UI push **commands** into `ClientPendingCommands` (the client outbox)
2. `flush_client_commands_to_server` serializes them over the transport; the server ingests them into `PendingGameCommands`, tagged with the sending peer's `PlayerId`
3. Server validates and processes commands (`crates/mud2-lib/src/game/systems.rs`)
4. `flush_server_messages` diffs per-peer state via `compute_events_for_peer` and sends **game events**
5. Client folds events into `ClientGameState` via `apply_game_events_to_client_state`

### The EmbeddedClient Pipeline (one wire, all modes)
EmbeddedClient mode = HeadlessServer + TcpClient running in the same `App`, connected by an **in-process loopback byte pipe** (`crates/mud2-lib/src/network/loopback.rs`) instead of a socket. There is no bypass: embedded runs the exact TCP pipeline — newline-framed serde_json included — so offline play cannot drift from networked play. Rules when adding systems:
- **Frame ordering** is declared through the ungated `network::sets` SystemSets: input → `NetClientSend` (client outbox → pipe) → `NetServerReceive` (pipe → `PendingGameCommands`, before `CommandIntercept`) → simulation → `NetServerSend` (per-peer diff → pipe) → `NetClientReceive` (pipe → event queues) → `apply_game_events_to_client_state` → presentation. In embedded mode this whole chain completes within one `Update`, so input still lands on screen the same frame.
- **Two command queues, two roles.** `ClientPendingCommands` is client intent and is drained *only* by the flush — it always crosses the wire and comes back peer-attributed. `PendingGameCommands` is the server-side queue (network ingest, admin REPL, scripts, editor); untargeted entries there are trusted as server-internal. Never push client intent into `PendingGameCommands` — in the unified embedded App it would be consumed locally and bypass the wire.
- **Server-side systems** must emit changes by mutating authoritative components/resources; `compute_events_for_peer` (in the `NetServerSend` flush) diffs them per peer. Systems that mutate replicated state late in the frame need a `.before(crate::network::sets::NetServerSend)` edge. Never mutate `ClientGameState` directly, and never push `GameEvent`s into `PendingGameEvents` from server systems — that queue is the client-side inbox.
- **Client-side (presentation) systems** must read from `ClientGameState` or from view-only components (`DisplayedVitalStats`, `ViewPosition`). Never query authoritative components (`VitalStats`, `SpaceResident`, `TilePosition`) from presentation code. The local player entity is a projected stub (`Without<PlayerIdentity>`) in *every* client mode; the authoritative player entity (with `PlayerIdentity`) coexists in the same World in embedded mode and carries no visuals — filter presentation queries accordingly.
- `apply_game_events_to_client_state` is the single fold function that turns events into client state; `GameServerPlugin` and `GameClientPlugin` both register it so ordering is identical in all three runtime modes (`crates/mud2-lib/src/game/mod.rs`).
- **Two event channels, two roles.** `GameEvent` (via `ServerMessage::Events`) is state replication — every field of `ClientGameState` is reachable through a `GameEvent` variant, and `compute_events_for_peer` is the sole serializer. `GameUiEvent` (via `ServerMessage::UiEvents`) is a one-shot signal bus orthogonal to state; per-player events go through `PendingGameUiEvents::push`, broadcasts through `push_broadcast` — the `.events` field is the client inbox, never written server-side.
- Embedded auth: the title screen's Play calls `connect_loopback`, which registers a peer born `AwaitingCharacter` on `LOCAL_ACCOUNT_ID` (the in-process pipe is the trust model; Login/Register are skipped). Everything from `ListCharacters` on — character CRUD, asset sync, gameplay — is real wire traffic.
- Never order client systems `.before(some_server_fn)` across the feature boundary; use the ungated sets in `network/sets.rs` (or `PythonConsoleToggleSet`).

### Module Layout (`crates/mud2-lib/src/`)
- **accounts/**: sqlite-backed account database (Argon2 hashed passwords), per-character save/load, autosave system
- **app/**: Bevy app setup, plugins, state machine, title screen, auth screen
- **game/**: Core command/event loop (commands.rs, resources.rs, systems.rs)
- **world/**: Map spaces, tiles, objects, object registry, collision
- **player/**: Player components (stats, inventory, chat), input handling
- **combat/**: Battle system, damage resolution, attack profiles
- **magic/**: Spell definitions loaded from YAML
- **npc/**: NPC AI (roaming, hostile chase behavior)
- **network/**: TCP protocol, connection management, message ser/de, TLS transport wrapper, loopback pipe, `sets.rs` pipeline ordering
- **persistence/**: World snapshot save/load (JSON format; players live in `accounts.db`, not this snapshot)
- **ui/**: HUD, docked panels, context menus, cursor management
- **scripting/**: Python console UI (thin client, ungated) + server-side RustPython REPL host (`admin_host.rs`, `game_repl.rs`, gated)

(Editor and viewers live in `crates/mud2-editor/src/`.)

### Auth & Persistence

- Every TCP connection must `Login` / `Register` before the server will send the asset manifest or any gameplay events. The peer state machine is `AwaitingAuth → AwaitingCharacter { account_id } → Authed { account_id, character_id }` (`crates/mud2-lib/src/network/resources.rs`). The embedded loopback peer skips credentials and is born `AwaitingCharacter` on the reserved `LOCAL_ACCOUNT_ID = 0` (`connect_loopback`).
- `PlayerId(character_id as u64)` — set at character select from the DB row, identically for TCP and loopback peers (`crates/mud2-lib/src/accounts/db.rs`).
- On-disk layout is per-role (see `crates/mud2-lib/src/app/paths.rs` — the single source of truth):

  | Role | Accounts DB | World snapshot | Asset cache |
  |---|---|---|---|
  | EmbeddedClient | `~/.local/share/mud2/embedded/accounts.db` | `~/.local/share/mud2/embedded/saves/world-state.json` | — |
  | HeadlessServer | `~/.local/share/mud2/server/accounts.db` | `~/.local/share/mud2/server/saves/world-state.json` | — |
  | TcpClient | — | — | `~/.cache/mud2/client/assets/` |

  Overrides: `--db-path` / `MUD2_DB_PATH`, `--save-path` / `MUD2_SAVE_PATH`, `--asset-cache` / `MUD2_ASSET_CACHE`. Run `mud2 paths` to print resolved locations; `mud2 clean-cache` wipes the client cache (`--all --yes` also wipes data).
- Per-character saves happen on disconnect (`PendingPlayerSaves` queue drained by `persist_disconnected_players` in the `Last` schedule), every 60s via `autosave_all_players`, and on `AppExit`.
- `WorldStateDump` **does not carry player data** (as of `format_version = 5`). If you need to save anything about a player, route it through the accounts DB.

### TLS

- `ServerTransport` / `ClientTransport` (`crates/mud2-lib/src/network/transport.rs`) wrap the raw `TcpStream` with optional TLS via `rustls::StreamOwned`. Sync nonblocking throughout — no tokio.
- Server: `--tls --tls-cert PATH --tls-key PATH`, plus `--generate-cert` (requires `dev-self-signed` Cargo feature) to emit a self-signed pair.
- Client: `--tls` uses `webpki-roots` trust anchors; `--insecure` skips verification (dev only). `--connect tls://host:port` is shorthand for both.

### Admin Python REPL

Two front ends share one server-side `AdminReplHost` interpreter (`crates/mud2-lib/src/scripting/admin_host.rs`) — one persistent Python scope, so admins can collaborate on globals regardless of how they connected. Live-bind a session to act-as a player with `world.attach_player(player_id)` (`world.attach_player(None)` detaches).

- **In-game console** (backtick, any client mode): the console UI is a thin client — each line travels as `GameCommand::AdminExec` and output returns as `GameUiEvent::ReplOutput`. Execution happens server-side in `crates/mud2-lib/src/scripting/game_repl.rs`, gated on the account's `is_admin` flag in the accounts DB (the embedded local account is always admin; grant others with `world.grant_admin("username")` / revoke with `world.revoke_admin`, effective on their next login). Thin clients ship the console UI but no interpreter.
- **UNIX socket** (HeadlessServer): pass `--admin-socket [PATH]`; default path is `~/.local/share/mud2/server/admin.sock`. Auth is by filesystem permissions (default mode `0600`, override with `--admin-socket-mode 660`). Connect with `nc -U <path>` or `socat - UNIX-CONNECT:<path>`. The listener (`crates/mud2-lib/src/network/admin.rs`, `#[cfg(unix)]`) reuses the sync-nonblocking pattern from `crates/mud2-lib/src/network/systems.rs`; stale sockets from a prior crash are auto-reclaimed at startup if no live listener answers on them.
- `AdminReplHost` compiles input as `Mode::Single` and pipes `sys.stdout` / `sys.stderr` / `sys.displayhook` through `world.log`, so bare expressions print their `repr` like CPython's REPL. Multi-line input is buffered per session until a blank line force-flushes (mirrors CPython). All Python execution happens on the Bevy main thread; no background threads.

### Data-Driven Design
- Map layouts: `assets/maps/*.yaml`
- Object definitions: `assets/overworld_objects/*/metadata.yaml`
- Spell definitions: `assets/spells/*.yaml`
- YAML schema docs: `docs/yaml_formats.md` (keep in sync with assets)

### Multi-Space System
The world consists of multiple independent spaces (Overworld, Underworld, ephemeral dungeons). `SpaceManager` resource tracks all spaces; portals connect them. Each space has its own tile grid and object set.

## Coding Conventions

- Rust standard: `PascalCase` types, `snake_case` functions/variables, rustfmt defaults
- Keep files short; split systems into separate files/directories
- Prefer small, focused Bevy systems
- Unit tests go in-module with `#[cfg(test)]`; integration tests in `tests/`
- If adding a crate dependency, update `Cargo.toml` and ask the user to rebuild before continuing

## Key Files

- `ISSUES.md`: Feature backlog and known problems (keep updated)
- `PLAN.md`: Detailed project plan
- `AGENTS.md`: Repository contribution guidelines
- `docs/yaml_formats.md`: YAML schema reference
- `docs/sprite_style.md`: Sprite perspective/art conventions (projection, height, anchoring). Source of truth for all `scripts/gen_*.py` art and the gen-sprite skill.
- `docs/progression.md`: Player progression design (D&D 3.5e-flavored: classes, XP/levels, skills, mana scaling, death penalty). Source of truth for `PLAN.md` Phase 6.
- `common_issues.md`: Recurring bugs and root causes (read before debugging rendering/NPC issues)
