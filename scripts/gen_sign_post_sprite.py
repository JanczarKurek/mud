"""
Generates assets/overworld_objects/sign_post/sprite.png

Upright-elevation wooden sign post per docs/sprite_style.md: 40×52 px
(0.833 × 1.083 tiles), bottom-anchored. South-facing board on a post with
lit top edges, shadowed east (right) sides, burnt-in text lines, and a
ground contact shadow. Deterministic output.
"""

from PIL import Image
import os

W, H = 40, 52
OUT_PATH = "assets/overworld_objects/sign_post/sprite.png"

BG          = (0,   0,   0,   0)
POST        = (110,  65,  20, 255)
POST_DARK   = ( 70,  38,   8, 255)   # east shadow side
POST_HI     = (150,  95,  35, 255)   # lit cap / west catch-light
BOARD       = (200, 155,  70, 255)
BOARD_DARK  = (140, 100,  40, 255)   # board underside / east edge
BOARD_HI    = (230, 190, 100, 255)   # lit top edge
TEXT_LINE   = ( 80,  50,  15, 255)
NAIL        = ( 90,  90, 100, 255)
SHADOW      = (  0,   0,   0,  60)

img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)


# ── Post (x 17..22, from cap down to the ground) ─────────────────────────
rect(17, 6, 6, 45, POST)
rect(21, 6, 2, 45, POST_DARK)      # east shadow side
rect(17, 4, 6, 2, POST_HI)         # lit top cap
for y in range(8, 50, 9):          # deterministic grain nicks
    px(18, y, POST_DARK)

# ── Board (x 3..36, y 10..25), nailed to the post front ──────────────────
rect(3, 10, 34, 16, BOARD)
rect(3, 10, 34, 1, BOARD_HI)       # lit top edge
rect(3, 25, 34, 1, BOARD_DARK)     # shadowed underside
rect(35, 11, 2, 14, BOARD_DARK)    # east end grain
rect(3, 11, 1, 14, BOARD_HI)       # west catch-light
# Burnt-in text lines.
for ty in (14, 18, 22):
    rect(7, ty, 22 if ty != 18 else 17, 2, TEXT_LINE)
# Nails into the post.
px(19, 12, NAIL)
px(20, 23, NAIL)

# ── Base + contact shadow ────────────────────────────────────────────────
rect(16, 49, 8, 1, POST_DARK)
rect(14, 51, 12, 1, SHADOW)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"wrote {OUT_PATH} ({W}x{H}) -> sprite_width_tiles: {W/48:.3f}, sprite_height_tiles: {H/48:.3f}")
