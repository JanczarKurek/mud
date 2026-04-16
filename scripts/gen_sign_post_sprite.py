"""
Generates assets/overworld_objects/sign_post/sprite.png
Top-down pixel-art wooden sign post with directional board, 32×32 px.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/sign_post/sprite.png"

BG          = (0,   0,   0,   0)
POST        = (110,  65,  20, 255)   # dark brown post
POST_DARK   = ( 70,  38,   8, 255)
POST_HI     = (150,  95,  35, 255)
BOARD       = (200, 155,  70, 255)   # lighter wood board
BOARD_DARK  = (140, 100,  40, 255)
BOARD_HI    = (230, 190, 100, 255)
TEXT_LINE   = ( 80,  50,  15, 255)   # dark burnt-in text lines
SHADOW      = (  0,   0,   0,  60)   # ground shadow

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# ── Ground shadow (ellipse under post) ──────────────────────────────────────
for ox in range(-4, 5):
    px(15 + ox, 28, SHADOW)
    if abs(ox) < 3:
        px(15 + ox, 29, SHADOW)

# ── Post (vertical, x:14-16, y:10-28) ───────────────────────────────────────
rect(14, 10, 4, 19, POST)
rect(14, 10, 1, 19, POST_DARK)   # left shadow
rect(17, 10, 1, 19, POST_DARK)   # right shadow
rect(15, 10, 2,  1, POST_HI)     # top highlight

# ── Sign board (x:5-26, y:5-14) ─────────────────────────────────────────────
rect(5, 5, 22, 10, BOARD)
# Border / frame
rect(5,  5, 22,  1, BOARD_HI)    # top highlight
rect(5, 14, 22,  1, BOARD_DARK)  # bottom shadow
rect(5,  5,  1, 10, BOARD_DARK)  # left shadow
rect(26, 5,  1, 10, BOARD_DARK)  # right shadow

# ── Directional arrow on board (pointing right) ────────────────────────────
# Arrow shaft
rect(8, 9, 10, 2, TEXT_LINE)
# Arrow head
px(18,  7, TEXT_LINE)
px(19,  8, TEXT_LINE)
px(20,  9, TEXT_LINE)
px(20, 10, TEXT_LINE)
px(19, 11, TEXT_LINE)
px(18, 12, TEXT_LINE)

# ── Small text lines (decorative horizontal lines suggesting text) ───────────
rect(8, 7, 8, 1, TEXT_LINE)
rect(8, 12, 8, 1, TEXT_LINE)

# ── Nail dots on board corners ────────────────────────────────────────────────
px( 7,  7, POST_DARK)
px(24,  7, POST_DARK)
px( 7, 12, POST_DARK)
px(24, 12, POST_DARK)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
