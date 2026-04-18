"""
Generates assets/overworld_objects/generic_corpse/sprite.png
A top-down skull-and-crossbones corpse marker, 32×32 px.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_PATH = "assets/overworld_objects/generic_corpse/sprite.png"

BG         = (0,   0,   0,   0)
BONE       = (215, 200, 165, 255)   # warm cream bone
BONE_HI    = (235, 225, 195, 255)   # highlight
BONE_DARK  = (145, 128,  90, 255)   # shadow / edge
SOCKET     = ( 30,  20,  10, 255)   # dark eye / nose sockets
TOOTH      = (200, 190, 155, 255)   # tooth fill (slightly darker than bone)

img = Image.new("RGBA", (W, H), BG)

def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        img.putpixel((x, y), c)

def rect(x, y, w, h, c):
    for dy in range(h):
        for dx in range(w):
            px(x + dx, y + dy, c)

# ── Skull ──────────────────────────────────────────────────────────────────────
# Roughly oval: 14 wide × 11 tall, centered at (15, 9)
skull_cx, skull_cy = 15, 9

# Draw skull using ellipse-ish coverage
skull_pixels = [
    # top arc row
    (10,4),(11,3),(12,3),(13,2),(14,2),(15,2),(16,2),(17,2),(18,2),(19,3),(20,3),(21,4),
    # row 5
    (9,5),(10,5),(11,5),(12,4),(13,4),(14,3),(15,3),(16,3),(17,3),(18,4),(19,4),(20,5),(21,5),(22,5),
    # rows 6-7 (wide middle)
    (9,6),(10,6),(11,6),(12,6),(13,5),(14,5),(15,5),(16,5),(17,5),(18,5),(19,5),(20,6),(21,6),(22,6),
    (8,7),(9,7),(10,7),(11,7),(12,7),(13,7),(14,7),(15,7),(16,7),(17,7),(18,7),(19,7),(20,7),(21,7),(22,7),(23,7),
    # rows 8-9
    (8,8),(9,8),(10,8),(11,8),(12,8),(13,8),(14,8),(15,8),(16,8),(17,8),(18,8),(19,8),(20,8),(21,8),(22,8),(23,8),
    (8,9),(9,9),(10,9),(11,9),(12,9),(13,9),(14,9),(15,9),(16,9),(17,9),(18,9),(19,9),(20,9),(21,9),(22,9),(23,9),
    # rows 10-11
    (8,10),(9,10),(10,10),(11,10),(12,10),(13,10),(14,10),(15,10),(16,10),(17,10),(18,10),(19,10),(20,10),(21,10),(22,10),(23,10),
    (9,11),(10,11),(11,11),(12,11),(13,11),(14,11),(15,11),(16,11),(17,11),(18,11),(19,11),(20,11),(21,11),(22,11),
    # row 12 (cheekbones narrow)
    (10,12),(11,12),(12,12),(13,12),(14,12),(15,12),(16,12),(17,12),(18,12),(19,12),(20,12),(21,12),
    # row 13 (jaw)
    (11,13),(12,13),(13,13),(14,13),(15,13),(16,13),(17,13),(18,13),(19,13),(20,13),
]

for (sx, sy) in skull_pixels:
    px(sx, sy, BONE)

# Highlights on top-left
for (sx, sy) in [(12,3),(13,3),(14,3),(11,4),(12,4),(13,4),(10,5),(11,5),(12,5)]:
    px(sx, sy, BONE_HI)

# Shadow on right edge
for (sx, sy) in [(22,7),(23,7),(22,8),(23,8),(22,9),(23,9),(22,10),(23,10),(21,11),(22,11),(21,12),(20,12)]:
    px(sx, sy, BONE_DARK)

# ── Eye sockets ────────────────────────────────────────────────────────────────
for (sx, sy) in [(11,7),(12,7),(11,8),(12,8),(11,9),(12,9)]:
    px(sx, sy, SOCKET)
for (sx, sy) in [(19,7),(20,7),(19,8),(20,8),(19,9),(20,9)]:
    px(sx, sy, SOCKET)

# ── Nose cavity ────────────────────────────────────────────────────────────────
for (sx, sy) in [(14,11),(15,11),(16,11),(14,12),(15,12),(16,12)]:
    px(sx, sy, SOCKET)

# ── Teeth ──────────────────────────────────────────────────────────────────────
# Jaw row teeth
for (sx, sy) in [(11,13),(13,13),(15,13),(17,13),(19,13)]:
    px(sx, sy, TOOTH)
for (sx, sy) in [(12,13),(14,13),(16,13),(18,13),(20,13)]:
    px(sx, sy, SOCKET)

# ── Crossed Bones ──────────────────────────────────────────────────────────────
# Two diagonal bone shafts crossing at center (15, 22)
# Each shaft ~3px wide. Diagonal from top-left to bottom-right and vice-versa.

def draw_bone_shaft(x0, y0, x1, y1):
    """Draw a thick diagonal line (Bresenham-ish, 3 px wide) in BONE color."""
    dx = x1 - x0
    dy = y1 - y0
    steps = max(abs(dx), abs(dy))
    for i in range(steps + 1):
        t = i / max(steps, 1)
        cx = round(x0 + t * dx)
        cy = round(y0 + t * dy)
        # 3px perpendicular thickness
        px(cx,   cy,   BONE)
        px(cx+1, cy,   BONE)
        px(cx,   cy+1, BONE)
        px(cx-1, cy,   BONE_DARK)
        px(cx,   cy-1, BONE_DARK)
        px(cx+1, cy+1, BONE_HI)

# Bone 1: top-left to bottom-right
draw_bone_shaft(4, 16, 27, 30)
# Bone 2: top-right to bottom-left
draw_bone_shaft(27, 16, 4, 30)

# Bone knuckle bumps at endpoints
def bone_knuckle(cx, cy):
    for (ox, oy) in [(-1,0),(1,0),(0,-1),(0,1),(0,0),(-1,-1),(1,1)]:
        px(cx+ox, cy+oy, BONE)
    px(cx, cy, BONE_HI)

bone_knuckle(4,  16)
bone_knuckle(27, 16)
bone_knuckle(4,  30)
bone_knuckle(27, 30)

# Re-draw skull on top of bones (skull is in front)
for (sx, sy) in skull_pixels:
    px(sx, sy, BONE)
for (sx, sy) in [(12,3),(13,3),(14,3),(11,4),(12,4),(13,4),(10,5),(11,5),(12,5)]:
    px(sx, sy, BONE_HI)
for (sx, sy) in [(22,7),(23,7),(22,8),(23,8),(22,9),(23,9),(22,10),(23,10),(21,11),(22,11),(21,12),(20,12)]:
    px(sx, sy, BONE_DARK)
for (sx, sy) in [(11,7),(12,7),(11,8),(12,8),(11,9),(12,9),(19,7),(20,7),(19,8),(20,8),(19,9),(20,9)]:
    px(sx, sy, SOCKET)
for (sx, sy) in [(14,11),(15,11),(16,11),(14,12),(15,12),(16,12)]:
    px(sx, sy, SOCKET)
for (sx, sy) in [(11,13),(13,13),(15,13),(17,13),(19,13)]:
    px(sx, sy, TOOTH)
for (sx, sy) in [(12,13),(14,13),(16,13),(18,13),(20,13)]:
    px(sx, sy, SOCKET)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
