# Issues and Ideas

## Active

- Decide whether to introduce a dedicated `shared/` crate before networking or only after the local prototype.
- Replace placeholder colored terrain with proper art/assets later.
- Decide how we want to represent stacked map objects visually once trees, items, and walls can share space.
- Expand collision from water-only blocking into a broader rule set for trees, walls, doors, and future objects.
- Decide whether decorative objects like flowers should share tiles with blocking objects through explicit layering rules.
- Decide how authored maps should support ranges/rectangles/brushes so large layouts are not verbose YAML tile lists.

## Risks

- Bevy version and ecosystem choices made too early can slow iteration if we over-invest in rendering/map tooling before movement and interaction are proven.
- Persistence-heavy gameplay will require durable IDs early; this should be introduced before item/container logic grows.

## Near-Term Next Features

- Start referencing real sprite assets from metadata.
- Introduce richer collision semantics than a single blocking flag.
- Add validation for map YAML so invalid object IDs or out-of-bounds placements fail clearly.

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

## Later Ideas

- Chunk-based world streaming.
- Persistent dropped items and containers.
- Server-authoritative movement and interaction.
- Debug/admin tools for spawning and inspecting entities.
