"""
Generates assets/overworld_objects/player/sheet.png
Sheet layout: 4 columns × 2 rows, each frame 32×48 px
  Row 0: idle (4 frames, breathing + hair sway + blink)
  Row 1: walk (4 frames, stride cycle)
Inspired by the existing sprite_large.png: purple tunic, yellow hair, light skin.
"""

from PIL import Image, ImageDraw
import os

FRAME_W = 32
FRAME_H = 48
COLS    = 4
ROWS    = 2
OUT     = "assets/overworld_objects/player/sheet.png"

# ── Palette ────────────────────────────────────────────────────────────────────
BG          = (0,   0,   0,   0)    # transparent

SKIN        = (220, 170, 120, 255)
SKIN_DARK   = (180, 130,  85, 255)
SKIN_HI     = (240, 195, 150, 255)

HAIR        = (220, 180,  35, 255)
HAIR_DARK   = (170, 130,  20, 255)
HAIR_HI     = (250, 215,  80, 255)

EYE_WHITE   = (240, 240, 240, 255)
EYE_IRIS    = ( 60, 110, 200, 255)
EYE_PUPIL   = ( 20,  20,  30, 255)

TUNIC       = (145,  55, 165, 255)  # purple
TUNIC_HI    = (175,  90, 200, 255)
TUNIC_DARK  = (100,  30, 120, 255)

BELT        = (130,  85,  25, 255)
BELT_BUCKLE = (210, 175,  50, 255)

PANTS       = ( 55,  70, 105, 255)  # dark blue-grey
PANTS_DARK  = ( 35,  48,  75, 255)

BOOT        = ( 72,  44,  18, 255)
BOOT_HI     = ( 95,  62,  28, 255)

CAPE        = (110,  30,  30, 255)  # dark red cape
CAPE_HI     = (145,  50,  50, 255)

SATCHEL     = (140,  95,  40, 255)  # small bag on hip


def make_frame(body_dy=0, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0,
               hair_dx=0, blink=False):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c, dy=0, dx=0):
        for ry in range(h):
            for rx in range(w):
                px(x + rx + dx, y + ry + dy, c)

    bd = body_dy   # body vertical shift

    # ── Boots ─────────────────────────────────────────────────────────────────
    # Left boot
    lby = 38 + l_foot_dy
    rect(10, lby,     5, 7, BOOT,    0)
    rect(10, lby,     5, 1, BOOT_HI, 0)
    rect( 9, lby+1,   1, 5, BOOT,    0)

    # Right boot
    rby = 38 + r_foot_dy
    rect(17, rby,     5, 7, BOOT,    0)
    rect(17, rby,     5, 1, BOOT_HI, 0)
    rect(22, rby+1,   1, 5, BOOT,    0)

    # ── Pants ─────────────────────────────────────────────────────────────────
    rect(10, 29+bd, 5, 10, PANTS,      0)   # left leg
    rect(17, 29+bd, 5, 10, PANTS,      0)   # right leg
    rect(13, 29+bd, 6,  5, PANTS,      0)   # crotch
    rect(10, 29+bd, 1, 10, PANTS_DARK, 0)   # left seam
    rect(21, 29+bd, 1, 10, PANTS_DARK, 0)   # right seam

    # ── Belt ──────────────────────────────────────────────────────────────────
    rect( 9, 27+bd, 14, 3, BELT,        0)
    rect(14, 27+bd,  3, 3, BELT_BUCKLE, 0)  # buckle

    # ── Cape (behind body, left side peek) ────────────────────────────────────
    rect(7, 16+bd, 3, 13, CAPE,    0)
    rect(7, 16+bd, 1, 13, CAPE_HI, 0)

    # ── Tunic ─────────────────────────────────────────────────────────────────
    rect( 9, 15+bd, 14, 13, TUNIC,      0)
    rect( 9, 15+bd,  1, 13, TUNIC_DARK, 0)   # left edge
    rect(22, 15+bd,  1, 13, TUNIC_DARK, 0)   # right edge
    rect( 9, 15+bd, 14,  1, TUNIC_HI,   0)   # collar highlight
    # Chest detail line
    for y in range(17+bd, 26+bd):
        px(15, y, TUNIC_DARK)

    # ── Satchel (right hip) ───────────────────────────────────────────────────
    rect(22, 24+bd, 4, 5, SATCHEL, 0)
    rect(22, 24+bd, 4, 1, BELT,    0)   # strap top

    # ── Left arm ──────────────────────────────────────────────────────────────
    lad = l_arm_dy
    rect( 6, 17+bd+lad, 4, 8, TUNIC,     0)
    rect( 6, 25+bd+lad, 4, 4, SKIN,      0)   # forearm
    rect( 6, 29+bd+lad, 4, 2, SKIN_DARK, 0)   # wrist
    rect( 6, 17+bd+lad, 1, 8, TUNIC_DARK,0)   # sleeve edge

    # ── Right arm ─────────────────────────────────────────────────────────────
    rad = r_arm_dy
    rect(22, 17+bd+rad, 4, 8, TUNIC,     0)
    rect(22, 25+bd+rad, 4, 4, SKIN,      0)
    rect(22, 29+bd+rad, 4, 2, SKIN_DARK, 0)
    rect(25, 17+bd+rad, 1, 8, TUNIC_DARK,0)

    # ── Neck ──────────────────────────────────────────────────────────────────
    rect(14, 12+bd, 4, 4, SKIN, 0)

    # ── Head ──────────────────────────────────────────────────────────────────
    hx, hy = 9, 2+bd
    rect(hx,   hy,   14, 12, SKIN,      0)
    rect(hx,   hy,    1, 12, SKIN_DARK, 0)
    rect(hx+13,hy,    1, 12, SKIN_DARK, 0)
    rect(hx,   hy,   14,  1, SKIN_HI,   0)

    # ── Hair (covers top + sides of head) ─────────────────────────────────────
    hdx = hair_dx
    rect(hx-1, hy-2,  16,  4, HAIR,      hdx)  # top hair
    rect(hx-1, hy-2,  16,  1, HAIR_HI,   hdx)  # highlight
    rect(hx-1, hy+2,   2,  6, HAIR,      hdx)  # left sideburn
    rect(hx+13,hy+2,   2,  6, HAIR,      hdx)  # right sideburn
    # Hair tuft at top
    px(hx+6+hdx, hy-3, HAIR)
    px(hx+7+hdx, hy-3, HAIR_HI)
    px(hx+8+hdx, hy-3, HAIR)

    # ── Ears ──────────────────────────────────────────────────────────────────
    px(hx-1, hy+4, SKIN)
    px(hx-1, hy+5, SKIN)
    px(hx+14,hy+4, SKIN)
    px(hx+14,hy+5, SKIN)

    # ── Eyes ──────────────────────────────────────────────────────────────────
    if blink:
        rect(hx+2, hy+5, 3, 1, SKIN_DARK, 0)
        rect(hx+9, hy+5, 3, 1, SKIN_DARK, 0)
    else:
        rect(hx+2, hy+4, 4, 4, EYE_WHITE, 0)
        rect(hx+9, hy+4, 4, 4, EYE_WHITE, 0)
        rect(hx+3, hy+5, 2, 2, EYE_IRIS,  0)
        rect(hx+10,hy+5, 2, 2, EYE_IRIS,  0)
        px(hx+4,  hy+6, EYE_PUPIL)
        px(hx+11, hy+6, EYE_PUPIL)

    # ── Eyebrows ──────────────────────────────────────────────────────────────
    rect(hx+2, hy+3, 4, 1, HAIR_DARK, 0)
    rect(hx+9, hy+3, 4, 1, HAIR_DARK, 0)

    # ── Nose & mouth ──────────────────────────────────────────────────────────
    px(hx+6, hy+8, SKIN_DARK)
    px(hx+7, hy+8, SKIN_DARK)
    rect(hx+4, hy+10, 6, 1, SKIN_DARK, 0)   # mouth line
    px(hx+5,  hy+10, SKIN_HI)               # smile
    px(hx+9,  hy+10, SKIN_HI)

    return img


# ── Frame definitions ──────────────────────────────────────────────────────────

# Idle: gentle breathing, hair sway, blink on frame 3
idle_frames = [
    make_frame(body_dy=0,  hair_dx=0,  blink=False),
    make_frame(body_dy=-1, hair_dx=0,  blink=False),
    make_frame(body_dy=-1, hair_dx=1,  blink=False),
    make_frame(body_dy=0,  hair_dx=0,  blink=True),
]

# Walk: 4-frame stride cycle with arm swing
walk_frames = [
    make_frame(body_dy=-1, l_foot_dy=-3, r_foot_dy=2,  l_arm_dy=3,  r_arm_dy=-3),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  l_arm_dy=0,  r_arm_dy=0),
    make_frame(body_dy=-1, l_foot_dy=2,  r_foot_dy=-3, l_arm_dy=-3, r_arm_dy=3),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  l_arm_dy=0,  r_arm_dy=0),
]

# ── Assemble ───────────────────────────────────────────────────────────────────
sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)

for col, frame in enumerate(idle_frames):
    sheet.paste(frame, (col * FRAME_W, 0))
for col, frame in enumerate(walk_frames):
    sheet.paste(frame, (col * FRAME_W, FRAME_H))

os.makedirs(os.path.dirname(OUT), exist_ok=True)
sheet.save(OUT)
print(f"Saved {OUT}  ({sheet.width}×{sheet.height})")
