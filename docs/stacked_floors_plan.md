# Stacked Floors (Z-Levels) — Remaining Work

**Status: paused.** Phases 0 and 1 shipped. The PoC demo (walk up into a
second-floor building, roof hides upper floor from below, stairs teleport
between floors) works. After Phase 1 the codebase pivoted to **floor-type
tiling** (corner-aware grass/dirt/stone transitions, tile variants — see
`src/world/floor_render.rs`, `src/world/floor_definitions.rs`,
`assets/floors/`). That tiling work uses the word "floor" too but is
*orthogonal* to z-levels; don't confuse them.

This doc tracks the remaining z-level work for whenever we resume it. None of
Phase 2 or Phase 3 has started.

See `git log` for implementation history. The data-model decision, the
roof-hiding algorithm, stair semantics, and the save-format migration are
committed and no longer up for debate. If you need that context, read the
commits that landed Phase 0 / Phase 1 alongside `src/world/floors.rs` and
`src/world/object_definitions.rs`.

---

## What's live now

- `TilePosition { x, y, z }`, propagated end-to-end through ECS, wire events,
  and save files (format v4, serde-default upgrade from v3).
- `FLOOR_Z_STEP = 10.0` + shared `y_sort_z(y, z)` / `flat_floor_z` helpers in
  `src/world/systems.rs`. Editor uses the same helpers (no drift).
- `VisibleFloorRange` resource + `recompute_visible_floors` system
  (`src/world/floors.rs`). `sync_tile_transforms` culls floors outside the
  visible range and dims floors below the player.
- `OverworldObjectDefinition.floor_transition` + stair handling in
  `handle_move_player`. `stairs_up` / `stairs_down` / `floor_plank` assets
  authored. Walk animation suppressed on z-change for player and projected
  entities.
- `RenderMetadata.occludes_floor_above` (walls, floor planks) drives
  roof-hiding.
- `RenderMetadata.walkable_surface` + `is_walkable_tile` validator: z > 0
  requires an explicit walkable-surface object at the target. Upper floors
  are positive-space; teleports will use the same validator.
- `cursor_to_tile` returns a tile at the player's z, so floor-1 objects are
  clickable.
- Minimap filters tiles, world-object paint, and overlay dots by player floor.
- Overworld YAML: two-floor building at (2,2)-(7,7) with stairs at (3,3).
- Tests: `stairs_transition_teleports_player_up_one_floor`,
  `stairs_blocked_destination_prevents_transition`,
  `upper_floor_walk_requires_walkable_surface`,
  `npc_does_not_chase_player_on_different_floor`.

---

## Phase 2 — Editor + HUD + NPC polish *(not started)*

- `EditorContext.current_editing_floor` + `PgUp`/`PgDn` shortcuts.
- Editor dims other floors to ~40% opacity for authoring visibility.
- `handle_editor_left_click` spawns at current floor.
- `FloorIndicatorLabel` HUD text ("Floor 1" / "Ground" / "-1").
- NPC floor-locking (the existing z-equality guard already prevents chasing
  across floors; this phase is about QA and `RoamBounds` hardening, and
  making sure spawners don't drop NPCs on untraversable upper-floor tiles).
- `SpaceDefinition` already has a `floors` field but it stores
  `HashMap<FloorTypeId, FloorPlacements>` for the *type-tiling* system.
  Decide whether to add a separate `max_z: i32` (or rename one of the two
  collisions) before adding editor tab counts.

**Deliverable:** Authoring a multi-floor building in the editor without
hand-editing YAML.

## Phase 3 — Full generality *(not started)*

- `ladder`, `rope_spot`, `hole` object kinds.
- Item-gated transitions (`requires_item`, `consumes_item` on
  `FloorTransitionDef`).
- Python scripting snapshot carries z.
- `docs/yaml_formats.md` documents `z`, `floors`, `floor_transition`,
  `occludes_floor_above`, `walkable_surface`.
- Regenerate JSON schemas (`cargo run --bin gen_schemas --features gen-schemas`).
- Minimap floor tabs.
- Projectile rendering gated by floor equality
  (`GameUiEvent::ProjectileFired` currently ignores z).
- `handle_cast_spell_at` — floor-equality guard via the distance helpers
  already returns `i32::MAX` for cross-floor targets; double-check spell
  range semantics match the intent.
- `AdminSpawn` console: accept a z argument (currently spawns on the admin's
  current floor implicitly).
- Underground floors (negative z). Should mostly work out of the box; needs
  content + editor tab scroll handling.

**Deliverable:** Feature parity with `FEATURE_BACKLOG.md` §1. Dungeons, caves,
and multi-floor towers all authorable.

---

## Known caveats

- **Oversized sprites on floor 0** (e.g. 2-tile-tall trees) visually extend
  into floor-1 space. The dimming pass tints them under floor-1 content, but
  the tree still occupies the visual footprint. Acceptable for the PoC.
- **Ground fill at z=0 only** — `spawn_ground_tiles_for_current_space` creates
  one `ClientGroundTile` per (x, y). Upper floors are built only from
  authored `floor_plank` / stair tiles. This is the intended rule (matches
  `walkable_surface` semantics), not a bug.
- **`RoamBounds` is 2D.** NPCs pick up z from their current `TilePosition` but
  their roam bounds don't constrain floor. They'll wander on whichever floor
  they happen to be on. Fine until we have NPCs that cross floors.

---

## Key files

- `src/world/floors.rs` — `VisibleFloorRange` + recompute system.
- `src/world/systems.rs` — shared y-sort helpers, floor-aware
  `sync_tile_transforms`, projection syncs.
- `src/game/systems.rs` — `handle_move_player`, `is_walkable_tile`,
  `chebyshev_distance_tiles`.
- `src/world/object_definitions.rs` — `FloorTransitionDef`, render flags
  (`occludes_floor_above`, `walkable_surface`).
- `src/editor/systems.rs` — where the Phase 2 floor-selector lives.
- `assets/overworld_objects/{stairs_up,stairs_down,floor_plank}/metadata.yaml`.
- `assets/maps/overworld.yaml` — PoC two-floor building.
