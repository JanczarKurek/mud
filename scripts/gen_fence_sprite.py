"""
Generates assets/overworld_objects/fence/sprite.png

Upright-elevation wooden fence segment per docs/sprite_style.md: 48×40 px
(1.0 × 0.833 tiles), bottom-anchored — the canvas bottom row sits on the
tile's south edge. Two posts with lit top caps and shadowed east sides,
two rails spanning the full canvas width so adjacent fence tiles connect
seamlessly. Deterministic output (no random).
"""

from PIL import Image
import os

W, H = 48, 40
OUT_PATH = "assets/overworld_objects/fence/sprite.png"

BG        = (0,   0,   0,   0)
WOOD      = (160, 100, 40,  255)   # rail mid-brown
WOOD_DARK = (100,  60, 20,  255)   # rail underside / gaps
WOOD_HI   = (200, 140, 70,  255)   # rail top edge
GRAIN     = (130,  78,  28, 255)   # grain dashes
POST      = (120,  72, 24,  255)
POST_DARK = ( 80,  44, 12,  255)   # east (right) shadow side
POST_HI   = (160, 110, 48,  255)   # lit top cap
SHADOW    = (  0,   0,   0,  70)   # ground contact shadow

img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)


# ── Posts (drawn first; rails overlap them) ──────────────────────────────
for post_x in (5, 37):
    rect(post_x, 6, 6, 33, POST)
    # East side shadow (camera is to the south-east, right edge darkened)
    rect(post_x + 4, 6, 2, 33, POST_DARK)
    # Lit top cap
    rect(post_x, 4, 6, 2, POST_HI)
    px(post_x + 5, 4, POST)

# ── Rails (full width so neighbouring fence tiles join) ──────────────────
for rail_y in (12, 24):
    rect(0, rail_y, W, 6, WOOD)
    rect(0, rail_y, W, 1, WOOD_HI)          # lit top edge
    rect(0, rail_y + 5, W, 1, WOOD_DARK)    # shadowed underside
    # Deterministic grain dashes
    for i in range(2, W - 2, 7):
        px(i, rail_y + 2 + (i // 7) % 2, GRAIN)
    # Plank joins
    for jx in (15, 31):
        rect(jx, rail_y + 1, 1, 4, WOOD_DARK)

# ── Post bases + contact shadow ──────────────────────────────────────────
for post_x in (5, 37):
    rect(post_x - 1, 38, 8, 1, POST_DARK)
    rect(post_x - 2, 39, 10, 1, SHADOW)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"wrote {OUT_PATH} ({W}x{H}) -> sprite_width_tiles: {W/48:.3f}, sprite_height_tiles: {H/48:.3f}")
