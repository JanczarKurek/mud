# Issues and Ideas

## Active

- Decide whether to introduce a dedicated `shared/` crate before networking or only after the local prototype.
- Finish migrating remaining presentation systems to consume replicated/view state instead of directly reading authoritative ECS/resources.
- Decide when to delete the now-obsolete direct-mutation helpers still left in `ui::systems`.
- Replace placeholder colored terrain with proper art/assets later.
- Decide how we want to represent stacked map objects visually once trees, items, and walls can share space.
- Decide whether decorative objects like flowers should share tiles with blocking objects through explicit layering rules.
- Decide how authored maps should support ranges/rectangles/brushes so large layouts are not verbose YAML tile lists.

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

## Later Ideas

- Chunk-based world streaming.
- Persistent dropped items and containers.
- Real transport/networking on top of the in-process authoritative command layer.
- Debug/admin tools for spawning and inspecting entities.
