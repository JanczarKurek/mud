"""
Generates assets/overworld_objects/sand/sprite.png
Top-down pixel-art sand ground tile, 32×32 px.
Light tan sandy beach with subtle granular texture.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/sand/sprite.png"

BG          = (0,   0,   0,   0)
SAND        = (210, 185, 120, 255)   # warm medium-tan base
SAND_DARK   = (180, 152,  88, 255)   # shadowed grain
SAND_HI     = (232, 212, 152, 255)   # highlight crest
SAND_PALE   = (245, 228, 175, 255)   # very light dry patch
WET_SAND    = (178, 148,  80, 255)   # darker wet patch near water

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# Base fill
rect(0, 0, W, H, SAND)

# ── Wet sand band at bottom edge (near water) ────────────────────────────────
rect(0, 26, W, 6, WET_SAND)
rect(0, 25, W, 1, SAND_DARK)   # transition line

# ── Light dune crest (soft diagonal ripple) ──────────────────────────────────
for x in range(W):
    y = 8 + (x * x % 5)
    px(x, y % H, SAND_HI)
    px(x, (y + 1) % H, SAND_PALE)

for x in range(W):
    y = 17 + (x % 4)
    if y < H:
        px(x, y, SAND_HI)

# ── Scattered grain dots (darker specks) ────────────────────────────────────
grain_coords = [
    (2,3),(7,1),(13,4),(19,2),(25,5),(30,3),
    (4,10),(10,12),(16,9),(22,11),(28,10),
    (1,18),(8,20),(14,17),(20,19),(27,18),
    (3,23),(9,22),(15,24),(21,23),(26,22),
    (5,29),(12,31),(18,28),(24,30),(31,29),
]
for (gx, gy) in grain_coords:
    if 0 <= gx < W and 0 <= gy < H:
        px(gx, gy, SAND_DARK)

# ── Pale dry patches ─────────────────────────────────────────────────────────
pale = [(5,6),(6,6),(11,14),(12,14),(20,7),(21,7),(17,21),(18,21),(9,15),(10,15)]
for (px_, py_) in pale:
    if 0 <= px_ < W and 0 <= py_ < H:
        px(px_, py_, SAND_PALE)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
