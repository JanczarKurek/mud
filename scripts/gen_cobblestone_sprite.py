"""
Generates assets/overworld_objects/cobblestone/sprite.png
Top-down pixel-art cobblestone ground tile, 32×32 px.
Grey stone blocks with mortar joints.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/cobblestone/sprite.png"

BG          = (0,   0,   0,   0)
MORTAR      = ( 90,  86,  80, 255)   # dark grey mortar joints
STONE_A     = (148, 142, 134, 255)   # medium grey stone
STONE_B     = (130, 124, 116, 255)   # slightly darker stone variant
STONE_C     = (162, 156, 148, 255)   # lighter stone variant
STONE_HI    = (178, 172, 164, 255)   # top-left highlight edge
STONE_DARK  = (108, 102,  94, 255)   # bottom-right shadow edge

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

def cobble(x, y, w, h, base_color):
    """Draw one cobblestone with highlight/shadow edges."""
    rect(x, y, w, h, base_color)
    # Top & left highlight
    rect(x, y, w, 1, STONE_HI)
    rect(x, y, 1, h, STONE_HI)
    # Bottom & right shadow
    rect(x, y + h - 1, w, 1, STONE_DARK)
    rect(x + w - 1, y, 1, h, STONE_DARK)

# Fill entire tile with mortar color first
rect(0, 0, W, H, MORTAR)

# ── Row 0 (y: 1-7) ───────────────────────────────────────────────────────────
cobble( 1,  1,  8, 7, STONE_A)
cobble(10,  1, 10, 7, STONE_B)
cobble(21,  1,  9, 7, STONE_C)

# ── Row 1 (y: 9-15) ──────────────────────────────────────────────────────────
cobble( 1,  9, 12, 7, STONE_C)
cobble(14,  9,  7, 7, STONE_A)
cobble(22,  9,  9, 7, STONE_B)

# ── Row 2 (y: 17-23) ─────────────────────────────────────────────────────────
cobble( 1, 17,  9, 7, STONE_B)
cobble(11, 17, 10, 7, STONE_C)
cobble(22, 17,  9, 7, STONE_A)

# ── Row 3 (y: 25-31) ─────────────────────────────────────────────────────────
cobble( 1, 25, 11, 6, STONE_A)
cobble(13, 25,  8, 6, STONE_B)
cobble(22, 25,  9, 6, STONE_C)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
