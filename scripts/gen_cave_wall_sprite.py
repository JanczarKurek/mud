"""
Generates assets/overworld_objects/cave_wall/sprite.png
Sheared jagged bedrock — same up-LEFT lean as the hewn wall (96×48 canvas,
matches half-grid floor offset), but with a rough irregular silhouette and
cracks instead of clean mortar courses.
"""

from PIL import Image
import os

W, H = 96, 48
OUT_PATH = "assets/overworld_objects/cave_wall/sprite.png"

BG          = (  0,   0,   0,   0)
ROCK        = ( 93,  81,  72, 255)
ROCK_HI     = (130, 114, 100, 255)
ROCK_DARK   = ( 60,  52,  46, 255)
ROCK_VDARK  = ( 36,  30,  26, 255)
CRACK       = ( 28,  22,  18, 255)
CAP_HI      = (148, 130, 114, 255)
MOSS        = ( 78,  98,  62, 255)
MOSS_DARK   = ( 54,  68,  42, 255)


img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)


FOOT_LEFT, FOOT_RIGHT = 24, 72
CAP_LEFT, CAP_RIGHT = 0, 48


def edges_at(y):
    t = y / max(H - 1, 1)
    left = round(CAP_LEFT + (FOOT_LEFT - CAP_LEFT) * t)
    right = round(CAP_RIGHT + (FOOT_RIGHT - CAP_RIGHT) * t)
    return left, right


# Random-but-deterministic jitter on the silhouette so the edges look bitten
# rather than ruler-straight.
JITTER_LEFT  = [0, -1, 0, 0, 1, 0, -1, 0, 1, 0, 0, -1, 0, 1, 0, 0,
                -1, 0, 1, 0, 0, 1, -1, 0, 0, 1, 0, -1, 0, 0, 1, 0,
                0, -1, 0, 1, 0, 0, -1, 0, 1, 0, -1, 0, 0, 1, 0, -1]
JITTER_RIGHT = [0, 1, 0, -1, 0, 1, 0, 0, -1, 0, 1, 0, 0, -1, 0, 1,
                0, 0, 1, -1, 0, 0, 1, 0, -1, 0, 1, 0, 0, -1, 0, 0,
                1, 0, -1, 0, 1, 0, 0, -1, 0, 1, 0, -1, 0, 1, 0, 0]


# Fill body with the slanted rock silhouette.
for y in range(H):
    l, r = edges_at(y)
    l += JITTER_LEFT[y % len(JITTER_LEFT)]
    r += JITTER_RIGHT[y % len(JITTER_RIGHT)]
    rect(l, y, max(r - l, 1), 1, ROCK)

# Top cap: a couple of brighter rows so the cave wall has a definite "top".
for y in range(0, 4):
    l, r = edges_at(y)
    l += JITTER_LEFT[y % len(JITTER_LEFT)]
    r += JITTER_RIGHT[y % len(JITTER_RIGHT)]
    rect(l, y, max(r - l, 1), 1, CAP_HI)

# Light-side highlight along the (camera-facing) left slanted edge.
for y in range(4, H):
    l, _ = edges_at(y)
    l += JITTER_LEFT[y % len(JITTER_LEFT)]
    px(l, y, ROCK_HI)

# Shadow on the back/right slanted edge.
for y in range(4, H):
    _, r = edges_at(y)
    r += JITTER_RIGHT[y % len(JITTER_RIGHT)]
    px(r - 1, y, ROCK_DARK)
    px(r - 2, y, ROCK_DARK)

# ── Cracks: a few diagonal fissures through the body ─────────────────
def x_at_y(frac, y):
    l, r = edges_at(y)
    return round(l + (r - l) * frac)

cracks = [
    [(0.20, 6), (0.18, 18), (0.22, 30), (0.20, 44)],
    [(0.50, 5), (0.48, 16), (0.52, 28), (0.50, 42)],
    [(0.78, 8), (0.80, 22), (0.76, 36), (0.78, 46)],
]
for crack in cracks:
    for i in range(len(crack) - 1):
        f0, y0 = crack[i]
        f1, y1 = crack[i + 1]
        x0 = x_at_y(f0, y0)
        x1 = x_at_y(f1, y1)
        steps = max(abs(x1 - x0), abs(y1 - y0))
        for s in range(steps + 1):
            t = s / max(steps, 1)
            px(round(x0 + (x1 - x0) * t), round(y0 + (y1 - y0) * t), CRACK)

# ── Small boulder clusters (dark dimples) for surface texture ───────
def boulder(cx, cy, rx, ry):
    for y in range(cy - ry, cy + ry + 1):
        for x in range(cx - rx, cx + rx + 1):
            dx = (x - cx) / max(rx, 1)
            dy = (y - cy) / max(ry, 1)
            if dx * dx + dy * dy <= 1.0:
                px(x, y, ROCK_DARK)

boulder(18, 18, 4, 2)
boulder(40, 14, 5, 2)
boulder(58, 24, 4, 2)
boulder(28, 32, 4, 2)
boulder(50, 36, 5, 2)
boulder(66, 40, 4, 2)

# Moss tufts on the top edge.
for (mx, my) in [(8, 2), (9, 3), (22, 1), (23, 2), (38, 3), (44, 2)]:
    px(mx, my, MOSS)
    px(mx, my + 1, MOSS_DARK)

# Bottom seam (very dark line where the rock meets the tile floor).
l, r = edges_at(H - 1)
rect(l, H - 1, max(r - l, 1), 1, ROCK_VDARK)


os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
