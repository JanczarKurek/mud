"""
Generates assets/overworld_objects/wall/sprite.png

The wall is drawn as a sheared slab leaning up-LEFT to match the half-grid
floor offset (-24 x, +24 y per floor). Canvas is 96×48 (2 tiles wide × 1
tile tall) with bottom-center anchor:

    ┌──── canvas (96 wide) ────────┐
    │ [   TOP CAP at x=0..48   ]   │   ← upper-left: where floor +1 lands
    │      ╲╲╲ sheared face ╲╲╲    │
    │       [ FOOTPRINT at x=24..72]│  ← lower-center: sits on the tile
    └──────────────────────────────┘

Stacking a wall on the same column at floor +1 then renders 24 px up-left,
landing flush on top of this wall's top cap.
"""

from PIL import Image
import os

W, H = 96, 48
OUT_PATH = "assets/overworld_objects/wall/sprite.png"

BG          = (  0,   0,   0,   0)
STONE       = (120, 114, 103, 255)   # base hewn stone (matches debug_color)
STONE_HI    = (160, 150, 134, 255)
STONE_DARK  = ( 80,  74,  66, 255)
STONE_VDARK = ( 50,  46,  40, 255)
MORTAR      = ( 55,  50,  44, 255)
CAP_HI      = (180, 170, 154, 255)   # bright lit cap surface
CAP_MID     = (140, 132, 118, 255)


img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)


# Geometry:
#   Footprint (the wall's base on the tile): x=24..72 at the BOTTOM half.
#   Top cap (where floor+1 renders): x=0..48 at the TOP, shifted 24 px LEFT.
#   For each row y, the wall body interpolates linearly between the two.
FOOT_LEFT, FOOT_RIGHT = 24, 72       # x bounds at y=H-1 (bottom row)
CAP_LEFT, CAP_RIGHT = 0, 48          # x bounds at y=0 (top row)


def edges_at(y):
    """Linear lerp between top cap edges (y=0) and footprint edges (y=H-1)."""
    t = y / max(H - 1, 1)
    left = round(CAP_LEFT + (FOOT_LEFT - CAP_LEFT) * t)
    right = round(CAP_RIGHT + (FOOT_RIGHT - CAP_RIGHT) * t)
    return left, right


# ── Fill the slanted wall body with stone ───────────────────────────────
for y in range(H):
    l, r = edges_at(y)
    rect(l, y, r - l, 1, STONE)

# ── Top cap: a bright lit horizontal band at y=0..6 to read as "top of wall" ──
for y in range(0, 6):
    l, r = edges_at(y)
    rect(l, y, r - l, 1, CAP_HI)
# Top cap shadow line just under the bright cap
for y in (6,):
    l, r = edges_at(y)
    rect(l, y, r - l, 1, CAP_MID)

# ── Mortar courses (horizontal) — three layers between top cap and bottom ──
for cy in (16, 28, 40):
    l, r = edges_at(cy)
    rect(l, cy, r - l, 1, MORTAR)

# ── Vertical block divisions: place mortar at fractional x positions so the
#    divisions track the slant of the wall.
def x_at_y(frac, y):
    """At row y, return the x position for a fractional column across the
    slanted wall (frac 0..1 from left edge to right edge)."""
    l, r = edges_at(y)
    return round(l + (r - l) * frac)

for y in range(H):
    if 6 < y < H - 1:
        for frac in (0.33, 0.66):
            xpos = x_at_y(frac, y)
            # Only draw mortar between courses (not on the course row)
            if y in (16, 28, 40):
                continue
            px(xpos, y, MORTAR)

# ── Slanted edge highlights/shadows ──────────────────────────────────────
# Light side (front face): the LEFT slanted edge catches more light because
# the camera is "up-left". Drop a 1-px highlight just inside the left edge.
for y in range(7, H):
    l, _ = edges_at(y)
    px(l, y, STONE_HI)
    px(l + 1, y, STONE)

# Shadow inside the right slanted edge (the back side of the wall).
for y in range(7, H):
    _, r = edges_at(y)
    px(r - 1, y, STONE_DARK)
    px(r - 2, y, STONE_DARK)

# Bottom seam (the wall sitting on the tile)
l, r = edges_at(H - 1)
rect(l, H - 1, r - l, 1, STONE_VDARK)

# ── Block-by-block edge highlights inside each course ─────────────────
def cell_for(course_top, course_bottom):
    """Return list of (x0, y0, x1, y1) cells in a course, split at frac
    columns. Each cell gets a top-left highlight + bottom-right shadow."""
    cells = []
    for f0, f1 in [(0.0, 0.33), (0.33, 0.66), (0.66, 1.0)]:
        # Sample at top and bottom of the course to compute the cell quad.
        xt0 = x_at_y(f0, course_top)
        xt1 = x_at_y(f1, course_top)
        xb0 = x_at_y(f0, course_bottom)
        xb1 = x_at_y(f1, course_bottom)
        cells.append((xt0, course_top, xt1, xb0, xb1, course_bottom))
    return cells


for (top_y, bot_y) in [(7, 15), (17, 27), (29, 39), (41, H - 2)]:
    for (xt0, ty, xt1, xb0, xb1, by) in cell_for(top_y, bot_y):
        # Top edge highlight (along the slanted top row of the block)
        rect(xt0, ty, xt1 - xt0, 1, STONE_HI)
        # Left edge highlight: 1 px diagonal from (xt0, ty) toward (xb0, by)
        h = by - ty
        for i in range(h + 1):
            t = i / max(h, 1)
            x = round(xt0 + (xb0 - xt0) * t)
            px(x, ty + i, STONE_HI)
        # Bottom edge shadow
        rect(xb0, by, xb1 - xb0, 1, STONE_DARK)
        # Right edge shadow
        for i in range(h + 1):
            t = i / max(h, 1)
            x = round(xt1 + (xb1 - xt1) * t) - 1
            px(x, ty + i, STONE_DARK)


os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
