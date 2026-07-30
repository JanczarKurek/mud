# Sprite Style & Perspective Guide

Single source of truth for how sprite art in this project is projected,
sized, anchored, and lit. Generator scripts (`scripts/gen_*.py`), the
`gen-sprite` skill, and `docs/yaml_formats.md` all defer to this document.

The constants referenced here live in two places that must stay mirrored:
`src/world/systems.rs` (`FLOOR_SHIFT_X_TILES`, `FLOOR_SHIFT_Y_TILES`) and
`scripts/wall_perspective.py` (the Python copies plus all projection helpers).

## Camera & projection

The game uses a **cabinet (oblique) projection**. The ground plane is drawn
top-down on a square tile grid; vertical height is drawn as a shear.

- **Pixel density: 48 px = 1 tile** (`TILE_PX` / `WorldConfig.tile_size`).
  All art is authored at this density; `sprite_width_tiles` /
  `sprite_height_tiles` only restate the canvas size, they never rescale
  (see [Canvas & metadata contract](#canvas--metadata-contract)).
- **The camera sits up-and-to-the-south-east.** Consequences:
  - South faces of objects are visible (the "front").
  - East faces are visible as a narrow side.
  - Tops are visible.
  - Lower `tile_y` (south) and higher `tile_x` (east) are both "closer to
    the viewer" for depth sorting (`y_sort_z` in `src/world/systems.rs`).
- **Height shears up-and-LEFT.** One full floor of height projects to
  `(FLOOR_SHIFT_X_TILES, FLOOR_SHIFT_Y_TILES) = (−0.75, +0.5)` tiles of
  screen offset = **(−36 px, 24 px up)** at 48 px tiles. One half-block
  (`z` step, `block_size` unit) is half that: (−18 px, 12 px up). The
  renderer applies the same shear to whole floors and to intra-tile object
  stacks (`floor_screen_offset`), so sprite art drawn with this slope stacks
  flush across floors.

In generator scripts, never hand-code the shear — import from
`scripts/wall_perspective.py`:

- `project(fx, fy, fz, anchor)` — 3D floor-coords → PIL pixel. `fx` = east,
  `fy` = north, `fz` = floors up; PIL `+y` is down.
- `canvas_for_content(corners_3d)` — tight canvas + anchor for a 3D shape.
- `fill_polygon`, `stroke_polygon`, `shade_polygon`, `_line`, `px`, `rect` —
  drawing primitives.
- The shared stone/wood palettes, if the object should match the wall/door
  masonry.

## Objects have visible height

Anything the player collides with, or that has `block_size > 0`, should read
as a **3D body**, not a flat top-down decal: draw the lit **top** face, the
**south** front face, and (when the body has depth) the shadowed **east**
face. This is the single most important rule — an obstacle whose sprite shows
no height looks like a floor stain the player inexplicably can't walk over.

- **Three tones per material region**: front (base), east side (darkened),
  top cap (highlighted). Walls (`gen_wall_set.py`), containers
  (`gen_container_set.py`) and the player (`gen_player_sheet.py`) all follow
  this; reuse their palettes or derive new ones the same way.
- **Art-height contract** (stacking, from `src/world/systems.rs`):
  - `block_size: 2` (full block) art tops out at **fz = 1.0** (one floor).
  - `block_size: 1` (half block) art tops out at **fz = 0.5**.
  - This makes stacked objects sit flush: a crate on a crate, a wall on a
    lower floor's wall.
- **Flat top-down art is only for**:
  - `block_size: 0` ground decals — rugs, paths, corpses, dropped items.
  - `rotation_by_facing` pieces (tables, benches) — the engine rotates the
    sprite 90°, so the art must be a **square, center-anchored, top-down**
    tile; a sheared sprite would break under rotation.
- Thin upright things (fences, signs, tombstones, vegetation) may be drawn
  as **upright elevations** (billboards) without the full oblique shear —
  visible height matters more than a strictly correct east face. Give them
  a bottom-anchored canvas and at least a hint of side shading.

## Anchoring & canvas

The renderer decides anchoring from metadata
(`src/world/setup.rs::bottom_anchor_for`):

- `y_sort: true` **or** `block_size > 0` → `Anchor::BOTTOM_CENTER`, and the
  bottom-center pixel of the canvas is pinned to the **south edge** of the
  home tile (`anchor_y_offset = -tile_size * 0.5` in
  `src/world/systems.rs::sync_tile_transforms`).
- `rotation_by_facing: true` overrides this → center anchor, square art.
- Neither → plain center anchor (flat decals).

For bottom-anchored art the generator must place the 3D origin so that the
tile's south-center `project(0.5, 0, 0)` lands at canvas
`(W/2, H-1)` — i.e. for a canvas drawn with `wall_perspective.project`:

```python
ANCHOR = (W // 2 - TILE_PX // 2, H - 1)
```

(`canvas_for_content` computes this for you from the shape's 3D corners.)

Keep canvases **tight**: a sprite's transparent margin still occludes
sprites behind it in z-tie situations (cross-tile alpha occlusion — see the
`canvas_for_content` docstring). Don't pad a canvas "for safety".

## Canvas & metadata contract

- `sprite_width_tiles` = PNG width ÷ 48, `sprite_height_tiles` = PNG height
  ÷ 48, **exactly**. The renderer sets `custom_size` from these fields; a
  mismatch silently *scales* the art (historic example: `stone_step`'s
  48×72 art squashed by `sprite_height_tiles: 1.0`).
- Animated sheets render at raw frame pixel size; the frame dimensions are
  the on-screen dimensions.
- `logical_height_tiles` tells HUD anchoring (health bar, status icons) the
  visual height of the character when the frame is taller than the art —
  the player's 96 px frame uses `logical_height_tiles: 1.2`.
- Base yamls: `assets/object_bases/obstacle.yaml` already sets
  `y_sort: true`; `static_world.yaml` does **not** — non-colliding upright
  props must set it themselves.

## Characters

**New characters use 4-facing oblique sheets**, modeled on
`scripts/gen_player_sheet.py`:

- **96×96 px frames**, sheet **4 columns × 8 rows** (384×768). The width
  absorbs the head's up-left lean (36 px/floor) for a footprint centered on
  the tile.
- Row order: `idle_s`, `walk_s`, `idle_n`, `walk_n`, `idle_e`, `walk_e`,
  `idle_w`, `walk_w` — 4 frames each; idle ~3 fps, walk ~8 fps.
- Body is composed of stacked 3D boxes via `wall_perspective.project`, three
  tones per region (front / east side / lit top), footprint symmetric about
  the tile centre `(0.5, 0.5)` so 90° facing changes keep the character on
  the same spot.
- Frame anchor: `ANCHOR = (FRAME_W // 2 - TILE_PX // 2, FRAME_H - 1)`.
- Metadata: `y_sort: true`, `logical_height_tiles`, and the 8 clips (see
  `assets/overworld_objects/player/metadata.yaml` for the exact block).

**Legacy format** — 32×48 frames, 4×2 sheet, single south-facing `idle` /
`walk` rows (goblin, Hollow Bell NPCs, most townsfolk). Still supported:
clip resolution (`src/world/animation.rs::resolved_clip`) prefers
facing-suffixed clips (`walk_n`) and falls back to unsuffixed (`walk`).
Use it only when matching an existing batch of legacy art; new characters
get the 4-facing treatment.

## Determinism

Generated art must be **byte-stable**: running a generator twice produces
identical PNGs. Never use `random` — key variation on `hashlib` digests of
stable coordinates (see `_block_shade` in `wall_perspective.py`, and the
invariants list in `common_issues.md`).
