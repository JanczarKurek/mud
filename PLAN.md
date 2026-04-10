# Project Plan

## 1. Project Goal

Build a Tibia-inspired open-world, grid-based, 2D MMO in Rust using Bevy.

Core target:
- Top-down tile-based movement and interaction.
- Large persistent overworld.
- Multiplayer with server authority.
- Persistent world objects such as dropped items, containers, doors, resource nodes, decorations, and player impact on the world.
- Strong separation between client, shared game rules, and backend/server logic so the project can scale beyond a prototype.

Non-goals for the first playable milestones:
- Fully open PvP with advanced guild politics.
- Complex quest editor and NPC scripting tools.
- Huge content volume.
- Perfect MMO scalability from day one.

The early plan should optimize for this sequence:
1. Prove the core gameplay loop locally.
2. Prove networking with authoritative movement and persistence.
3. Expand into a persistent shared world.

## 2. Design Pillars

### 2.1 Gameplay Pillars
- Grid-based movement with precise tile occupancy rules.
- Readable 2D world simulation over flashy presentation.
- Meaningful persistence: world changes should matter and survive restarts where intended.
- Slow, deliberate RPG-style progression and exploration.
- Strong systemic interactions between map, creatures, items, and players.

### 2.2 Technical Pillars
- Deterministic or mostly deterministic server-side simulation where practical.
- Clear ECS boundaries for world state, combat state, AI state, and networking state.
- Data-driven content definitions where possible.
- Save/load support designed early, not bolted on late.
- Modular code layout with small files and focused systems.

### 2.3 Product Pillars
- Reach a playable vertical slice quickly.
- Keep local development simple.
- Avoid premature backend complexity until the core loop is fun.
- Preserve the ability to split into multiple crates later.

## 3. High-Level Architecture Direction

### 3.1 Recommended Project Structure
- `src/main.rs`
  - Thin entry point.
- `src/app/`
  - Bevy app setup, plugins, states, schedules.
- `src/game/`
  - Core gameplay systems and components.
- `src/world/`
  - Map, tiles, chunks, spatial queries, persistence-facing structures.
- `src/player/`
  - Input, movement intent, player state, inventory, stats.
- `src/combat/`
  - Damage, health, targeting, attacks, status effects.
- `src/creatures/`
  - NPCs, monsters, basic AI, spawners.
- `src/items/`
  - Item definitions, containers, drops, loot, equipment.
- `src/net/`
  - Client/server networking abstractions and replication.
- `src/persistence/`
  - Save/load, snapshots, durable object state.
- `src/ui/`
  - HUD, debug panels, inventory UI, chat UI.
- `src/data/`
  - Loaders for static content definitions.
- `assets/`
  - Sprites, tilemaps, fonts, sounds, data files.
- `tests/`
  - Integration tests when the codebase is large enough.

### 3.2 Longer-Term Crate Split

Once the prototype proves itself, likely split into:
- `client`
- `server`
- `shared`
- `tools` or `content_packer`

Initial recommendation:
- Start as a single Cargo package with strong internal module boundaries.
- Split crates only after gameplay and networking boundaries are validated.

### 3.3 Simulation Model
- Server-authoritative simulation for movement, combat, world state, and persistent objects.
- Client handles rendering, prediction where useful, interpolation, and UI.
- Shared data layer defines IDs, messages, components, and content schemas.

## 4. Core Game Scope

### 4.1 Player Experience Targets
- Login or local character selection.
- Spawn into a persistent tile-based overworld.
- Walk around smoothly on a grid.
- See creatures, players, items, and world objects.
- Pick up, drop, move, and use items.
- Open containers and interact with doors/levers/basic usable objects.
- Fight simple monsters.
- Gain loot and basic progression.

### 4.2 Persistent Overworld Object Scope
- Dropped items on ground.
- Containers with persistent contents.
- Doors with open/closed/locked states.
- Resource nodes with regeneration timers.
- Corpses/loot remains with decay timers.
- Static decorations with optional interaction flags.
- Spawned world props placed by tooling or generated content.

### 4.3 World Model Scope
- Tile grid with walkability, opacity, elevation/floor concept if needed later.
- Chunking for streaming/loading/saving.
- Object occupancy rules per tile.
- Interest management based on nearby chunks or area-of-interest radius.

## 5. Development Phases

## 5.1 Phase 0: Pre-Production and Foundations

Goal:
- Establish the technical baseline, project structure, and decision log before gameplay work expands.

Tasks:
- Initialize Cargo project if not already present.
- Add Bevy and only essential dependencies.
- Create initial module layout and plugin registration structure.
- Add `ISSUES.md` if missing and use it as a lightweight backlog/risk log.
- Add `PLAN.md` and keep it updated when scope changes.
- Set up formatting, linting, and basic CI-ready command expectations.
- Decide early asset pipeline format:
  - raw spritesheets
  - LDtk/Tiled map pipeline
  - custom data files for gameplay definitions
- Decide networking library direction after initial prototype research.

Deliverables:
- Project compiles with empty app.
- Window opens and closes cleanly.
- Module structure is in place.
- Planning docs exist and reflect current direction.

Definition of done:
- `cargo check` passes.
- `cargo fmt` produces no further changes.
- Base app/plugin structure exists.

## 5.2 Phase 1: Core Single-Player Prototype

Goal:
- Prove the main loop locally without networking.

Tasks:
- Render a tile grid map.
- Add camera controls or player-follow camera.
- Create a controllable player entity.
- Implement one-tile movement with collision and occupancy checks.
- Add simple map loading from a static source.
- Add world objects:
  - walls
  - doors
  - containers
  - dropped items
- Implement interaction verbs:
  - move
  - inspect
  - use
  - pickup
  - drop
  - open
- Add basic inventory model.
- Add simple enemy NPC with chase/attack behavior.
- Add HP and damage model.
- Add death and corpse/drop flow.
- Add basic HUD for health, selected target, and inventory visibility.

Deliverables:
- Local playable slice with exploration, item interaction, and simple combat.

Definition of done:
- Player can move around a map.
- World objects block or allow passage correctly.
- Items can exist both in containers and on the ground.
- Simple monsters can kill and be killed.
- Save/load is not required yet, but object state structures should not block it.

## 5.3 Phase 2: Data Model Hardening

Goal:
- Refactor the prototype into durable data structures before adding multiplayer.

Tasks:
- Introduce stable IDs for:
  - entities that must persist
  - items
  - containers
  - world objects
  - characters
  - chunks
- Separate transient ECS entity handles from durable game IDs.
- Introduce content definition files for:
  - item types
  - creature types
  - object types
  - tile types
- Normalize interaction rules into reusable systems.
- Define event flows for:
  - movement intent
  - object interaction
  - combat actions
  - inventory transfers
- Add robust spatial query helpers.
- Add serialization-friendly state structures.

Current status note:
- The project now has an initial in-process authoritative command-processing layer in `src/game/`.
- UI and player input paths submit commands instead of mutating gameplay state directly for movement, targeting, use/use-on, spell casts, drag/drop, and console spawns.
- A later step still needs a true client view/replication layer and transport abstraction before the client/server split is complete.

Deliverables:
- Clear domain model that can support save/load and networking.

Definition of done:
- Core game systems no longer depend on fragile direct entity references where persistence is needed.
- Static content is data-driven for at least one category.

## 5.4 Phase 3: Persistence Layer

Goal:
- Make the local world survive restarts.

Tasks:
- Choose persistence approach:
  - file snapshots first
  - optional database later
- Design save format boundaries:
  - static map data
  - dynamic world object state
  - player state
  - creature spawn state where needed
- Implement world chunk serialization.
- Implement persistent object serialization.
- Implement player inventory/equipment serialization.
- Add loading pipeline for world boot.
- Add save triggers:
  - periodic autosave
  - clean shutdown save
  - targeted dirty-chunk save later
- Add object decay/regeneration timers that serialize correctly.
- Test restart correctness with dropped items and modified containers.

Deliverables:
- Local persistent world prototype.

Definition of done:
- World state restores after restart.
- Dropped items and object state survive reload where intended.
- Save format is versioned or version-ready.

## 5.5 Phase 4: Networking Prototype

Goal:
- Convert the simulation into a multiplayer-capable client/server model.

Tasks:
- Decide on transport and replication approach.
- Split client-only concerns from simulation concerns.
- Create dedicated server executable path or server mode.
- Define network protocol messages for:
  - connect/auth handshake
  - snapshot/state sync
  - movement intent
  - interaction requests
  - combat intents
  - chat
- Implement authoritative server movement.
- Implement client-side presentation updates from server state.
- Add basic interpolation or smoothing for remote entities.
- Add interest management by chunk or radius.
- Add reconnect-safe player identification design.

Deliverables:
- Two or more clients can connect to the same world and see shared movement/object state.

Definition of done:
- Shared world state is synchronized.
- Server owns truth for movement and interactions.
- Persistent objects modified by one player are visible to another.

## 5.6 Phase 5: MMO World Backbone

Goal:
- Make the world structure viable for larger shared play.

Tasks:
- Introduce chunk streaming and chunk activation rules.
- Load only nearby world sections on client.
- Simulate only relevant regions on server where appropriate.
- Add spawn systems for creatures/resources.
- Add respawn and regeneration systems.
- Add chat channels:
  - local
  - global or system
  - private later
- Add account/character persistence boundaries.
- Add anti-duplication safeguards for item transfer logic.
- Add audit-friendly logs for critical state transitions.

Deliverables:
- Persistent shared overworld with basic scalability patterns.

Definition of done:
- World state remains coherent as multiple players interact in nearby areas.
- Chunks can be loaded, saved, and reactivated without corrupting state.

## 5.7 Phase 6: RPG Systems Expansion

Goal:
- Turn the technical prototype into an actual game loop.

Tasks:
- Add stats system:
  - health
  - mana if desired
  - speed
  - melee/ranged/magic skill placeholders
- Add equipment slots and stat modifiers.
- Add loot tables.
- Add NPC vendors or basic service NPCs.
- Add progression:
  - experience
  - levels
  - skill advancement
- Add spells or special abilities if within target fantasy.
- Add creature archetypes and regional distribution.
- Add simple quests only after the systemic base is stable.

Deliverables:
- Repeatable progression loop with exploration and combat rewards.

Definition of done:
- Player progression changes gameplay in noticeable ways.
- Combat and loot support repeated play sessions.

## 5.8 Phase 7: Content Pipeline and Tooling

Goal:
- Make world building and content iteration practical.

Tasks:
- Decide and integrate map editor workflow.
- Add import pipeline for tiles, object placements, and regions.
- Add validation tools for map/content consistency.
- Add developer debug tools:
  - spawn item
  - teleport
  - inspect tile
  - inspect entity
  - save/load diagnostics
- Add admin commands for live debugging.
- Add content conventions for IDs, assets, and regions.

Deliverables:
- Team can build and iterate on world/content without hand-editing code for everything.

Definition of done:
- At least one real zone can be authored mostly via data/tools.

## 5.9 Phase 8: Production Hardening

Goal:
- Reduce risk before larger content and player count growth.

Tasks:
- Add integration tests for critical systems.
- Add serialization migration tests.
- Add soak tests for object persistence.
- Add profiling for:
  - chunk loading
  - pathfinding hot spots
  - replication bandwidth
- Improve error handling and logging.
- Add server admin observability.
- Review exploits:
  - duplication
  - desync
  - invalid move injection
  - stale interaction requests

Deliverables:
- Stable foundation suitable for ongoing content growth and external testing.

Definition of done:
- Core regressions are covered.
- Operational debugging is practical.

## 6. Detailed System Breakdown

## 6.1 World and Map System

Subtasks:
- Define tile schema.
- Define chunk schema.
- Support tile flags:
  - walkable
  - opaque
  - swimmable if needed later
  - interaction anchor
- Decide whether multiple objects can stack on a tile.
- Define floor/level support now, even if only one z-level is initially used.
- Add region IDs for biome/content ownership.
- Add helper APIs for neighboring tiles and area queries.

Risks:
- If the map format is too ECS-coupled, persistence becomes messy.
- If chunk IDs and tile coordinates are not normalized early, networking becomes harder later.

## 6.2 Movement System

Subtasks:
- Grid step intent.
- Collision resolution.
- Occupancy rules.
- Speed/cooldown timing.
- Path replay or queued movement later.
- Client prediction hooks.

Risks:
- Smooth visuals can mask simulation errors if the grid rules are unclear.

## 6.3 Interaction System

Subtasks:
- Define interaction intent object.
- Centralize permission/range checks.
- Separate interaction request from interaction result.
- Support tile-targeted and object-targeted interactions.
- Add consistent failure reasons for UI and debugging.

## 6.4 Item and Inventory System

Subtasks:
- Item definitions.
- Stackable vs non-stackable items.
- Ground items.
- Containers.
- Equipment slots.
- Inventory transfer rules.
- Ownership/binding rules only if desired later.

High-risk areas:
- Duplication bugs during split/move/drop actions.
- Coupling inventory UI too tightly to game state internals.

## 6.5 Combat System

Subtasks:
- Auto-attack or explicit attack command.
- Range validation.
- Line-of-sight if required.
- Damage calculation.
- Attack cooldowns.
- Death handling.
- Corpse generation.
- Loot generation.

Risks:
- Combat rules often sprawl unless represented as events and isolated systems.

## 6.6 AI and Creature System

Subtasks:
- Spawn definitions.
- Idle/chase/attack states.
- Leash behavior.
- Respawn control.
- Region-based creature pools.

Risks:
- Pathfinding cost may become a major hotspot in crowded zones.

## 6.7 Persistence System

Subtasks:
- Stable IDs.
- Dirty-state tracking.
- Save batching.
- Versioned format.
- Migration strategy.
- Partial chunk save/load.

Risks:
- Saving full ECS snapshots directly is usually fragile.
- Object timers can desync if wall-clock assumptions are inconsistent.

## 6.8 Networking System

Subtasks:
- Protocol versioning.
- Snapshot vs delta replication.
- Interest management.
- Intent validation.
- Resync path after desync.
- Latency simulation in debug builds.

Risks:
- Over-replicating full state will kill scalability early.
- Lack of protocol versioning will slow development later.

## 6.9 UI System

Subtasks:
- Health/status bar.
- Inventory window.
- Container window.
- Chat panel.
- Target information.
- Debug overlay.

Risks:
- UI can become a bottleneck if built before interaction/state flows are stable.

### 6.9.1 Docked Right-Panel Window System

Problem statement:
- The current right sidebar UI is already becoming too special-cased.
- Container panels and target information are implemented as bespoke layouts with bespoke state.
- This will not scale once more small right-side tools appear, such as inspect panels, spellbook, quest notes, equipment detail, crafting, or debug/info windows.

Desired direction:
- Treat all small right-side panels as the same class of UI object.
- Conceptually they should be docked windows mounted inside the right sidebar rather than one-off HUD fragments.
- Each panel should have:
  - a title bar
  - close button
  - future minimize button
  - scrollable content region
  - resizable height
  - reorderable position within the sidebar stack

Initial candidates to migrate:
- Current target panel
- Opened container panels

Recommended architecture:
- Introduce a generic right-panel manager resource instead of separate UI state per panel type.
- Distinguish:
  - panel chrome and layout behavior
  - panel runtime state
  - panel-specific content rendering

Recommended runtime model:
- `DockedPanelState` resource
  - owns ordered list of open right-side panels
- `DockedPanel`
  - `id`
  - `kind`
  - `title`
  - `order`
  - `height`
  - `minimized`
  - `closable`
  - `resizable`
  - optional future fields:
    - `scroll_offset`
    - `pinned`
    - `focus`
    - `last_interaction_time`
- `DockedPanelKind`
  - `CurrentTarget`
  - `Container { entity: Entity }`
  - future examples:
    - `Inspect { object_id: u64 }`
    - `Spellbook`
    - `QuestLog`
    - `CharacterStats`
    - `Crafting`
    - `AdminDebug`

Recommended ECS/UI component split:
- Generic chrome components:
  - `DockedPanelRoot { panel_id }`
  - `DockedPanelHeader { panel_id }`
  - `DockedPanelTitle { panel_id }`
  - `DockedPanelCloseButton { panel_id }`
  - `DockedPanelMinimizeButton { panel_id }`
  - `DockedPanelBody { panel_id }`
  - `DockedPanelResizeHandle { panel_id }`
- Content marker components:
  - `CurrentTargetPanelContent { panel_id }`
  - `ContainerPanelContent { panel_id, entity }`

System responsibilities:
- Panel manager systems:
  - open panel
  - close panel
  - minimize/restore panel
  - bring to front or reorder
  - resize
  - persist and restore layout later if desired
- Chrome/layout systems:
  - sync panel order into sidebar stack
  - sync title text
  - sync visibility/minimized state
  - sync resize handle visuals
  - provide scrollable body viewport
- Content systems:
  - render current target content
  - render container contents
  - future panel kinds render themselves through dedicated systems

Important design rule:
- A right-side tool should not get its own bespoke top-level HUD state unless it is fundamentally different from a docked panel.
- "Current target" should stop being a permanently hardcoded row under equipment.
- "Open container" should stop being a special inventory mode.
- Both should become instances of the same docked panel system.

Recommended layout behavior:
- The right sidebar contains:
  - compact fixed player status area
  - compact fixed equipment/backpack area, unless those too are later migrated into docked windows
  - docked panel canvas/stack
- The docked panel canvas should behave like a constrained window area:
  - panels stack vertically
  - order can change
  - each panel can have its own height
  - body scroll is internal to the panel, not the whole sidebar
- Hidden or closed panels must fully collapse out of layout and not reserve space.

Recommended first implementation scope:
- Not full drag-and-drop floating windows.
- Only docked windows inside the right sidebar.
- Vertical reordering is enough.
- Height resizing is enough.
- Scrollable content body is enough.
- Minimize can be optional in the first pass, but the model should leave room for it.

Incremental migration plan:
1. Introduce `DockedPanelState` and generic panel chrome.
2. Add a right-panel canvas in the sidebar for dynamically managed docked windows.
3. Migrate container panels first:
   - `DockedPanelKind::Container { entity }`
   - opening a container opens or focuses its corresponding docked panel
4. Migrate current target second:
   - `DockedPanelKind::CurrentTarget`
   - target info renders through panel content systems rather than a fixed row
5. Delete the old dedicated open-container stack and old dedicated target row.
6. Add resize and reorder behavior once the generic panel life cycle is stable.

Why this is the right abstraction:
- The current issue is not just spacing bugs.
- The deeper problem is that the UI state model is too specific to the first two panel types.
- A generic docked panel system reduces future rewrites when more tools are added.
- It also improves consistency:
  - one close interaction model
  - one title bar model
  - one scroll model
  - one resize model

Known risks:
- If panel state is encoded only in Bevy UI entities, later reordering and persistence will be painful.
- If container content logic stays tightly coupled to "active container" assumptions, migration will stall.
- If scroll behavior is bolted on panel-by-panel, future panels will diverge unnecessarily.

Pragmatic recommendation for next implementation session:
- Build the generic docked panel state and rendering skeleton first.
- Migrate container windows onto it before touching resize/reorder.
- Only after container migration is solid, move current target into the same system.
- Avoid partial special-cases that keep both the old and new abstractions alive longer than necessary.

## 7. Suggested Milestone Sequence

### Milestone A: Blank App to Walkable Map
- Cargo project boots.
- Bevy app opens.
- Tile map renders.
- Player walks on grid.

### Milestone B: First Playable Loop
- Collision works.
- Items exist.
- Containers open.
- Monster chases and attacks.
- Player can die and loot can drop.

### Milestone C: Persistent Local Sandbox
- Save/load works.
- Dropped items persist.
- Doors and containers persist.

### Milestone D: Shared World Prototype
- Server runs.
- Two clients connect.
- Players see each other.
- Shared objects replicate.

### Milestone E: MMO Skeleton
- Chunk streaming.
- Respawns and persistent object lifecycle.
- Character persistence.
- Basic chat/admin/debug tooling.

## 8. Recommended Early Technical Decisions

Decisions to make early:
- Map source format:
  - Tiled
  - LDtk
  - custom format
- Networking library approach.
- Save format:
  - JSON/ron for early debugging
  - binary/db later for scale
- Whether to use Bevy states heavily for app flow.
- Whether server also uses Bevy ECS or a separate simulation architecture.

Current recommendation:
- Use Bevy for both client and early server prototype if it keeps iteration fast.
- Keep persistence model outside direct rendering concerns.
- Prefer debug-friendly save formats first.

## 9. Risks and Mitigations

### 9.1 Scope Risk
- MMO scope is large.
- Mitigation:
  - keep a narrow vertical slice first
  - defer advanced social systems
  - ship persistence before scale

### 9.2 Architecture Risk
- Mixing client presentation and server simulation too early will cause painful rewrites.
- Mitigation:
  - separate simulation modules from rendering/UI modules from the beginning

### 9.3 Persistence Risk
- Persistent overworld objects are easy to get wrong and central to the project identity.
- Mitigation:
  - stable IDs
  - chunk-based save boundaries
  - restart tests early

### 9.4 Content Risk
- Even a good engine feels empty without enough objects, creatures, and map detail.
- Mitigation:
  - build content tooling before large content expansion

## 10. Immediate Next Steps

### 10.1 First Implementation Step
- Initialize the Rust/Bevy project skeleton in this repository.
- Create `src/main.rs`.
- Create base plugin/module layout.
- Add `Cargo.toml`.
- Add minimal Bevy app that opens a window.

### 10.2 First Gameplay Step
- Implement a simple tile grid and one player entity with discrete movement.

### 10.3 First Persistence-Oriented Step
- Introduce explicit tile coordinates and durable IDs before object interactions become too complex.

## 11. Living Document Rules

This file should be updated when:
- scope changes
- a milestone is completed
- major architecture decisions are made
- technical risks become clearer
- implementation proves some assumptions wrong

Related planning file:
- `ISSUES.md` should track immediate problems, ideas, and open implementation questions.
