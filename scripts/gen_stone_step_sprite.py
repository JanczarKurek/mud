"""
Generates assets/overworld_objects/stone_step/sprite.png
Bottom-anchored stone step block, 48x72 px (1 tile wide, 1.5 tiles tall).
Designed to read as a single climbable stair step — flat walkable top with a
visible thickness below.
"""

from PIL import Image
import os

W, H = 48, 72
OUT_PATH = "assets/overworld_objects/stone_step/sprite.png"

BG          = (  0,   0,   0,   0)
STONE       = (150, 142, 128, 255)   # matches debug_color
STONE_HI    = (188, 178, 162, 255)   # top tread highlight
STONE_DARK  = (102,  96,  86, 255)   # under-tread shadow / riser detail
STONE_MID   = (124, 118, 108, 255)
MORTAR      = ( 70,  64,  56, 255)
TREAD       = (170, 162, 148, 255)   # walkable top surface
SHADOW      = (  0,   0,   0,  80)


img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)


# Ground shadow under the step
for ox in range(-22, 23):
    rect(24 + ox, 70, 1, 1, SHADOW)
for ox in range(-20, 21):
    rect(24 + ox, 71, 1, 1, SHADOW)

# ── Riser (front face of the step) — spans full width, lower portion ───────
# Riser body: y 26..70
rect(0, 26, W, 44, STONE)

# Vertical block divisions on the riser (3 stones across)
rect(15, 27, 1, 42, MORTAR)
rect(31, 27, 1, 42, MORTAR)

# Horizontal mortar courses on the riser
for cy in (40, 54):
    rect(0, cy, W, 1, MORTAR)

# Riser block highlights and shadows (lower-left lit corners, upper-right
# shadow lines)
for (bx, by, bw, bh) in [
    ( 0, 26, 15, 14), (16, 26, 15, 14), (32, 26, 16, 14),
    ( 0, 41, 15, 13), (16, 41, 15, 13), (32, 41, 16, 13),
    ( 0, 55, 15, 14), (16, 55, 15, 14), (32, 55, 16, 14),
]:
    rect(bx, by, bw, 1, STONE_HI)              # top edge
    rect(bx, by, 1, bh, STONE_HI)              # left edge
    rect(bx + bw - 1, by, 1, bh, STONE_DARK)   # right edge
    rect(bx, by + bh - 1, bw, 1, STONE_DARK)   # bottom edge

# ── Tread (top step surface) — full width, slight overhang ────────────────
# Tread body: y 18..27 (slightly thicker than a typical capstone)
rect(0, 18, W, 9, TREAD)
# Top-edge highlight (the actual walkable surface lit from above)
rect(0, 18, W, 2, STONE_HI)
# Front under-shadow where tread overhangs riser
rect(0, 26, W, 1, STONE_DARK)
# Subtle wear marks along the tread
for x in (8, 14, 22, 30, 38, 44):
    px(x, 22, STONE_MID)
    px(x + 1, 23, STONE_MID)

# ── Back lip — thin darker band along the top to read the depth ─────────
rect(0, 17, W, 1, STONE_DARK)
rect(0, 16, W, 1, STONE_MID)

# Ground seam at the very bottom so the step reads as resting on tile
rect(0, H - 1, W, 1, (28, 24, 20, 255))


os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
