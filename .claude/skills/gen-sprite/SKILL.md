---
name: gen-sprite
description: Generate a pixel-art sprite or animated sprite sheet for a game object in this project. Use when the user asks to create, generate, or add a sprite/animation for a character, NPC, prop, or object.
argument-hint: "<object-id> [description of appearance]"
allowed-tools: Bash Read Write Edit Glob
---

Generate sprite art for the object: **$ARGUMENTS**

## Your task

Create the sprite (sheet or static) and wire it into the game for the object
ID given above.

### 0. Read the style guide

**First read `docs/sprite_style.md`.** It is the single source of truth for
the project's perspective and art rules. The short version you must never
violate:

- Cabinet projection, camera up-and-to-the-south-east; 48 px = 1 tile.
- Height shears **up-and-LEFT**: (−36 px, +24 px up) per floor, half that
  per half-block. Never hand-code the shear — import
  `scripts/wall_perspective.py` (`project`, `canvas_for_content`,
  `fill_polygon`, …).
- **Obstacles and `block_size > 0` objects must show visible 3D height**:
  lit top face, south front face, shadowed east face (3 tones per material).
  Art tops out at fz = 0.5 for `block_size: 1`, fz = 1.0 for `block_size: 2`.
  Flat top-down art is only for `block_size: 0` decals and
  `rotation_by_facing` pieces (square, center-anchored).
- Bottom-anchored art (`y_sort` or `block_size > 0`) pins its bottom-center
  pixel to the tile's south edge: `ANCHOR = (W//2 - TILE_PX//2, H-1)`.
- `sprite_width_tiles` / `sprite_height_tiles` = PNG px ÷ 48, exactly.
- Byte-stable output: no `random`; hash-keyed variation only.

### 1. Gather context

- **Locate the object directory.** Call it `OBJ_DIR`. It is
  `assets/overworld_objects/$0/` if that exists; otherwise the object belongs to
  a content module — find it with `ls -d assets/modules/*/overworld_objects/$0/`
  and use that path. All outputs below go inside `OBJ_DIR` so a module stays
  self-contained.
- Read `OBJ_DIR/metadata.yaml` for the object's name, description, base
  (`extends:`), collision, `block_size`, and current render settings — these
  decide which path below applies.
- Read the existing sprite (if any) with the Read tool to see the current art.

### 2. Pick the right path

**Character/NPC** (anything that walks): follow the 4-facing convention.
- Reference art: `assets/overworld_objects/player/sheet.png`.
- Reference code: `scripts/gen_player_sheet.py`.
- **96×96 px frames, 4 cols × 8 rows** (384×768 sheet). Rows in order:
  `idle_s, walk_s, idle_n, walk_n, idle_e, walk_e, idle_w, walk_w`, 4 frames
  each. Idle ~3 fps (breathing bob ±1 px, blink on frame 3), walk ~8 fps
  (stride with opposite arm swing).
- Build the body from stacked 3D boxes via `wall_perspective.project`, three
  tones per region, footprint symmetric about tile centre `(0.5, 0.5)`.
- Only fall back to the **legacy format** (32×48 frames, 4×2 sheet,
  unsuffixed `idle`/`walk` clips, see `scripts/gen_goblin_sheet.py`) when the
  user asks for it or the sprite must match an existing batch of legacy
  module art.

**Prop/obstacle** (static object): draw a 3D body in the shared projection.
- Reference code: `scripts/gen_container_set.py` (chest/barrel/crate).
- Import `wall_perspective`; model the object as a box/cylinder with TOP,
  SOUTH, and EAST faces; size the canvas with `canvas_for_content` from the
  3D corners; respect the block_size art-height contract.
- Thin upright things (fence-like, sign-like) may be simple upright
  elevations with side shading instead of a full oblique solid.
- Animated props (fire, levers) get a sheet with a single `idle` clip row.

**Flat decal** (`block_size: 0` ground art, or `rotation_by_facing` pieces):
top-down, square, center-anchored — see `scripts/gen_furniture_sheets.py`
flat pieces.

### 3. Write the generator script

Write a Python script to `scripts/gen_<object_id>_sheet.py` (sheets) or
`scripts/gen_<object_id>_sprite.py` (static). Conventions:
- Use `PIL.Image`; named RGBA tuples at the top; reuse `wall_perspective`
  palettes when the material matches (stone, wood, iron).
- Start with a docstring stating the canvas size, projection assumptions, and
  output path.
- Output path: `OBJ_DIR/sheet.png` or `OBJ_DIR/sprite.png` (beside the
  object's `metadata.yaml`, which may be under
  `assets/modules/<name>/overworld_objects/<object_id>/`).

### 4. Run the generator

```bash
nix-shell -p python3Packages.pillow --run "python3 scripts/gen_<object_id>_sheet.py"
```

Then immediately **view the output PNG** with the Read tool. Check: does the
height read (top + front faces)? Does the base sit at the canvas
bottom-center? Is the canvas tight (no wasted transparent margin)?

### 5. Update metadata.yaml

Update the `render:` block in `OBJ_DIR/metadata.yaml`. Sheet/sprite paths are
**relative to the `assets/` root** — i.e. `OBJ_DIR` minus the leading
`assets/`. Rules:
- Static sprite: set `sprite_path`, plus `sprite_width_tiles` /
  `sprite_height_tiles` = canvas px ÷ 48 (exactly — mismatches scale the art).
- Ensure `y_sort: true` for anything upright/bottom-anchored (`obstacle`
  base already provides it; `static_world` does not).
- Character sheet metadata (96×96, 4×8):

```yaml
  animation:
    sheet_path: overworld_objects/<object_id>/sheet.png   # or modules/<name>/overworld_objects/<object_id>/sheet.png
    frame_width: 96
    frame_height: 96
    sheet_columns: 4
    sheet_rows: 8
    clips:
      idle_s: { row: 0, start_col: 0, frame_count: 4, fps: 3.0, looping: true }
      walk_s: { row: 1, start_col: 0, frame_count: 4, fps: 8.0, looping: true }
      idle_n: { row: 2, start_col: 0, frame_count: 4, fps: 3.0, looping: true }
      walk_n: { row: 3, start_col: 0, frame_count: 4, fps: 8.0, looping: true }
      idle_e: { row: 4, start_col: 0, frame_count: 4, fps: 3.0, looping: true }
      walk_e: { row: 5, start_col: 0, frame_count: 4, fps: 8.0, looping: true }
      idle_w: { row: 6, start_col: 0, frame_count: 4, fps: 3.0, looping: true }
      walk_w: { row: 7, start_col: 0, frame_count: 4, fps: 8.0, looping: true }
```

  Also set `logical_height_tiles` (visual height of the character in tiles,
  e.g. `1.2`) so HUD bars sit just above the head, not above the frame.
  Legacy sheets instead use `frame_width: 32`, `frame_height: 48`,
  `sheet_rows: 2`, and unsuffixed `idle` / `walk` clips.

### 6. Verify

Run `cargo check` (via `nix-shell --run "cargo check"`) to confirm the
project still compiles cleanly.

### Style guide (pixel-level)

- Blocky pixel art, 2–3 shading levels per material, no anti-aliasing.
- Characters readable at game scale (48 px tiles).
- Transparent background `(0, 0, 0, 0)` for all empty pixels.
- Avoid pure black outlines — use darkened versions of the material color.
- Light comes from above-left in general: tops highlighted, east faces and
  undersides darkened, subtle contact shadow at the base (`shade_polygon`).
