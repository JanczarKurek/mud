"""
Generates assets/overworld_objects/side_wall/sprite.png

The side wall is a wall running NORTH–SOUTH, viewed from its east face.
Canvas 48×96 px (sprite_width_tiles=1.0, sprite_height_tiles=2.0).

Geometry (PIL coordinates, anchor at canvas (24, 95) = bottom-center):

  ─ East face: a leaning parallelogram with corners
       (48, 95) — z=0 south-east  (bottom-right)
       (48, 47) — z=0 north-east  (mid-right)
       (24, 23) — z=1 north-east  (upper middle)
       (24, 71) — z=1 south-east  (mid middle)
    The face's vertical edges are vertical in screen; the south and north
    edges slant up-LEFT (24 px left over the wall's z=1 rise).

  ─ Top cap: a thin vertical strip at x=16..24, y=23..71 (8 px wide × 48 tall)
    showing the wall's flat top viewed from camera-above.

A front wall (E–W) would lean the same direction; the difference here is
that the WALL LENGTH runs vertically in screen, so the visible face is a
TALL parallelogram rather than a wide one.
"""

from PIL import Image
import os

W, H = 48, 96
OUT_PATH = "assets/overworld_objects/side_wall/sprite.png"

BG          = (  0,   0,   0,   0)
STONE       = (120, 114, 103, 255)
STONE_HI    = (160, 150, 134, 255)
STONE_DARK  = ( 80,  74,  66, 255)
STONE_VDARK = ( 50,  46,  40, 255)
MORTAR      = ( 55,  50,  44, 255)
CAP_HI      = (190, 178, 160, 255)
CAP_MID     = (148, 138, 124, 255)
CAP_DARK    = ( 88,  82,  72, 255)


img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)


# ── East face: for each column x in [24, 48], fill y in [x-1, x+47]. ────
# Slant: shifting left by 1 px shifts y bounds up by 1 px (matching the
# half-grid floor-offset slope: 24 px left for 24 px up).
FACE_LEFT, FACE_RIGHT = 24, 48
TOP_LEFT, TOP_RIGHT = 16, 24


def face_y_bounds(x):
    """Return (y_top, y_bottom) of the east face at canvas column x."""
    dx = FACE_RIGHT - x          # 0 at the right edge of the face, 24 at the left
    y_top = 47 - dx
    y_bot = 95 - dx
    return y_top, y_bot


# Fill the east face with the base stone colour.
for x in range(FACE_LEFT, FACE_RIGHT):
    y_top, y_bot = face_y_bounds(x)
    for y in range(y_top, y_bot + 1):
        px(x, y, STONE)

# ── Top cap (the wall's flat top, viewed from above-camera) ─────────────
# Thin rectangle sitting just left of the east face's z=1 edge. Bright
# highlight so it reads as a lit top surface.
rect(TOP_LEFT, 23, TOP_RIGHT - TOP_LEFT, 48 + 1, CAP_MID)
# Top-edge brighter row
rect(TOP_LEFT, 23, TOP_RIGHT - TOP_LEFT, 2, CAP_HI)
# Bottom-edge shadow row of cap
rect(TOP_LEFT, 70, TOP_RIGHT - TOP_LEFT, 1, CAP_DARK)
# Left edge shadow
for y in range(23, 72):
    px(TOP_LEFT, y, CAP_DARK)
# Right edge (seam with the east face) — slightly darker to define the corner
for y in range(23, 72):
    px(TOP_RIGHT - 1, y, CAP_MID)


# ── Horizontal mortar courses on the east face ──────────────────────
# Stones stack along the wall length (N–S = vertical in screen), so mortar
# bands appear as roughly horizontal lines crossing the face. With the
# slant, each band is itself slanted up-left by 24 px over the face height.
MORTAR_ROWS_AT_X48 = (59, 71, 83)  # PIL y values at the rightmost column
for base_y in MORTAR_ROWS_AT_X48:
    for x in range(FACE_LEFT, FACE_RIGHT):
        dx = FACE_RIGHT - x
        y = base_y - dx
        y_top, y_bot = face_y_bounds(x)
        if y_top <= y <= y_bot:
            px(x, y, MORTAR)

# Vertical (perpendicular-to-wall) stone separator at the face mid-line so
# we read two visible block columns across the wall's thickness.
for x in range(FACE_LEFT, FACE_RIGHT):
    y_top, y_bot = face_y_bounds(x)
    mid = (y_top + y_bot) // 2
    if (mid - y_top) % 12 != 0:  # skip rows that already carry mortar courses
        # this is a no-op visually; placeholder for future tweak
        pass

# ── Slanted top/bottom edges of the east face (the wall outline) ────────
# Bottom slanted edge: south-side of the face, from BSE (48, 95) to TSE (24, 71).
for i in range(25):
    t = i / 24
    sx = round(48 - 24 * t)
    sy = round(95 - 24 * t)
    px(sx, sy, STONE_VDARK)

# Top slanted edge: north-side, from BNE (48, 47) to TNE (24, 23).
for i in range(25):
    t = i / 24
    sx = round(48 - 24 * t)
    sy = round(47 - 24 * t)
    px(sx, sy, CAP_DARK)

# Vertical right edge (z=0 east edge of the wall) — shadow line.
for y in range(47, 96):
    px(FACE_RIGHT - 1, y, STONE_DARK)
# Subtle highlight just inside the slanted top edge (light catches the
# upper "ridge" of each block).
for i in range(1, 25):
    t = i / 24
    sx = round(48 - 24 * t)
    sy = round(47 - 24 * t)
    if 0 <= sy + 1 < H and 0 <= sx < W:
        px(sx, sy + 1, STONE_HI)


# ── Per-course block edge highlights inside the face ─────────────────
# At each mortar course, highlight the row just below (top of next block).
for base_y in MORTAR_ROWS_AT_X48:
    for x in range(FACE_LEFT + 1, FACE_RIGHT - 1):
        dx = FACE_RIGHT - x
        y = base_y - dx + 1
        y_top, y_bot = face_y_bounds(x)
        if y_top <= y <= y_bot:
            px(x, y, STONE_HI)


os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
