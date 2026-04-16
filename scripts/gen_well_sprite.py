"""
Generates assets/overworld_objects/well/sprite.png
Top-down pixel-art stone well with bucket and rope, 32×32 px.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/well/sprite.png"

BG          = (0,   0,   0,   0)
STONE       = (140, 130, 120, 255)   # light grey stone
STONE_DARK  = ( 90,  82,  72, 255)   # shadow mortar lines
STONE_HI    = (180, 170, 160, 255)   # top highlight
WATER       = ( 40,  80, 160, 255)   # dark water inside
WATER_HI    = ( 80, 130, 210, 255)   # water highlight glint
ROPE        = (180, 140,  60, 255)   # rope tan
ROPE_DARK   = (120,  90,  30, 255)
BUCKET      = ( 80,  50,  20, 255)   # wooden bucket dark
BUCKET_HI   = (130,  85,  35, 255)
BUCKET_BAND = (100, 100, 100, 255)   # metal band

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# ── Outer stone ring (hollow circle approximated) ────────────────────────────
# Draw outer ring as a thick border, leaving interior hollow
# Outer: x 6-25, y 6-25 approximately; inner opening: x 10-21, y 10-21
outer_pixels = []
inner_pixels = []

for y in range(H):
    for x in range(W):
        cx, cy = x - 15.5, y - 15.5
        d = (cx*cx + cy*cy) ** 0.5
        if 9 <= d <= 13:
            outer_pixels.append((x, y))
        elif d < 9:
            inner_pixels.append((x, y))

for (x, y) in outer_pixels:
    img.putpixel((x, y), STONE)

# Mortar lines (horizontal) across the ring
for y in [10, 14, 18, 22]:
    for (rx, ry) in outer_pixels:
        if ry == y:
            img.putpixel((rx, ry), STONE_DARK)

# Highlight top arc
for (x, y) in outer_pixels:
    cx, cy = x - 15.5, y - 15.5
    if cy < -8:
        img.putpixel((x, y), STONE_HI)

# ── Water interior ───────────────────────────────────────────────────────────
for (x, y) in inner_pixels:
    img.putpixel((x, y), WATER)

# Water glint
for (x, y) in inner_pixels:
    cx, cy = x - 15.5, y - 15.5
    if -4 <= cx <= -1 and -3 <= cy <= -1:
        img.putpixel((x, y), WATER_HI)

# ── Rope (two diagonal lines from top of well toward center) ────────────────
for i in range(5):
    px(14 - i, 6 + i, ROPE)
    px(13 - i, 6 + i, ROPE_DARK)

# ── Bucket (small rectangle on right side, resting on well edge) ────────────
rect(21, 9, 5, 7, BUCKET)
rect(21, 9, 5, 1, BUCKET_HI)     # top highlight
rect(21, 12, 5, 1, BUCKET_BAND)  # metal band
rect(25, 9, 1, 7, BUCKET)        # right shadow implied

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
