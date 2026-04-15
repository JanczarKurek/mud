---
name: gen-sprite
description: Generate an animated pixel-art sprite sheet for a game object in this project. Use when the user asks to create, generate, or add a sprite/animation for a character, NPC, or object.
argument-hint: "<object-id> [description of appearance]"
allowed-tools: Bash Read Write Edit Glob
---

Generate an animated sprite sheet for the object: **$ARGUMENTS**

## Your task

Create a sprite sheet and wire it into the game for the object ID given above. Follow these steps:

### 1. Gather context

- Read `assets/overworld_objects/$0/metadata.yaml` to understand the object's name, description, and current render settings.
- Read the existing sprite (if any) to see the current art style — use the Read tool to view the PNG visually.
- Check `assets/overworld_objects/goblin/sheet.png` and `assets/overworld_objects/player/sheet.png` as style references for what good sheets look like in this project.
- Look at `scripts/gen_goblin_sheet.py` and `scripts/gen_player_sheet.py` as code references for how sheets are generated.

### 2. Design the sprite

Based on the object's name, description, colors (`debug_color`), and any existing sprite, plan:
- **Color palette**: 8–15 named RGBA tuples covering base, highlight, shadow, and detail colors
- **Silhouette**: body proportions and distinctive features that match the character's lore
- **Idle animation**: 4 frames at ~3 fps — subtle breathing bob (±1 px), hair/accessory sway, blink on frame 3
- **Walk animation**: 4 frames at 8 fps — stride cycle with arm swing opposite to legs

All frames must be **32×48 px** (matches the rest of the project). The sheet is **128×96 px** total (4 cols × 2 rows).

### 3. Write the generator script

Write a Python script to `scripts/gen_<object_id>_sheet.py`. Model it closely on the existing generator scripts. Key conventions:
- Use `PIL.Image` and `PIL.ImageDraw`
- Define all colors as named RGBA tuples at the top
- Use `img.putpixel` for single pixels and a local `rect(x, y, w, h, color)` helper for filled blocks
- Draw from bottom to top: boots → pants → belt → torso → arms → neck → head → hair
- The `make_frame(body_dy, l_foot_dy, r_foot_dy, l_arm_dy, r_arm_dy, ...)` signature should match the existing pattern
- Output path: `assets/overworld_objects/<object_id>/sheet.png`

### 4. Run the generator

```bash
nix-shell -p python3Packages.pillow --run "python3 scripts/gen_<object_id>_sheet.py"
```

Then immediately **view the output PNG** with the Read tool to verify it looks right before continuing.

### 5. Update metadata.yaml

Add or replace the `animation:` block under `render:` in `assets/overworld_objects/<object_id>/metadata.yaml`:

```yaml
  animation:
    sheet_path: overworld_objects/<object_id>/sheet.png
    frame_width: 32
    frame_height: 48
    sheet_columns: 4
    sheet_rows: 2
    clips:
      idle:
        row: 0
        start_col: 0
        frame_count: 4
        fps: 3.0
        looping: true
      walk:
        row: 1
        start_col: 0
        frame_count: 4
        fps: 8.0
        looping: true
```

Also ensure `y_sort: true` is set in the `render:` block for any character that stands upright.

### 6. Verify

Run `cargo check` to confirm the project still compiles cleanly.

### Style guide

- Match the pixel-art style of the existing goblin and player sheets: blocky outlines, 2–3 shading levels, no anti-aliasing
- Characters should be recognizable at 32×48 and readable at game scale (48 px tiles)
- Transparent background (`(0, 0, 0, 0)`) for all empty pixels
- Avoid pure black outlines — use darkened versions of the relevant color instead
