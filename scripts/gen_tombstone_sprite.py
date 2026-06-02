"""
Generates assets/overworld_objects/tombstone/sprite.png
Top-down pixel-art weathered stone tombstone with a rounded top and a
faintly etched cross, 32×32 px. Static (single frame).
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/tombstone/sprite.png"

BG          = (0,   0,   0,   0)
STONE       = (150, 150, 158, 255)   # weathered grey
STONE_DARK  = (100, 100, 108, 255)
STONE_HI    = (185, 185, 192, 255)
ETCH        = ( 70,  70,  78, 255)   # carved cross / cracks
MOSS        = ( 95, 130,  80, 255)   # mossy green patch
MOSS_HI     = (130, 165, 100, 255)
GROUND      = ( 85,  70,  55, 255)   # dirt mound at the base
GROUND_HI   = (115,  95,  70, 255)
SHADOW      = (  0,   0,   0,  80)

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# Ground shadow under the stone (oval).
for ox in range(-8, 9):
    px(16 + ox, 28, SHADOW)
for ox in range(-9, 10):
    px(16 + ox, 29, SHADOW)
for ox in range(-7, 8):
    px(16 + ox, 30, SHADOW)

# Dirt mound at the base.
rect(8, 26, 16, 2, GROUND)
rect(9, 25, 14, 1, GROUND_HI)

# Main stone body (rectangular slab y:8..27, x:10..22).
rect(10, 8, 13, 19, STONE)

# Rounded top — chip the upper corners by overdrawing with transparency,
# and add a couple of rounded crown pixels.
def clear(x, y):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), BG)

clear(10, 8); clear(11, 8)
clear(21, 8); clear(22, 8)
clear(10, 9)
clear(22, 9)
# Crown highlight along the new rounded top.
px(12, 8, STONE_HI)
px(13, 8, STONE_HI)
px(14, 8, STONE_HI)
px(15, 8, STONE_HI)
px(16, 8, STONE_HI)
px(17, 8, STONE_HI)
px(18, 8, STONE_HI)
px(19, 8, STONE_HI)
px(20, 8, STONE_HI)
px(11, 9, STONE_HI)
px(21, 9, STONE_HI)

# Left-edge highlight + right-edge shadow on the slab body.
rect(10, 10, 1, 17, STONE_HI)
rect(22, 10, 1, 17, STONE_DARK)
rect(10, 26, 13, 1, STONE_DARK)

# Carved cross etching in the upper-middle of the slab.
# Vertical bar.
rect(16, 12, 1, 8, ETCH)
# Horizontal bar.
rect(13, 15, 7, 1, ETCH)

# Hairline crack running from the cross down to the base.
px(15, 21, ETCH)
px(15, 22, ETCH)
px(14, 23, ETCH)
px(15, 24, ETCH)
px(14, 25, ETCH)

# Moss patches on the lower-left of the slab.
px(11, 22, MOSS)
px(12, 22, MOSS)
px(11, 23, MOSS_HI)
px(12, 23, MOSS)
px(13, 23, MOSS)
px(12, 24, MOSS)
px(13, 24, MOSS_HI)

# Tiny moss specks on the top edge.
px(13, 9, MOSS)
px(19, 10, MOSS)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
