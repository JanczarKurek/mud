"""
Generates assets/overworld_objects/goblin/sheet.png
Sheet layout: 4 columns × 2 rows, each frame 32×48 px
  Row 0: idle (4 frames, subtle breathing bob)
  Row 1: walk (4 frames, leg stride cycle)
"""

from PIL import Image, ImageDraw
import os

FRAME_W = 32
FRAME_H = 48
COLS = 4
ROWS = 2
OUT_PATH = "assets/overworld_objects/goblin/sheet.png"

# ── Palette ────────────────────────────────────────────────────────────────────
# Sampled / inspired by the existing sprite.png
BG         = (0, 0, 0, 0)        # transparent
SKIN       = (92,  140,  52, 255) # goblin green skin
SKIN_DARK  = (56,   96,  28, 255) # shadow
SKIN_HI    = (124, 180,  72, 255) # highlight
EYE        = (255, 220,  30, 255) # yellow eyes
PUPIL      = (20,   20,  20, 255) # pupils
MOUTH      = (40,   20,  10, 255) # dark mouth
TUNIC      = (80,   50,  20, 255) # brown tunic
TUNIC_DARK = (50,   28,  10, 255)
BELT       = (120,  80,  20, 255)
PANTS      = (56,   36,  12, 255)
BOOT       = (40,   25,   8, 255)
BOOT_HI    = (60,   40,  14, 255)
TOOTH      = (240, 230, 180, 255)
CLAW       = (200, 200, 120, 255)

def make_frame(body_dy=0, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0, blink=False):
    """
    Draw one 32×48 goblin frame.
    body_dy     – vertical offset for torso (breathing bob)
    l/r_foot_dy – vertical offset for each foot (walk)
    l/r_arm_dy  – vertical offset for each arm swing
    blink       – whether to draw closed eyes
    """
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    d   = ImageDraw.Draw(img)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c, dy=0):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry + dy, c)

    # ── Feet / boots ──────────────────────────────────────────────────────────
    # Left boot (x=10..13, y=38+)
    lby = 38 + l_foot_dy
    rect(10, lby,     4, 6, BOOT,    0)
    rect(10, lby,     4, 1, BOOT_HI, 0)
    rect(9,  lby+2,   1, 3, BOOT,    0)   # ankle shadow

    # Right boot (x=18..21, y=38+)
    rby = 38 + r_foot_dy
    rect(18, rby,     4, 6, BOOT,    0)
    rect(18, rby,     4, 1, BOOT_HI, 0)
    rect(22, rby+2,   1, 3, BOOT,    0)

    # ── Pants ─────────────────────────────────────────────────────────────────
    bd = body_dy
    # Left leg
    rect(10, 30+bd, 4, 9, PANTS, 0)
    # Right leg
    rect(18, 30+bd, 4, 9, PANTS, 0)
    # Crotch join
    rect(14, 30+bd, 4, 4, PANTS, 0)

    # ── Belt ──────────────────────────────────────────────────────────────────
    rect(9, 28+bd, 14, 3, BELT, 0)

    # ── Tunic torso ───────────────────────────────────────────────────────────
    rect(9, 18+bd,  14, 11, TUNIC,      0)
    rect(9, 18+bd,   1, 11, TUNIC_DARK, 0)   # left shadow
    rect(22,18+bd,   1, 11, TUNIC_DARK, 0)   # right shadow

    # ── Left arm ──────────────────────────────────────────────────────────────
    lad = l_arm_dy
    rect(7,  20+bd+lad, 3, 8, TUNIC,     0)
    rect(7,  28+bd+lad, 3, 3, SKIN,      0)   # forearm skin
    rect(7,  31+bd+lad, 3, 2, CLAW,      0)   # claw tip
    px(  6,  29+bd+lad,    SKIN_DARK)          # arm shadow

    # ── Right arm ─────────────────────────────────────────────────────────────
    rad = r_arm_dy
    rect(22, 20+bd+rad, 3, 8, TUNIC,     0)
    rect(22, 28+bd+rad, 3, 3, SKIN,      0)
    rect(22, 31+bd+rad, 3, 2, CLAW,      0)
    px(  25, 29+bd+rad,    SKIN_DARK)

    # ── Neck ──────────────────────────────────────────────────────────────────
    rect(14, 15+bd, 4, 4, SKIN, 0)

    # ── Head ──────────────────────────────────────────────────────────────────
    hx, hy = 8, 4+bd
    rect(hx,   hy,   16, 14, SKIN,      0)
    rect(hx,   hy,    1, 14, SKIN_DARK, 0)   # left shadow
    rect(hx+15,hy,    1, 14, SKIN_DARK, 0)   # right shadow
    rect(hx,   hy,   16,  1, SKIN_HI,   0)   # top highlight

    # Ear bumps
    px(hx-1,   hy+5,       SKIN)
    px(hx-1,   hy+6,       SKIN)
    px(hx+16,  hy+5,       SKIN)
    px(hx+16,  hy+6,       SKIN)

    # Eyes
    if blink:
        rect(hx+2,  hy+5, 3, 1, PUPIL, 0)
        rect(hx+11, hy+5, 3, 1, PUPIL, 0)
    else:
        # whites
        rect(hx+2,  hy+4, 4, 4, EYE,   0)
        rect(hx+10, hy+4, 4, 4, EYE,   0)
        # pupils
        rect(hx+3,  hy+5, 2, 2, PUPIL, 0)
        rect(hx+11, hy+5, 2, 2, PUPIL, 0)

    # Nose
    px(hx+7,  hy+8, SKIN_DARK)
    px(hx+8,  hy+8, SKIN_DARK)

    # Mouth / teeth
    rect(hx+3,  hy+10, 10, 2, MOUTH, 0)
    px(hx+5,  hy+10, TOOTH)   # left tusk
    px(hx+10, hy+10, TOOTH)   # right tusk

    # ── Ears (pointy) ─────────────────────────────────────────────────────────
    px(hx-2, hy+3, SKIN)
    px(hx-2, hy+4, SKIN)
    px(hx-3, hy+3, SKIN_DARK)
    px(hx+18, hy+3, SKIN)
    px(hx+18, hy+4, SKIN)
    px(hx+19, hy+3, SKIN_DARK)

    return img


# ── Frame definitions ──────────────────────────────────────────────────────────

# Idle: gentle breathing bob (body rises/falls 1 px) + occasional blink
idle_frames = [
    make_frame(body_dy=0,  blink=False),
    make_frame(body_dy=-1, blink=False),
    make_frame(body_dy=-1, blink=False),
    make_frame(body_dy=0,  blink=True),
]

# Walk: 4-frame stride cycle
#   Frame 0: left foot forward,  right foot back  — arms opposite
#   Frame 1: feet level (contact), slight body dip
#   Frame 2: right foot forward, left foot back   — arms opposite
#   Frame 3: feet level (contact), slight body dip
walk_frames = [
    make_frame(body_dy=-1, l_foot_dy=-3, r_foot_dy=2, l_arm_dy=3,  r_arm_dy=-3),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0, l_arm_dy=0,  r_arm_dy=0),
    make_frame(body_dy=-1, l_foot_dy=2,  r_foot_dy=-3,l_arm_dy=-3, r_arm_dy=3),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0, l_arm_dy=0,  r_arm_dy=0),
]

# ── Assemble sheet ─────────────────────────────────────────────────────────────
sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)

for col, frame in enumerate(idle_frames):
    sheet.paste(frame, (col * FRAME_W, 0))

for col, frame in enumerate(walk_frames):
    sheet.paste(frame, (col * FRAME_W, FRAME_H))

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
sheet.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({sheet.width}×{sheet.height})")
