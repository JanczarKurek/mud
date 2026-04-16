"""
Generates assets/overworld_objects/fence/sprite.png
Top-down pixel-art wooden fence segment, 32×32 px.
Horizontal planks with vertical posts on each end.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/fence/sprite.png"

BG          = (0,   0,   0,   0)
WOOD        = (160, 100, 40,  255)   # warm mid-brown planks
WOOD_DARK   = (100,  60, 20,  255)   # shadow / gaps between planks
WOOD_HI     = (200, 140, 70,  255)   # highlight top edge of planks
POST        = (120,  72, 24,  255)   # slightly darker post
POST_DARK   = ( 80,  44, 12,  255)   # post shadow side
POST_HI     = (160, 110, 48,  255)   # post highlight

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# ── Left post (x: 3-6, y: 8-23) ────────────────────────────────────────────
rect(3, 8, 4, 16, POST)
rect(3, 8, 1, 16, POST_DARK)     # shadow left edge
rect(6, 8, 1, 16, POST_DARK)     # shadow right edge
rect(4, 8, 2,  1, POST_HI)       # highlight top

# ── Right post (x: 25-28, y: 8-23) ─────────────────────────────────────────
rect(25, 8, 4, 16, POST)
rect(25, 8, 1, 16, POST_DARK)
rect(28, 8, 1, 16, POST_DARK)
rect(26, 8, 2,  1, POST_HI)

# ── Top horizontal plank (y: 11-14) ────────────────────────────────────────
rect(7, 11, 18, 4, WOOD)
rect(7, 11, 18,  1, WOOD_HI)     # highlight top edge
rect(7, 14, 18,  1, WOOD_DARK)   # shadow bottom edge
# nail holes
px(9,  12, WOOD_DARK)
px(23, 12, WOOD_DARK)

# ── Bottom horizontal plank (y: 17-20) ────────────────────────────────────
rect(7, 17, 18, 4, WOOD)
rect(7, 17, 18,  1, WOOD_HI)
rect(7, 20, 18,  1, WOOD_DARK)
# nail holes
px(9,  18, WOOD_DARK)
px(23, 18, WOOD_DARK)

# ── Gap between planks (y: 15-16) ────────────────────────────────────────
rect(7, 15, 18, 2, WOOD_DARK)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
