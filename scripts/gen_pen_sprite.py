"""
Generates assets/overworld_objects/pen/sprite.png
Top-down pixel-art quill pen with a feathered shaft and ink-stained
nib, 32×32 px. Static (single frame).
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/pen/sprite.png"

BG          = (0,   0,   0,   0)
FEATHER     = (230, 215, 175, 255)   # creamy quill feather
FEATHER_HI  = (250, 240, 210, 255)
FEATHER_DK  = (170, 150, 105, 255)
SHAFT       = (180, 145,  90, 255)   # bone shaft below the vanes
SHAFT_DK    = (120,  90,  50, 255)
NIB         = ( 90,  70,  45, 255)   # bronze nib
NIB_HI      = (180, 150,  90, 255)
INK         = ( 25,  25,  55, 255)   # blue-black ink at the tip
SHADOW      = (  0,   0,   0,  55)

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

# Soft diagonal shadow underneath the quill, offset down-right.
for i in range(-9, 10):
    px(17 + i, 24 + (-i // 3), SHADOW)
    px(17 + i, 25 + (-i // 3), SHADOW)

# The pen runs diagonally from top-left (feather tip) to bottom-right
# (nib + ink). We draw it in segments along that diagonal.

# Feather vanes (broad fan along the top half of the shaft).
# Start at (8, 4) top-left, ending around (19, 17) before the bare shaft.
feather_pts = [
    # (x, y, width perpendicular to diagonal)
    (8,  4, 3),
    (9,  5, 4),
    (10, 6, 5),
    (11, 7, 5),
    (12, 8, 6),
    (13, 9, 6),
    (14,10, 6),
    (15,11, 5),
    (16,12, 5),
    (17,13, 4),
    (18,14, 4),
    (19,15, 3),
]
for (cx, cy, w) in feather_pts:
    # Perpendicular spread: roughly horizontal feather barbs.
    for dx in range(-w, w + 1):
        x = cx + dx
        y = cy + (dx // 4)  # slight droop on outer barbs
        # Choose color by distance from the spine for shading.
        if dx == 0:
            c = FEATHER_DK            # central rachis line
        elif abs(dx) <= 1:
            c = FEATHER_HI
        elif abs(dx) <= w - 1:
            c = FEATHER
        else:
            c = FEATHER_DK            # outer barb tip
        px(x, y, c)

# Bare shaft (bone) from end of feathers to the nib.
shaft_pts = [(20, 16), (21, 17), (22, 18), (23, 19), (24, 20)]
for (x, y) in shaft_pts:
    px(x - 1, y - 1, SHAFT_DK)
    px(x,     y,     SHAFT)
    px(x + 1, y,     SHAFT)
    px(x,     y + 1, SHAFT_DK)

# Metal nib — a small wedge angled the same direction.
px(25, 21, NIB_HI)
px(26, 22, NIB)
px(25, 22, NIB)
px(26, 23, NIB)
px(27, 23, NIB)
px(27, 24, NIB)

# Ink drop pooling at the very tip.
px(28, 24, INK)
px(28, 25, INK)
px(27, 25, INK)
# Tiny splatter dot just past the nib.
px(29, 26, INK)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
