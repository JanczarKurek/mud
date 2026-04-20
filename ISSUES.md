# Issues and Ideas

## Active

- Account admin tool (`mud2-admin` binary) for password reset, account ban, account deletion.
- Add `--ca-cert PATH` client TLS trust anchor so self-signed dev certs can be verified without `--insecure`.
- TOFU (trust-on-first-use) fingerprint pinning for client TLS; store fingerprints in `~/.local/share/mud2/known_hosts`.
- Multiple characters per account — schema migration (reintroduce a `characters` table) plus `ListCharacters` / `SelectCharacter` / `CreateCharacter` protocol variants and a character-select screen.
- Rate limiting on auth attempts (currently unbounded — an open server is susceptible to brute force).
- Username / password validation policy beyond the v1 minimums (length, allowed chars, leaked-password check).
- When `--tls` is off and the server binds to a non-loopback address, emit a startup warning so operators don't accidentally run cleartext over the public internet.
- Decide whether to introduce a dedicated `shared/` crate before networking or only after the local prototype.
- Finish migrating remaining presentation systems to consume replicated/view state instead of directly reading authoritative ECS/resources.
- Decide when to delete the now-obsolete direct-mutation helpers still left in `ui::systems`.
- Replace placeholder colored terrain with proper art/assets later.
- Decide how we want to represent stacked map objects visually once trees, items, and walls can share space.
- Decide whether decorative objects like flowers should share tiles with blocking objects through explicit layering rules.
- Decide how authored maps should support ranges/rectangles/brushes so large layouts are not verbose YAML tile lists.
- Finish migrating persistence/save data to serialize multiple runtime spaces instead of the old single-map dump format.
- Add dedicated ECS query helpers/system params for common same-space access patterns so AI and interaction systems stop hand-filtering residents.

## Risks

- Bevy version and ecosystem choices made too early can slow iteration if we over-invest in rendering/map tooling before movement and interaction are proven.
- Persistence-heavy gameplay will require durable IDs early; this should be introduced before item/container logic grows.

## Near-Term Next Features

- Introduce richer collision semantics than a single blocking flag.
- Add validation for map YAML so invalid object IDs or out-of-bounds placements fail clearly.
- Decide how much scripting authority the embedded Python console should keep once server-authoritative logic exists.
- Generalize the new NPC behavior system so mobs/NPCs can share the same behavior component layer.
- Add a transport abstraction on top of the new in-process command pipeline so embedded loopback networking can become the default.

## Completed

- Bootstrapped the Bevy project structure.
- Added the initial app, world, and player plugin layout.
- Added a simple colored tile grid.
- Spawned a player marker with explicit tile coordinates.
- Implemented one-tile movement with map bounds clamping.
- Switched to Tibia-style centered-player scrolling, where world tiles move relative to the player position.
- Added starter map features with water patches and tree clusters.
- Added a data-driven overworld object definition format in per-object asset directories.
- Added ECS-based collider components and used them to make water block player movement.
- Expanded the overworld object catalog with grass, walls, barrels, flowers, and stones, with collision driven by metadata.
- Moved the default map layout into YAML so object placement is no longer hardcoded in Rust.
- Added an embedded Python console with world listing and object spawning commands exposed in-game.
- Fixed shutdown instability by avoiding embedded Python VM teardown on app exit.
- Added data-driven equippable gear definitions with typed equipment slots for future paperdoll logic.
- Added basic player stats with equipment-driven health, mana, and storage bonuses.
- Added metadata-driven usable consumables with context-menu use actions and randomized use text.
- Added instance-authored roaming NPC behavior with bounded random movement.
- Added a first combat loop with per-character targets, a global battle tick, and melee hit log messages.
- Added a first attribute system with strength, agility, constitution, willpower, charisma, and focus driving derived health, mana, and carrying capacity.
- Added first-pass melee damage so combat turns reduce hit points, kill NPCs, and can defeat the player.
- Added a hostile roam-and-chase NPC behavior and a first goblin encounter.
- Added first-pass scroll-cast magic with YAML-defined spells, untargeted/self-cast and targeted spell modes, and a dedicated spell-target cursor.
- Generalized the right sidebar into docked windows so status, equipment, backpack, target, and container panels share the same scrollable/resizable dock system, with title-bar reordering for movable panel order.
- Introduced a first server-authoritative command layer inside the single-player app, moving gameplay mutations for movement, targeting, item actions, spell casting, drag/drop, and console spawns behind a central game-processing plugin.
- Allowed right-click context interactions and combat targeting against nearby remote players.
- Made players block movement and occupied-tile placement for other players through the authoritative collider path.
- Added server-side world-state dumping on graceful exit, including `Ctrl+C` handling and JSON save output for authoritative players, objects, and runtime registry state.
- Added first-pass authored multi-space support with `persistent`/`ephemeral` space definitions, portal travel, shared runtime dungeon instancing per entrance, and same-space snapshot filtering for clients.
- Added a persistent underworld space with dedicated cave assets and a two-way portal connection from the overworld.
- Added a first title screen with splash art, server selection, author credits, connect flow, and exit action for client builds.
- Made embedded/client-only play load and save the same world snapshot path as headless server mode, fixed local combat HP desync caused by client projection writing over authoritative player state, and added first-pass logging for client state changes plus snapshot/YAML loads.
- Added account-level persistence: sqlite DB at `~/.local/share/mud2/accounts.db`, Argon2 password hashing, Login/Register protocol with per-character save on disconnect/autosave/exit. Embedded mode uses the reserved `account_id = 0` local account. World snapshot format bumped to v5; players no longer ride in `WorldStateDump`.
- Added TLS support via `rustls` (sync nonblocking; no tokio). Server: `--tls --tls-cert --tls-key`, `--generate-cert` with the `dev-self-signed` feature for self-signed dev pairs. Client: `--tls` (webpki-roots) or `--insecure`, plus `tls://host:port` URL shorthand.

## Later Ideas

- Chunk-based world streaming.
- Persistent dropped items and containers.
- Real transport/networking on top of the in-process authoritative command layer.
- Debug/admin tools for spawning and inspecting entities.
