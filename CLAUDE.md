# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Mud 2.0 is a Tibia-inspired multiplayer MUD built with **Bevy 0.18** (Rust game engine) using ECS architecture. It supports embedded single-player, TCP multiplayer with a headless server, and features grid-based movement, equipment, combat, magic, NPC AI, persistent world saves, and an embedded Python scripting console.

## Build & Run Commands

```bash
cargo run --bin mud2                            # Run game (embedded client+server)
cargo run --bin server                          # Headless TCP server
cargo run --bin mud2 -- --connect 127.0.0.1:7000  # Connect to remote server
cargo check                                     # Always run after changes before reporting success
cargo test                                      # Run tests
cargo fmt                                       # Format code
cargo clippy                                    # Lint (fix warnings before merging)
packaging/build-appimage.sh                     # Linux AppImage (run inside nix-shell)
packaging/build-windows.sh                      # Windows x86_64 zip via mingw cross-compile (run inside nix-shell)
```

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
1. Client sends **commands** via `PendingGameCommands` (move, cast, etc.)
2. Server validates and processes commands (`crates/mud2-lib/src/game/systems.rs`)
3. Server produces **game events** via `PendingGameEvents`
4. Client applies events to local state via `ClientGameState`

### The EmbeddedClient Invariant
EmbeddedClient mode = HeadlessServer + TcpClient running in the same `App`. The wire protocol is *bypassed* but the data flow must be identical, otherwise offline play will drift from networked play. Keep these rules when adding systems:
- **Server-side systems** must emit changes through `PendingGameEvents`. Never mutate `ClientGameState` directly.
- **Client-side (presentation) systems** must read from `ClientGameState` or from view-only components (`DisplayedVitalStats`, `ViewPosition`). Never query authoritative components (`VitalStats`, `SpaceResident`, `TilePosition`) from presentation code. Projected entities (`ClientProjectedWorldObject`, `ClientRemotePlayerVisual`, the projected local player in TcpClient mode) carry *only* `ViewPosition`, never the authoritative pair.
- `apply_game_events_to_client_state` is the single fold function that turns events into client state. Both `GameServerPlugin` and `GameClientPlugin` register it so system-graph ordering is identical in all three runtime modes (`crates/mud2-lib/src/game/mod.rs`).
- **Two event channels, two roles.** `GameEvent` (via `ServerMessage::Events`) is state replication — every field of `ClientGameState` is reachable through a `GameEvent` variant, and `compute_events_for_peer` is the sole serializer. `GameUiEvent` (via `ServerMessage::UiEvents`) is a one-shot signal bus orthogonal to state (e.g. "open this container now"); do not use it to replicate state.
- Before adding a new code path, ask: "would this still work if the server were on another machine?" If no, it belongs on the presentation side.

### Module Layout (`crates/mud2-lib/src/`)
- **accounts/**: sqlite-backed account database (Argon2 hashed passwords), per-character save/load, autosave system
- **app/**: Bevy app setup, plugins, state machine, title screen, auth screen
- **game/**: Core command/event loop (commands.rs, resources.rs, systems.rs)
- **world/**: Map spaces, tiles, objects, object registry, collision
- **player/**: Player components (stats, inventory, chat), input handling
- **combat/**: Battle system, damage resolution, attack profiles
- **magic/**: Spell definitions loaded from YAML
- **npc/**: NPC AI (roaming, hostile chase behavior)
- **network/**: TCP protocol, connection management, message ser/de, TLS transport wrapper
- **persistence/**: World snapshot save/load (JSON format; players live in `accounts.db`, not this snapshot)
- **ui/**: HUD, docked panels, context menus, cursor management
- **scripting/**: Embedded RustPython console

(Editor and viewers live in `crates/mud2-editor/src/`.)

### Auth & Persistence

- Every TCP connection must `Login` / `Register` before the server will send the asset manifest or any gameplay events. The peer state machine is `AwaitingAuth → Authed { account_id }` (`crates/mud2-lib/src/network/resources.rs`).
- `PlayerId(account_id as u64)` — the auth path sets a player's identity from their DB row, and embedded mode uses the reserved `LOCAL_ACCOUNT_ID = 0` (`crates/mud2-lib/src/accounts/db.rs`).
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

- HeadlessServer only. Pass `--admin-socket [PATH]` to bind a UNIX-domain socket; default path is `~/.local/share/mud2/server/admin.sock`. Auth is by filesystem permissions (default mode `0600`, override with `--admin-socket-mode 660`). Connect with `nc -U <path>` or `socat - UNIX-CONNECT:<path>`.
- One persistent Python scope is shared across all admin connections — admins can collaborate on globals. Live-bind a session to act-as a player with `world.attach_player(player_id)` (`world.attach_player(None)` detaches).
- `AdminReplHost` (`crates/mud2-lib/src/scripting/admin_host.rs`) compiles input as `Mode::Single` and pipes `sys.stdout` / `sys.stderr` / `sys.displayhook` through `world.log`, so bare expressions print their `repr` like CPython's REPL. Multi-line input is buffered until a blank line force-flushes (mirrors CPython).
- The listener (`crates/mud2-lib/src/network/admin.rs`, `#[cfg(unix)]`) reuses the existing sync-nonblocking pattern from `crates/mud2-lib/src/network/systems.rs`. All Python execution happens on the Bevy main thread; no background threads. Stale sockets from a prior crash are auto-reclaimed at startup if no live listener answers on them.

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
