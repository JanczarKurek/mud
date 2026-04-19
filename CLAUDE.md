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

### Module Layout (`src/`)
- **app/**: Bevy app setup, plugins, state machine, title screen
- **game/**: Core command/event loop (commands.rs, resources.rs, systems.rs)
- **world/**: Map spaces, tiles, objects, object registry, collision
- **player/**: Player components (stats, inventory, chat), input handling
- **combat/**: Battle system, damage resolution, attack profiles
- **magic/**: Spell definitions loaded from YAML
- **npc/**: NPC AI (roaming, hostile chase behavior)
- **network/**: TCP protocol, connection management, message ser/de
- **persistence/**: World snapshot save/load (JSON format)
- **ui/**: HUD, docked panels, context menus, cursor management
- **scripting/**: Embedded RustPython console

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
