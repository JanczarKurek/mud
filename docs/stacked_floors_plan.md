# Stacked Floors (Z-Levels) — Implementation Plan

Proof-of-concept target: buildings on the overworld map get a walkable second
floor, reached by stairs, with a roof that hides the upper floor when viewed
from below.

---

## 1. Data-Model Decision

**Extend `TilePosition` to `{ x, y, z: i32 }`. Do NOT add a separate `Floor` component.**

Reasoning:
- Every position query that needs x/y also needs z (collision, rendering, AI,
  range checks). A position without floor is never correct.
- A separate `Floor` component forces every existing query tuple to grow, and
  there's no ECS enforcement that position and floor are updated atomically —
  drift becomes a silent bug class.
- `TilePosition` is already `Copy` and serialized via serde; adding an i32 with
  `#[serde(default)]` makes v3 save files load as v4 transparently.

Name the field `z` (not `floor`) for symmetry with `x`/`y`. Add a
`TilePosition::ground(x, y)` constructor so the ~150 existing `::new(x, y)`
call sites don't have to think about z during migration.

---

## 2. Roof-Hiding Algorithm (Tibia-style)

Given local player on floor `P`:
- **Floor f = P**: render at full brightness, y-sorted as today.
- **Floor f < P** (`P-3 ≤ f < P`): render dimmed (~0.55 alpha) so players see
  down through holes.
- **Floor f > P**: hidden if any tile at `(player.x, player.y, f)` has an
  object tagged `occludes_floor_above: true` (walls, ceilings, floor planks).

`occludes_floor_above: bool` is a new field on `RenderMetadata` (default
`false`). Keep as the single opt-in flag: walls and floor-plank pieces tag it,
nothing else does. This gives the "walk inside, roof vanishes" feel that
defines Tibia's look.

Implementation:
- New resource `VisibleFloorRange { player_floor, lowest_visible, highest_visible }`.
- New system `recompute_visible_floors` runs each frame; cheap (only walks
  floors above the player until it finds a covering tile).
- `sync_tile_transforms` and the editor's duplicate use `VisibleFloorRange` to
  either place sprites normally, tint them, or hide with the `-10_000.0`
  offscreen sentinel already used for cross-space hides.
- `FLOOR_Z_STEP: f32 = 10.0`. `y_sort_z(y, z) = z * 10.0 + (1.0 - y * 0.01)`.
  Safe given y-sort's max span is ~1.5 on the largest authored maps.

Critical: the editor has a duplicated `sync_tile_transforms_editor` with its
own y-sort math. Factor the calculation into a shared helper or they'll drift.

---

## 3. Stair Semantics

**Same-tile teleport** (same pattern as portals in `handle_move_player`):
- Walking onto a `stairs_up` tile at `(x, y, z)` → player moves to `(x, y, z+1)`
  immediately.
- If destination is blocked by a collider → refuse, player stays at `(x, y, z)`,
  chat log "The way is blocked.".
- If destination has another player → refuse silently (same as player-on-player
  collision today).

`OverworldObjectDefinition` gains:

```rust
pub struct FloorTransitionDef {
    pub delta: i32,                      // +1 for stairs_up, -1 for stairs_down, rope, hole
    pub requires_item: Option<String>,   // "rope" for rope_spot (out of PoC)
}

pub floor_transition: Option<FloorTransitionDef>,
```

The stair tile itself is non-colliding; you step onto it like any other
walkable tile. The transition runs *after* the normal move, before the portal
check, so stairs + portal on the same tile compose cleanly (stairs first, then
portal).

Also: a stair transition is not a step — suppress `JustMoved` so the walk
animation doesn't play across floors.

---

## 4. Save-Format Migration (v3 → v4)

Cheap path, no explicit migrator:

```rust
pub struct TilePosition {
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub z: i32,
}
```

- v3 saves deserialize with `z = 0` (serde default) — everything lands on the
  ground floor, no data loss.
- Bump `format_version: 3 → 4` in `save_world_on_app_exit`.
- Log "upgrading save file from v3 to v4" once on load when version < 4.
- Fix the stale `assert_eq!(dump.format_version, 2);` test assertion while we're
  in there (pre-existing latent bug).

Wire protocol version tracks save version — document in CLAUDE.md that embedded
and headless builds must match.

---

## 5. Phased Delivery

### Phase 0 — Type propagation, zero behaviour change

Land `TilePosition { x, y, z }` everywhere with `z = 0` baked into every call
site. Game behaves identically to today. `cargo test` green.

Touches ~27 files, 281 occurrences of `TilePosition`. Mechanical: add
`TilePosition::ground(x, y)` helper and global-replace `::new(x, y)` →
`::ground(x, y)`. Floor-equality guards added to:
- `is_near_player` (currently x/y-only, duplicated in `src/game/systems.rs` and
  `src/ui/systems.rs` — dedupe while adding z)
- `chebyshev_distance_tiles` callers (8 of them) — distance stays 2D but every
  caller gets a z-equality guard
- `is_target_in_range` and combat leash
- NPC blocker / nearest-player / roam-step checks
- `CastSpellAt` — floor equality before range

**Deliverable:** All existing gameplay works. No new content.

### Phase 1 — Rendering + stairs (PoC)

- Add `FLOOR_Z_STEP` and the 3D-y-sort helper.
- Add `VisibleFloorRange` resource and `recompute_visible_floors` system.
- Extend `sync_tile_transforms` (and editor twin) for floor cull + dim tint.
- Add `FloorTransitionDef` + stair handling in `handle_move_player`.
- Add `occludes_floor_above` to `RenderMetadata`.
- Author `stairs_up`, `stairs_down`, `floor_plank` object kinds in
  `assets/overworld_objects/`.
- Hand-edit `assets/maps/overworld.yaml` to add a building with a floor 1 (a
  ring of walls, floor-planks, stairs up + down).
- Filter minimap dots by `player_floor` so floors don't bleed through.

**Unit tests:**
- `stairs_transition_teleports_player_up_one_floor`
- `stairs_blocked_destination_prevents_transition`
- `stairs_transition_does_not_trigger_walk_animation`
- `roof_hides_floor_above_when_player_under_cover`
- `roof_does_not_hide_when_player_steps_out`
- `floor_below_player_renders_dimmed`
- `move_item_to_different_floor_is_out_of_reach`
- `npc_does_not_chase_player_on_different_floor`
- `projection_sends_tile_position_z_in_event`
- `persistence_load_v3_save_defaults_z_to_zero`

**Deliverable:** Walk up into a second-floor building. Roof hides the upper
floor until you go inside. This is the PoC demo.

### Phase 2 — Editor + HUD + NPC polish

- `EditorContext.current_editing_floor` + `PgUp`/`PgDn` shortcuts.
- Editor dims other floors to ~40% opacity for authoring visibility.
- `handle_editor_left_click` spawns at current floor.
- `FloorIndicatorLabel` HUD text ("Floor 1" / "Ground" / "-1").
- NPC floor-locking (NPCs don't leave their floor; chase/roam clamped to z).
- `SpaceDefinition.floors: i32` (default 1) declares max floor for the editor
  tab count.

**Deliverable:** Authoring a multi-floor building in the editor without
hand-editing YAML.

### Phase 3 — Full generality

- `ladder`, `rope_spot`, `hole` object kinds.
- Item-gated transitions (`requires_item`, `consumes_item`).
- Python scripting snapshot carries z.
- `docs/yaml_formats.md` gets z / floors / floor_transition / occludes_floor_above.
- Regenerate JSON schemas (`cargo run --bin gen_schemas --features gen-schemas`).
- Minimap floor tabs.
- Projectile rendering gated by floor equality.

**Deliverable:** Feature parity with backlog §1. Dungeons, caves, and
multi-floor towers all authorable.

---

## 6. PoC Scope — IN / OUT

**IN (Phase 0 + Phase 1):**
- `TilePosition` gains `z`, propagated end-to-end.
- Transparent v3 → v4 save migration.
- `stairs_up` + `stairs_down`, same-tile teleport.
- Floor-aware rendering: dim below, cull above, roof-hiding via
  `occludes_floor_above`.
- One hand-authored building with a floor 1 on the overworld.
- Minimap filters by floor.
- Named unit tests.

**OUT:**
- Underground floors (negative z).
- Ladder, rope, hole objects.
- Item-gated transitions.
- Editor floor selector.
- HUD floor indicator.
- Minimap floor tabs.
- Full `yaml_formats.md` rewrite (short addendum only).
- NPC floor-awareness hardening beyond "z-equality in existing checks".
- Python scripting z-awareness.

---

## 7. Architectural Landmines

1. **Duplicated y-sort math** — `src/world/systems.rs:268` and
   `src/editor/systems.rs:223`. Factor into a shared helper or one will drift.
2. **`is_near_player` is duplicated** — `src/game/systems.rs:~2487` and
   `src/ui/systems.rs:~2079`. Dedupe while adding z-equality.
3. **`chebyshev_distance_tiles` is triplicated** — `game/`, `combat/`, `npc/`.
   Distance stays 2D, but every caller needs a z-equality guard.
4. **Editor y-sort trap** — the duplicate calculation means PRs can look
   correct in editor but broken in-game (or vice versa). Shared helper is
   non-negotiable.
5. **Ground fill at z=0 only** — `spawn_ground_tiles_for_current_space` creates
   one `ClientGroundTile` per (x, y). For the PoC, floor-1 content is only the
   explicitly authored `floor_plank` objects (no default fill). Clean rule.
6. **Oversized sprites on floor 0** (e.g. 2-tile trees) will visually extend
   into floor-1 space. The dimming pass handles it correctly — the tree tints
   dim under floor-1 content.
7. **`CastSpellAt` across floors** — currently would target the wrong thing.
   Add floor-equality guard in `handle_cast_spell_at`.
8. **`GameUiEvent::ProjectileFired`** — projectiles carry no floor. Post-PoC:
   gate projectile rendering on floor equality.
9. **`NpcStateDump`** — NPCs automatically pick up z from `TilePosition`; their
   2D `RoamBounds` means they roam at whichever floor they happen to land on.
   Fine for the PoC, worth thought for dungeons.
10. **`AdminSpawn`** — today takes a 2D coord; after Phase 0 it carries z
    implicitly but the debug console syntax may need a z parameter.

---

## 8. Key File Targets

- `src/world/components.rs` — `TilePosition`, `ViewPosition`, `SpacePosition`,
  `SpaceResident`, `WorldVisual`. The type change radiates from here.
- `src/world/systems.rs` — `y_sort_z`, `sync_tile_transforms`, `sync_player_z`,
  all `sync_*_projection` siblings. Rendering path.
- `src/game/systems.rs` — `handle_move_player` (stairs transition lives here),
  `is_near_player`, `chebyshev_distance_tiles`, `handle_cast_spell_at`,
  `handle_admin_spawn`. Authoritative movement.
- `src/world/map_layout.rs` — `TileCoordinate`, `SpaceDefinition`,
  `PortalDefinition`. YAML deserialization; the `floors:` field lands here.
- `src/world/object_definitions.rs` — `OverworldObjectDefinition`,
  `RenderMetadata`. Home of `FloorTransitionDef` and `occludes_floor_above`.
- `src/persistence/mod.rs` — `TilePosition` serde, `format_version: 3 → 4`.
- `src/editor/systems.rs` — duplicate render logic + authoring tools.
