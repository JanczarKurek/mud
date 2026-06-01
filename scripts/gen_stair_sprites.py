"""
Generates assets/overworld_objects/stair_{n,s,e,w}_{low,high}/sprite.png.

Two-tile ramps come in 4 facings × 2 pieces:
  *_low  = block_size:1 (lower step) → 48×48 PNG
  *_high = block_size:2 (upper step) → 48×72 PNG (matches stone_step)

Color palette and stone styling mirror gen_stone_step_sprite.py so the new
stairs blend with stone_step and cave_wall. Each facing draws the visible
riser on a different edge so the "going up" direction reads at a glance.

Run from the repo root:
    nix-shell -p python3Packages.pillow --run "python3 scripts/gen_stair_sprites.py"
"""

import os

from PIL import Image


TILE = 48
# Low step: 1 tile wide × 1 tile tall (block_size:1, half-tile-tall visual).
LOW_W, LOW_H = TILE, TILE
# High step: 1 tile wide × 1.5 tiles tall (block_size:2, full-tile riser + tread).
HIGH_W, HIGH_H = TILE, 72

BG          = (  0,   0,   0,   0)
STONE       = (165, 150, 130, 255)   # matches debug_color in metadata
STONE_HI    = (200, 188, 168, 255)   # tread highlight
STONE_DARK  = (110, 100,  86, 255)   # under-tread shadow / riser shadow
STONE_MID   = (138, 126, 110, 255)
MORTAR      = ( 76,  68,  58, 255)
TREAD       = (185, 172, 152, 255)   # walkable top surface
TREAD_HI    = (208, 196, 178, 255)
SHADOW      = (  0,   0,   0,  80)
GROUND_SEAM = ( 28,  24,  20, 255)


def make_image(w, h):
    return Image.new("RGBA", (w, h), BG)


def putpx(img, x, y, c):
    if 0 <= x < img.width and 0 <= y < img.height:
        img.putpixel((x, y), c)


def fill_rect(img, x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            putpx(img, x + dx, y + dy, c)


def draw_riser_blocks(img, x, y, w, h):
    """Stone-block masonry pattern for the visible riser face."""
    fill_rect(img, x, y, w, h, STONE)
    # 3 vertical block divisions
    if w >= 32:
        fill_rect(img, x + w // 3, y, 1, h, MORTAR)
        fill_rect(img, x + (2 * w) // 3, y, 1, h, MORTAR)
    # horizontal mortar courses every ~14 px
    course_step = 14
    cy = y + course_step
    while cy < y + h - 1:
        fill_rect(img, x, cy, w, 1, MORTAR)
        cy += course_step
    # subtle bevels on the riser
    fill_rect(img, x, y, w, 1, STONE_DARK)
    fill_rect(img, x, y + h - 1, w, 1, STONE_DARK)


def draw_tread(img, x, y, w, h):
    """Walkable top surface with a lit upper edge and a thin back lip."""
    fill_rect(img, x, y, w, h, TREAD)
    # top-edge highlight
    fill_rect(img, x, y, w, 2, TREAD_HI)
    # back lip (depth cue)
    fill_rect(img, x, y - 1, w, 1, STONE_DARK)
    fill_rect(img, x, y - 2, w, 1, STONE_MID)
    # front under-shadow where tread overhangs riser
    fill_rect(img, x, y + h, w, 1, STONE_DARK)


def draw_ground_shadow(img):
    w = img.width
    h = img.height
    # narrow shadow strip along the bottom
    for dy in (h - 2, h - 1):
        for x in range(2, w - 2):
            putpx(img, x, dy, SHADOW)
    fill_rect(img, 0, h - 1, w, 1, GROUND_SEAM)


def draw_directional_accent(img, direction):
    """Subtle per-direction shading so a player can tell the facings apart.
    The accent is a 1-px highlight on the edge the stair "rises toward"."""
    w = img.width
    h = img.height
    if direction == "n":
        # rises toward back (north) → light north (top) inner edge
        fill_rect(img, 2, 2, w - 4, 1, STONE_HI)
    elif direction == "s":
        # rises toward camera (south) → light south (bottom of tread) inner edge
        # find tread bottom heuristically: 2px from tread bottom
        fill_rect(img, 2, h - 4, w - 4, 1, STONE_HI)
    elif direction == "e":
        fill_rect(img, w - 3, 2, 1, h - 4, STONE_HI)
    elif direction == "w":
        fill_rect(img, 2, 2, 1, h - 4, STONE_HI)


def build_low(direction):
    img = make_image(LOW_W, LOW_H)
    # Riser occupies the bottom ~28 px; tread occupies y 16..27.
    riser_y, riser_h = 28, LOW_H - 28 - 2  # leave space for ground shadow
    tread_y, tread_h = 18, 10
    draw_riser_blocks(img, 0, riser_y, LOW_W, riser_h)
    draw_tread(img, 0, tread_y, LOW_W, tread_h)
    draw_directional_accent(img, direction)
    draw_ground_shadow(img)
    return img


def build_high(direction):
    img = make_image(HIGH_W, HIGH_H)
    # Riser occupies the bottom ~46 px; tread occupies y 18..27 (matches stone_step).
    riser_y = 28
    riser_h = HIGH_H - riser_y - 2
    tread_y, tread_h = 18, 10
    draw_riser_blocks(img, 0, riser_y, HIGH_W, riser_h)
    draw_tread(img, 0, tread_y, HIGH_W, tread_h)
    draw_directional_accent(img, direction)
    draw_ground_shadow(img)
    return img


def main():
    out_dir = "assets/overworld_objects"
    targets = []
    for direction in ("n", "s", "e", "w"):
        targets.append((f"stair_{direction}_low", build_low(direction)))
        targets.append((f"stair_{direction}_high", build_high(direction)))

    for name, img in targets:
        path = os.path.join(out_dir, name, "sprite.png")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        img.save(path)
        print(f"Saved {path}  ({img.width}×{img.height})")


if __name__ == "__main__":
    main()
