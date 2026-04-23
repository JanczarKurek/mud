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
```

## Architecture

### Three Runtime Modes (configured in `src/app/plugin.rs`)
- **EmbeddedClient**: Single binary, shared memory client+server (default, for dev)
- **TcpClient**: Connects to remote server over TCP
- **HeadlessServer**: No graphics, listens for TCP connections

### Server-Authoritative Flow
1. Client sends **commands** via `PendingGameCommands` (move, cast, etc.)
2. Server validates and processes commands (`src/game/systems.rs`)
3. Server produces **game events** via `PendingGameEvents`
4. Client applies events to local state via `ClientGameState`

### The EmbeddedClient Invariant
EmbeddedClient mode = HeadlessServer + TcpClient running in the same `App`. The wire protocol is *bypassed* but the data flow must be identical, otherwise offline play will drift from networked play. Keep these rules when adding systems:
- **Server-side systems** must emit changes through `PendingGameEvents`. Never mutate `ClientGameState` directly.
- **Client-side (presentation) systems** must read from `ClientGameState` or from view-only components (`DisplayedVitalStats`, `ViewPosition`). Never query authoritative components (`VitalStats`, `SpaceResident`, `TilePosition`) from presentation code. Projected entities (`ClientProjectedWorldObject`, `ClientRemotePlayerVisual`, the projected local player in TcpClient mode) carry *only* `ViewPosition`, never the authoritative pair.
- `apply_game_events_to_client_state` is the single fold function that turns events into client state. Both `GameServerPlugin` and `GameClientPlugin` register it so system-graph ordering is identical in all three runtime modes (`src/game/mod.rs`).
- **Two event channels, two roles.** `GameEvent` (via `ServerMessage::Events`) is state replication — every field of `ClientGameState` is reachable through a `GameEvent` variant, and `compute_events_for_peer` is the sole serializer. `GameUiEvent` (via `ServerMessage::UiEvents`) is a one-shot signal bus orthogonal to state (e.g. "open this container now"); do not use it to replicate state.
- Before adding a new code path, ask: "would this still work if the server were on another machine?" If no, it belongs on the presentation side.

### Module Layout (`src/`)
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

### Auth & Persistence

- Every TCP connection must `Login` / `Register` before the server will send the asset manifest or any gameplay events. The peer state machine is `AwaitingAuth → Authed { account_id }` (`src/network/resources.rs`).
- `PlayerId(account_id as u64)` — the auth path sets a player's identity from their DB row, and embedded mode uses the reserved `LOCAL_ACCOUNT_ID = 0` (`src/accounts/db.rs`).
- On-disk layout is per-role (see `src/app/paths.rs` — the single source of truth):

  | Role | Accounts DB | World snapshot | Asset cache |
  |---|---|---|---|
  | EmbeddedClient | `~/.local/share/mud2/embedded/accounts.db` | `~/.local/share/mud2/embedded/saves/world-state.json` | — |
  | HeadlessServer | `~/.local/share/mud2/server/accounts.db` | `~/.local/share/mud2/server/saves/world-state.json` | — |
  | TcpClient | — | — | `~/.cache/mud2/client/assets/` |

  Overrides: `--db-path` / `MUD2_DB_PATH`, `--save-path` / `MUD2_SAVE_PATH`, `--asset-cache` / `MUD2_ASSET_CACHE`. Run `mud2 paths` to print resolved locations; `mud2 clean-cache` wipes the client cache (`--all --yes` also wipes data).
- Per-character saves happen on disconnect (`PendingPlayerSaves` queue drained by `persist_disconnected_players` in the `Last` schedule), every 60s via `autosave_all_players`, and on `AppExit`.
- `WorldStateDump` **does not carry player data** (as of `format_version = 5`). If you need to save anything about a player, route it through the accounts DB.

### TLS

- `ServerTransport` / `ClientTransport` (`src/network/transport.rs`) wrap the raw `TcpStream` with optional TLS via `rustls::StreamOwned`. Sync nonblocking throughout — no tokio.
- Server: `--tls --tls-cert PATH --tls-key PATH`, plus `--generate-cert` (requires `dev-self-signed` Cargo feature) to emit a self-signed pair.
- Client: `--tls` uses `webpki-roots` trust anchors; `--insecure` skips verification (dev only). `--connect tls://host:port` is shorthand for both.

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
- `common_issues.md`: Recurring bugs and root causes (read before debugging rendering/NPC issues)
