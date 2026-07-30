"""
Generates assets/overworld_objects/tombstone/sprite.png

Upright-elevation weathered headstone per docs/sprite_style.md: 32×40 px
(0.667 × 0.833 tiles), bottom-anchored. Rounded-top slab seen from the
south with a 3 px shadowed east (right) side for visible thickness, etched
cross, moss at the base, and a small dirt mound. Deterministic output.
"""

from PIL import Image
import os

W, H = 32, 40
OUT_PATH = "assets/overworld_objects/tombstone/sprite.png"

BG          = (0,   0,   0,   0)
STONE       = (150, 150, 158, 255)   # weathered grey front face
STONE_DARK  = (100, 100, 108, 255)   # east side / crevices
STONE_VDARK = ( 76,  76,  84, 255)   # deepest shadow line
STONE_HI    = (185, 185, 192, 255)   # lit top arc
ETCH        = ( 70,  70,  78, 255)   # carved cross / cracks
MOSS        = ( 95, 130,  80, 255)
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


# Slab silhouette: x 5..26 body, rounded top between y 3 and y 9.
SLAB_L, SLAB_R = 5, 26        # inclusive front-face bounds incl. side
SIDE_W = 3                    # shadowed east-side thickness
TOP_Y, BASE_Y = 3, 36

# Half-widths of the rounded top per row (distance from slab centre 15.5).
ROUND = {3: 4, 4: 6, 5: 8, 6: 9, 7: 10, 8: 10}

for y in range(TOP_Y, BASE_Y + 1):
    half = ROUND.get(y, 11)
    x0 = 16 - half
    x1 = 15 + half
    rect(x0, y, x1 - x0 + 1, 1, STONE)

# East-side thickness: darken the rightmost SIDE_W columns of each row.
for y in range(TOP_Y, BASE_Y + 1):
    half = ROUND.get(y, 11)
    x1 = 15 + half
    for i in range(SIDE_W):
        px(x1 - i, y, STONE_DARK if i > 0 else STONE_VDARK)

# Lit top arc (upper-left rim).
for y in range(TOP_Y, 9):
    half = ROUND.get(y, 11)
    x0 = 16 - half
    px(x0, y, STONE_HI)
    px(x0 + 1, y, STONE_HI)
for x in range(12, 20):
    px(x, TOP_Y, STONE_HI)

# Etched cross.
rect(14, 10, 2, 12, ETCH)
rect(10, 13, 10, 2, ETCH)
# Weathering cracks (deterministic).
px(8, 20, STONE_DARK)
px(9, 21, STONE_DARK)
px(20, 26, STONE_DARK)
px(21, 27, STONE_DARK)
px(22, 28, STONE_DARK)

# Moss at the base of the front face.
for (mx, my) in [(7, 32), (8, 32), (8, 33), (9, 33), (18, 34), (19, 34),
                 (12, 33), (13, 34), (20, 33)]:
    px(mx, my, MOSS)
px(8, 31, MOSS_HI)
px(19, 33, MOSS_HI)

# Dirt mound + contact shadow on the ground.
rect(4, 36, 24, 2, GROUND)
rect(6, 36, 8, 1, GROUND_HI)
rect(3, 38, 26, 1, SHADOW)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"wrote {OUT_PATH} ({W}x{H}) -> sprite_width_tiles: {W/48:.3f}, sprite_height_tiles: {H/48:.3f}")
