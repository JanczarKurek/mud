"""
Generates assets/overworld_objects/barrel/sprite.png
Side-view wooden barrel as a flat half-tile slab, 48x24 px.
Bottom-anchored — sits on the lower half of its tile. When a chest stacks
on top, the chest renders 24 px above and lines up flush with the barrel's
top hoop.
"""

from PIL import Image
import os

W, H = 48, 24
OUT_PATH = "assets/overworld_objects/barrel/sprite.png"

BG          = (  0,   0,   0,   0)
WOOD        = (134,  83,  42, 255)   # matches debug_color
WOOD_HI     = (180, 124,  68, 255)
WOOD_DARK   = ( 90,  54,  26, 255)
WOOD_VDARK  = ( 60,  36,  18, 255)
HOOP        = ( 70,  62,  52, 255)
HOOP_HI     = (120, 108,  92, 255)
HOOP_DARK   = ( 40,  34,  28, 255)
SHADOW      = (  0,   0,   0,  70)


img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)


# Ground shadow flush with the bottom.
for ox in range(-20, 21):
    rect(24 + ox, H - 1, 1, 1, SHADOW)
for ox in range(-18, 19):
    rect(24 + ox, H - 2, 1, 1, SHADOW)

# ── Barrel silhouette: rounded sides (slightly narrower at top & bottom). ─
# Body spans y=2..22, x=4..43. The "bulge" pushes out to full width at the
# vertical middle (y=11..13).
def stave_x_at(y):
    """Return (x_left, x_right) for the staves at row y so the sides bulge."""
    # parabola peaking at the middle
    mid = (H - 1) / 2.0
    t = (y - mid) / mid  # -1..1
    bulge = int(round((1.0 - t * t) * 2))  # 0..2
    return 5 - bulge, 42 + bulge


for y in range(2, H - 1):
    xl, xr = stave_x_at(y)
    rect(xl, y, xr - xl + 1, 1, WOOD)
    # Outline
    px(xl, y, WOOD_DARK)
    px(xr, y, WOOD_DARK)

# Top rim (slightly darker, narrower)
rect(7, 1, 34, 1, WOOD_DARK)
rect(8, 0, 32, 1, WOOD_VDARK)

# Top wood-grain (highlight just below rim)
rect(8, 2, 32, 1, WOOD_HI)

# ── Iron hoops: top and bottom (and a middle band) ──────────────────────
# Top hoop (near rim)
for y in (4, 5):
    xl, xr = stave_x_at(y)
    rect(xl, y, xr - xl + 1, 1, HOOP)
    px(xl, y, HOOP_DARK)
    px(xr, y, HOOP_DARK)
# Highlight on top hoop
for x in range(8, 40, 4):
    px(x, 4, HOOP_HI)

# Middle hoop
for y in (11, 12):
    xl, xr = stave_x_at(y)
    rect(xl, y, xr - xl + 1, 1, HOOP)
    px(xl, y, HOOP_DARK)
    px(xr, y, HOOP_DARK)
for x in range(8, 40, 4):
    px(x, 11, HOOP_HI)

# Bottom hoop
for y in (18, 19):
    xl, xr = stave_x_at(y)
    rect(xl, y, xr - xl + 1, 1, HOOP)
    px(xl, y, HOOP_DARK)
    px(xr, y, HOOP_DARK)
for x in range(8, 40, 4):
    px(x, 18, HOOP_HI)

# ── Stave separator lines (vertical wood grain between hoops) ──────────
for sx in (12, 18, 24, 30, 36):
    for sy in (3,):
        px(sx, sy, WOOD_DARK)
    for sy in (6, 7, 8, 9, 10):
        px(sx, sy, WOOD_DARK)
    for sy in (13, 14, 15, 16, 17):
        px(sx, sy, WOOD_DARK)
    for sy in (20, 21):
        px(sx, sy, WOOD_DARK)

# Left-edge shadow column for some depth
for y in range(3, 22):
    xl, _ = stave_x_at(y)
    px(xl + 1, y, WOOD_DARK)

# Right-edge highlight column
for y in range(3, 22):
    _, xr = stave_x_at(y)
    px(xr - 1, y, WOOD_HI)


os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
