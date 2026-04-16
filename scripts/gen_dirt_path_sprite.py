"""
Generates assets/overworld_objects/dirt_path/sprite.png
Top-down pixel-art dirt path ground tile, 32×32 px.
Brown trampled earth with subtle wheel-rut texture.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/dirt_path/sprite.png"

BG          = (0,   0,   0,   0)
DIRT        = (142, 100,  52, 255)   # mid brown base
DIRT_DARK   = (110,  74,  32, 255)   # darker rut shadows
DIRT_HI     = (168, 126,  72, 255)   # lighter raised areas
DIRT_PALE   = (186, 148,  96, 255)   # pale dry patches
PEBBLE      = ( 90,  76,  58, 255)   # small embedded stone

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# Fill base dirt
rect(0, 0, W, H, DIRT)

# ── Horizontal rut lines (wheel tracks) ─────────────────────────────────────
for y in [8, 9, 20, 21]:
    rect(0, y, W, 1, DIRT_DARK)

# ── Raised center ridge between ruts ────────────────────────────────────────
rect(0, 13, W, 4, DIRT_HI)
rect(0, 14, W, 2, DIRT_PALE)

# ── Edge variation (slightly raised edges) ───────────────────────────────────
rect(0,  0, W, 3, DIRT_HI)
rect(0, 29, W, 3, DIRT_HI)

# ── Scattered pebbles / dark spots ──────────────────────────────────────────
pebble_coords = [
    (3, 4), (8, 6), (14, 3), (20, 5), (27, 4),
    (5, 26), (11, 28), (17, 25), (23, 27), (29, 26),
    (2, 14), (30, 15), (16, 13),
    (7, 11), (24, 10), (15, 23),
]
for (bx, by) in pebble_coords:
    px(bx, by, PEBBLE)
    px(bx + 1, by, DIRT_DARK)

# ── Subtle noise variation (light patches) ───────────────────────────────────
pale_spots = [
    (4, 16), (10, 17), (18, 16), (25, 15),
    (6, 22), (13, 23), (22, 22),
]
for (sx, sy) in pale_spots:
    px(sx, sy, DIRT_PALE)
    px(sx + 1, sy, DIRT_PALE)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
