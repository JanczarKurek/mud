"""
Generates assets/overworld_objects/book/sprite.png
Top-down pixel-art closed leather-bound book, 32×32 px.
Brown leather cover with a gold band across the spine and faint
embossing on the front. Static (single frame).
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/book/sprite.png"

BG          = (0,   0,   0,   0)
COVER       = (110,  55,  25, 255)   # leather brown
COVER_DARK  = ( 65,  30,  10, 255)
COVER_HI    = (150,  85,  40, 255)
PAGES       = (235, 220, 180, 255)   # ivory page edges
PAGES_DARK  = (180, 160, 110, 255)
SPINE       = ( 80,  40,  15, 255)   # darker spine groove
GOLD        = (215, 170,  60, 255)   # gold band on spine
GOLD_HI     = (245, 215, 110, 255)
EMBOSS      = (140,  75,  30, 255)   # faint embossing on cover
SHADOW      = (  0,   0,   0,  70)   # ground shadow

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# Ground shadow under the book (soft oval)
for ox in range(-9, 10):
    px(16 + ox, 26, SHADOW)
for ox in range(-10, 11):
    px(16 + ox, 27, SHADOW)
for ox in range(-9, 10):
    px(16 + ox, 28, SHADOW)
for ox in range(-7, 8):
    px(16 + ox, 29, SHADOW)

# Book body: viewed slightly from above, 3/4 angle.
# Outer footprint x:5-26, y:8-25.

# Page edges (visible right side + bottom slivers) drawn first so the
# cover can overlap them on top.
rect(6, 9, 21, 16, PAGES)
# Darken the rear / bottom edge of pages for a hint of depth.
rect(6, 24, 21, 1, PAGES_DARK)
rect(26, 9, 1, 16, PAGES_DARK)

# Cover (top face) — slightly inset so the page block peeks out on
# right and bottom.
rect(5, 8, 21, 16, COVER)
# Highlight on the top and left edges of the cover.
rect(5, 8, 21, 1, COVER_HI)
rect(5, 8,  1, 16, COVER_HI)
# Shadow on the bottom and right edges of the cover.
rect(5, 23, 20, 1, COVER_DARK)
rect(24, 8, 1, 16, COVER_DARK)

# Spine groove on the left side (slight indent).
rect(7, 9, 1, 14, SPINE)

# Gold band across the spine (two parallel bands)
rect(5, 12, 3, 1, GOLD)
rect(5, 13, 3, 1, GOLD_HI)
rect(5, 18, 3, 1, GOLD)
rect(5, 19, 3, 1, GOLD_HI)

# Faint embossed shape on the front cover (small diamond)
px(15, 13, EMBOSS)
px(14, 14, EMBOSS)
px(16, 14, EMBOSS)
px(13, 15, EMBOSS)
px(17, 15, EMBOSS)
px(14, 16, EMBOSS)
px(16, 16, EMBOSS)
px(15, 17, EMBOSS)
# Small dot in the diamond's center for an embossing highlight
px(15, 15, COVER_HI)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
