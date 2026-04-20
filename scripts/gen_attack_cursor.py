"""
Generates assets/cursors/attack_cursor.png
32x32 pixel-art sword cursor, hotspot at (0, 0) top-left.
The blade tip sits at the hotspot so clicking picks the tile under the tip.
"""

from PIL import Image

W, H = 32, 32
OUT_PATH = "assets/cursors/attack_cursor.png"

BG           = (0, 0, 0, 0)
BLADE        = (216, 218, 224, 255)   # polished steel
BLADE_HI     = (244, 246, 250, 255)   # highlight along blade edge
BLADE_SHADE  = (162, 166, 176, 255)   # shaded edge
OUTLINE      = ( 28,  30,  36, 255)   # near-black outline
GUARD        = (198, 162,  72, 255)   # brass cross-guard
GUARD_HI     = (236, 206, 120, 255)
GUARD_DARK   = (132, 104,  36, 255)
GRIP         = ( 80,  48,  28, 255)   # leather-wrapped grip
GRIP_DARK    = ( 48,  28,  16, 255)
POMMEL       = (198, 162,  72, 255)
BLOOD        = (176,  28,  24, 255)   # red accent drops near tip


img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


# Diagonal sword from upper-left tip (hotspot 0,0) down to lower-right hilt.
# Blade runs along the main diagonal; we draw it two pixels wide with
# lighter edge on top-right and darker edge on bottom-left, plus an outline.

def put_blade_segment(i):
    """Draw one diagonal blade segment i pixels from the tip (0,0)."""
    # Center pixel on the diagonal
    x = i
    y = i
    # Outline on the outer corners
    px(x + 1, y - 1, OUTLINE)
    px(x - 1, y + 1, OUTLINE)
    # Highlight edge (upper-right of diagonal)
    px(x + 1, y, BLADE_HI)
    # Core blade
    px(x, y, BLADE)
    # Shaded edge (lower-left of diagonal)
    px(x, y + 1, BLADE_SHADE)


# Blade length: from (0,0) to (17,17). Tip is the hotspot.
BLADE_LENGTH = 18
for i in range(BLADE_LENGTH):
    put_blade_segment(i)

# Sharp tip outline
px(0, 0, BLADE_HI)
px(1, 0, OUTLINE)
px(0, 1, OUTLINE)

# Tiny blood drop near the tip for attack flavor
px(3, 2, BLOOD)
px(2, 3, BLOOD)

# Cross-guard: a short diagonal perpendicular to the blade around (18,18).
# Perpendicular direction is (+1, -1) / (-1, +1). Draw guard across that axis.
GX, GY = 18, 18  # guard center
for t in range(-3, 4):
    gx = GX + t
    gy = GY - t
    # outline
    px(gx, gy - 1, OUTLINE)
    px(gx + 1, gy, OUTLINE)
    # guard body
    if t == 0:
        px(gx, gy, GUARD_HI)
    elif abs(t) == 3:
        px(gx, gy, GUARD_DARK)
    else:
        px(gx, gy, GUARD)

# Grip: diagonal from (20,20) to (27,27), two px wide.
for i in range(7):
    x = 20 + i
    y = 20 + i
    # outline
    px(x + 1, y - 1, OUTLINE)
    px(x - 1, y + 1, OUTLINE)
    # shaded then base
    px(x, y, GRIP if i % 2 == 0 else GRIP_DARK)
    px(x + 1, y, GRIP_DARK if i % 2 == 0 else GRIP)

# Pommel at (28,28) — small round cap
for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)]:
    px(28 + dx, 28 + dy, POMMEL)
# pommel outline
px(27, 28, OUTLINE)
px(28, 27, OUTLINE)
px(30, 28, OUTLINE)
px(28, 30, OUTLINE)
px(30, 29, OUTLINE)
px(29, 30, OUTLINE)
px(29, 29, GUARD_HI)


img.save(OUT_PATH)
print(f"wrote {OUT_PATH} ({W}x{H})")
