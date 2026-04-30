# Issues and Ideas

Short bug/risk/idea log. Bigger system work belongs in `FEATURE_BACKLOG.md`;
the project's broad direction lives in `PLAN.md`.

## Active

### Auth & operations
- Account admin tool (`mud2-admin` binary) for password reset, account ban, account deletion.
- Multiple characters per account — schema migration (reintroduce a `characters` table) plus `ListCharacters` / `SelectCharacter` / `CreateCharacter` protocol variants and a character-select screen.
- Rate limiting on auth attempts (currently unbounded — an open server is susceptible to brute force).
- Username / password validation policy beyond the v1 minimums (length, allowed chars, leaked-password check).
- When `--tls` is off and the server binds to a non-loopback address, emit a startup warning so operators don't accidentally run cleartext over the public internet.

### TLS hardening
- `--ca-cert PATH` client TLS trust anchor so self-signed dev certs can be verified without `--insecure`.
- TOFU (trust-on-first-use) fingerprint pinning for client TLS; store fingerprints in `~/.local/share/mud2/known_hosts`.

### Architecture / cleanup
- Finish migrating remaining presentation systems to consume replicated/view state instead of directly reading authoritative ECS/resources.
- Decide when to delete the now-obsolete direct-mutation helpers still left in `ui::systems`.
- Add dedicated ECS query helpers/system params for common same-space access patterns so AI and interaction systems stop hand-filtering residents.

### Multi-floor (z-level) follow-up
The PoC in `docs/stacked_floors_plan.md` Phase 0/1 is shipped (z in `TilePosition`,
roof hiding, stairs). Subsequent commits pivoted to *floor-type tiling*
(grass/dirt/stone transitions, tile variants) — so Phase 2/3 of stacked floors
is paused. See that doc for the open items if/when we resume:
- Editor floor selector (PgUp/PgDn) + per-floor dimming.
- `FloorIndicatorLabel` HUD text.
- Ladder / rope / hole transition object kinds.
- `docs/yaml_formats.md` documentation for `z`, `floor_transition`, `occludes_floor_above`, `walkable_surface`.

### Map authoring
- Decide how authored maps should support ranges/rectangles/brushes so large layouts are not verbose YAML tile lists.
- Add validation for map YAML so invalid object IDs or out-of-bounds placements fail clearly.
- Decide how decorative objects (flowers, etc.) should share tiles with blocking objects through explicit layering rules.
- Decide how stacked map objects render visually once trees, items, and walls can share a tile.

### Gameplay polish
- Introduce richer collision semantics than a single blocking flag.
- Generalize the new NPC behavior system so mobs/NPCs can share the same behavior component layer.
- Decide how much scripting authority the embedded Python console should keep once server-authoritative logic exists.

## Risks

- Persistence-heavy gameplay will require durable IDs to keep working as item/container counts grow (mostly addressed by the format-v7 multi-space dump, but new persistent systems must not regress this).
- AoI / interest management is not implemented — `compute_events_for_peer` broadcasts everything. Player count above ~5 will saturate bandwidth. Track in `FEATURE_BACKLOG.md`.

## Completed

- Bootstrapped the Bevy project structure and initial app/world/player plugin layout.
- Added a simple colored tile grid and a player marker with explicit tile coordinates.
- Implemented one-tile movement with map-bounds clamping and Tibia-style centered-player scrolling.
- Added starter map features (water patches, tree clusters), a data-driven overworld object format, and ECS collider components.
- Expanded the overworld object catalog (grass, walls, barrels, flowers, stones) with metadata-driven collision.
- Moved default map layout into YAML; placement no longer hardcoded in Rust.
- Added an embedded Python console (RustPython) with world listing and object spawning.
- Added data-driven equippable gear definitions with typed equipment slots.
- Added basic player stats with equipment-driven health, mana, and storage bonuses.
- Added metadata-driven usable consumables with context-menu use actions.
- Added instance-authored roaming NPC behavior with bounded random movement.
- Added a first combat loop with per-character targets, global battle tick, and melee hit log.
- Added a first attribute system (strength/agility/constitution/willpower/charisma/focus) driving derived health, mana, and carrying capacity.
- Added first-pass melee damage so combat turns reduce hit points and can defeat the player.
- Added a hostile roam-and-chase NPC behavior and a first goblin encounter.
- Added first-pass scroll-cast magic with YAML spell defs, untargeted/self-cast and targeted modes, and a spell-target cursor.
- Generalized the right sidebar into docked windows; status, equipment, backpack, target, container panels share the same scrollable/resizable dock.
- Introduced a server-authoritative command layer; gameplay mutations for movement, targeting, item actions, spell casting, drag/drop, and console spawns go through `PendingGameCommands`.
- Allowed right-click context interactions and combat targeting against nearby remote players.
- Made players block movement and occupied-tile placement for other players via the authoritative collider path.
- Added server-side world-state dumping on graceful exit (`Ctrl+C` handling, JSON save) for authoritative players, objects, and runtime registry state.
- Added authored multi-space support with `persistent`/`ephemeral` space definitions, portal travel, shared dungeon instancing per entrance, and same-space snapshot filtering.
- Added a persistent underworld space with cave assets and a two-way overworld portal.
- Added a title screen with splash art, server selection, author credits, connect flow, exit action.
- Made embedded play load and save the same world snapshot path as headless server mode; fixed local combat HP desync from client projection writing over authoritative state.
- **Account-level persistence**: sqlite DB at `~/.local/share/mud2/accounts.db`, Argon2 password hashing, Login/Register protocol, per-character save on disconnect/autosave/exit. Embedded mode uses reserved `account_id = 0`. World snapshot v5 + later — players no longer ride in `WorldStateDump`.
- **TLS** via `rustls` (sync nonblocking, no tokio). Server: `--tls --tls-cert --tls-key`, `--generate-cert` with `dev-self-signed` for self-signed dev pairs. Client: `--tls` (webpki-roots) or `--insecure`, plus `tls://host:port` URL shorthand.
- **Periodic autosave**: `autosave_all_players` runs every 60s in addition to disconnect/exit saves.
- **Multi-space persistence**: world snapshot is now a `Vec<RuntimeSpaceDump>`; format_version bumped to 7.
- **Stacked floors PoC** (Phase 0/1 only): `TilePosition.z`, `FLOOR_Z_STEP`, roof hiding, stair transitions, floor-aware minimap, two-floor authored building. Phase 2/3 (editor selector, ladder/rope/hole, schemas) paused — see Active section.
- **Dialog system**: yarnspinner-driven NPC dialog with `DialogPanel*` UI, dedicated `dialog_node` field on object definitions, and a first authored villager dialog (`assets/dialogs/demo_villager.yarn`).
- **Quest engine**: per-player persistent quest state with Python and Yarn quest scripting (`src/quest/`, `assets/quests/hunter.py`).
- **Ranged combat**: bow / crossbow / arrow / bolt assets, ranged attack profile in object definitions, archer goblin enemy, kiting AI.
- **Minimap** with floor-aware tile/object filtering.
- **Directional movement** + object rotation by player.
- **Map editor** with placement, modal property editing, undo, and YAML serialization (`src/editor/`).
- **Floor-type tiling**: grass/dirt/stone tilesets with corner-aware transitions and tile variants (`src/world/floor_render.rs`, `assets/floors/`); tileset pack/unpack helper script (`scripts/tile_permutor.py`).
- **In-process command pipeline / transport abstraction**: `ServerTransport`/`ClientTransport` wrap raw TCP and TLS streams; embedded mode runs `GameServerPlugin` and `GameClientPlugin` in the same `App` so the wire protocol is bypassed but data flow is identical to networked mode.
- **Decision: stay single-crate.** Networking shipped without splitting `shared/`; module boundaries inside `src/` are sufficient. Revisit only if a real second binary needs a fragment of the code.

## Later Ideas

- Chunk-based world streaming and AoI-based replication.
- Persistent dropped items and containers (today: containers persist; dropped items handled via world-object loot, but ground-item decay timers are not implemented).
- Debug/admin tools for spawning and inspecting entities (today: only Python `world.spawn_object`).
