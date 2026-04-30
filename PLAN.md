# Project Plan

## 1. Project Goal

Build a Tibia-inspired open-world, grid-based, 2D MMO in Rust using Bevy.

Core target:
- Top-down tile-based movement and interaction.
- Large persistent overworld.
- Multiplayer with server authority.
- Persistent world objects: dropped items, containers, doors, resource nodes, decorations, and player impact on the world.
- Strong separation between client, shared game rules, and backend/server logic so the project can scale beyond a prototype.

Non-goals for the first playable milestones:
- Fully open PvP with advanced guild politics.
- Complex quest editor and NPC scripting tools (we already have minimal quest scripting; "complex" tooling is the non-goal).
- Huge content volume.
- Perfect MMO scalability from day one.

Optimization sequence (originally proposed; the first three are done):
1. Prove the core gameplay loop locally. ✅
2. Prove networking with authoritative movement and persistence. ✅
3. Expand into a persistent shared world. *In progress* — multi-space + accounts + TLS shipped; AoI / character roster / reconnection still open.

## 2. Design Pillars

### 2.1 Gameplay
- Grid-based movement with precise tile occupancy rules.
- Readable 2D world simulation over flashy presentation.
- Meaningful persistence: world changes should matter and survive restarts where intended.
- Slow, deliberate RPG-style progression and exploration.
- Strong systemic interactions between map, creatures, items, and players.

### 2.2 Technical
- Mostly deterministic server-side simulation where practical.
- Clear ECS boundaries for world state, combat, AI, and networking.
- Data-driven content definitions where possible.
- Save/load designed early, not bolted on late.
- Modular code layout with small files and focused systems.

### 2.3 Product
- Reach a playable vertical slice quickly.
- Keep local development simple.
- Avoid premature backend complexity until the core loop is fun.
- Preserve the ability to split into multiple crates later (decision so far: single crate is enough).

## 3. Current Architecture (snapshot)

The detailed module layout, runtime modes, server-authoritative flow, and the
EmbeddedClient invariant are documented in `CLAUDE.md` (the source of truth for
contributors and agents). Highlights only here:

- Single-binary Bevy 0.18 app with three runtime modes (`EmbeddedClient`, `TcpClient`, `HeadlessServer`) — `src/app/plugin.rs`.
- Server-authoritative command/event loop (`PendingGameCommands` → server validation → `PendingGameEvents` → `apply_game_events_to_client_state`).
- TLS-capable transport via `rustls` (sync nonblocking).
- Account-level persistence (sqlite + Argon2) separate from the world snapshot (JSON, format_version 7, multi-space).
- Multi-space world (overworld / underworld / ephemeral dungeons) with portal travel.
- Yarnspinner dialog engine, Python-scripted quest engine, embedded RustPython dev console.
- Map editor (`src/editor/`), floor-type tiling with corner-aware transitions, minimap, docked right-side panel system.

## 4. Roadmap (what's left)

The original Phases 0–4 (single-player loop, data hardening, persistence,
networking prototype) are shipped. Surviving phases below.

### 4.1 Phase 5 — Persistent Shared World

In progress. Account login, multi-space persistence, and the world-state dump
are done. Still open:

- **Interest management (AoI)**: today `compute_events_for_peer` broadcasts
  every event regardless of distance. This is the largest scaling blocker.
- **Reconnection** with session tokens and a grace window.
- **Multiple characters per account** and a character-select flow.
- **Audit-friendly logs** for critical state transitions (item moves, deaths,
  account ops).
- **Anti-duplication safeguards** for item transfer paths.

### 4.2 Phase 6 — RPG Systems Expansion

The combat/magic/equipment systems exist but are flat. See
`FEATURE_BACKLOG.md` §1 (Progression, Combat depth, Magic expansion) for the
candidate list. The big-ticket items — XP/levels, skill levels, vocations,
death penalty, status effects, armor, ranged-vs-melee balance — depend on
`src/combat/systems.rs` and a stable progression model.

### 4.3 Phase 7 — Content Pipeline & Tooling

Map editor and YAML content pipeline are shipped. Open items:

- Map authoring brushes / ranges / templates.
- YAML schema validation surfaced as friendly editor errors.
- Hot reload for object metadata and spells.
- Data-driven NPC behavior templates (today behaviors are code-only).
- Editor floor selector + multi-floor authoring (paused work — see
  `docs/stacked_floors_plan.md`).
- Admin commands (`/teleport`, `/spawn`, `/noclip`).

### 4.4 Phase 8 — Production Hardening

Mostly TODO:

- Atomic / crash-safe save writes (today: single JSON, partial writes can corrupt).
- Explicit save-format migration framework.
- Profiling for chunk loading, pathfinding hotspots, replication bandwidth.
- Structured logging output (`game.log`) instead of `bevy::log` only.
- Server admin observability.
- Exploit review: duplication, desync, invalid move injection, stale interaction requests.
- Real CI (`.github/workflows/`) and broader test coverage.

## 5. Risks and Mitigations

### Scope risk
MMO scope is large. Mitigation: keep a narrow vertical slice (we have one),
defer advanced social systems, ship persistence before scale (mostly done).

### Architecture risk
Mixing client presentation and server simulation causes rewrites. Mitigation:
the EmbeddedClient invariant in `CLAUDE.md` enforces this; keep new systems on
the right side of it.

### Persistence risk
Persistent overworld objects and durable IDs are central to project identity.
Mitigation: format_version is honored, multi-space dump is in place, but
crash-safe writes and a real migration path are still open.

### Content risk
Even a good engine feels empty without content volume. Mitigation: the dialog
+ quest engines already exist; the next big content unlock is vendors + quest
log UI + NPC spawners (see `FEATURE_BACKLOG.md` "Living-world batch").

## 6. Living-Document Rules

Update this file when:
- scope changes,
- a phase completes or stalls,
- a major architecture decision is made,
- a risk becomes clearer.

Companion docs:
- `CLAUDE.md` — runtime architecture, conventions, invariants (source of truth for *how* the code is structured).
- `ISSUES.md` — bugs, near-term todos, recently-completed log.
- `FEATURE_BACKLOG.md` — system-sized feature gaps and suggested batches.
- `common_issues.md` — past bug root causes, useful before debugging similar symptoms.
- `docs/yaml_formats.md` — YAML schema reference for assets.
- `docs/stacked_floors_plan.md` — paused multi-z-level work.
