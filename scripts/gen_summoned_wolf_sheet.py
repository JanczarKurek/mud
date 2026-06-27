"""
Generates assets/overworld_objects/summoned_wolf/sheet.png (+ sprite.png still).
Sheet layout: 4 columns x 2 rows, each frame 32x48 px
  Row 0: idle (4 frames, breathing bob + tail flick + blink)
  Row 1: walk (4 frames, quadruped stride cycle)

A spectral side-profile wolf (facing right) in translucent blue-grey with a
faint ghostly glow halo, summoned as a player's companion.
"""

from PIL import Image, ImageDraw
import os

FRAME_W = 32
FRAME_H = 48
COLS = 4
ROWS = 2
OUT_DIR = "assets/overworld_objects/summoned_wolf"
OUT_PATH = f"{OUT_DIR}/sheet.png"
STILL_PATH = f"{OUT_DIR}/sprite.png"

# -- Palette (RGBA; slight translucency sells the "spectral" look) ------------
BG       = (0, 0, 0, 0)
FUR      = (122, 130, 150, 235)   # base blue-grey
FUR_DARK = (78,  86,  112, 235)   # underside / shadow
FUR_HI   = (176, 186, 210, 235)   # back highlight
PAW      = (60,  66,  92,  235)
EYE      = (190, 235, 255, 255)   # pale glowing eye
EYE_CORE = (245, 255, 255, 255)
NOSE     = (52,  58,  82,  255)
MOUTH    = (44,  48,  66,  230)
GLOW     = (150, 175, 225, 70)    # faint outer halo


def make_frame(body_dy=0, fl_dx=0, bl_dx=0, tail_dy=0, blink=False):
    """One 32x48 spectral-wolf frame, facing right.
    body_dy  - vertical bob for the whole torso/head
    fl_dx    - horizontal stride shift of the front legs
    bl_dx    - horizontal stride shift of the back legs
    tail_dy  - vertical flick of the tail tip
    blink    - draw a closed (slit) eye
    """
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry, c)

    GROUND = 43

    # -- Legs (drawn first, behind the body) ----------------------------------
    # Back pair (hind) near x=8, far x=11; front pair near x=20, far x=23.
    def leg(x, top, color):
        rect(x, top, 2, GROUND - top, color)
        rect(x - 1, GROUND - 1, 3, 2, PAW)   # paw

    leg(11 + bl_dx, 32 + body_dy, FUR_DARK)  # far hind
    leg(22 + fl_dx, 31 + body_dy, FUR_DARK)  # far front
    leg(8 - bl_dx, 32 + body_dy, FUR)        # near hind
    leg(19 - fl_dx, 31 + body_dy, FUR)       # near front

    bd = body_dy

    # -- Tail (back-left, sweeping up) ----------------------------------------
    rect(4, 26 + bd + tail_dy, 4, 3, FUR)
    rect(2, 23 + bd + tail_dy, 3, 4, FUR)
    px(2, 22 + bd + tail_dy, FUR_HI)
    px(3, 22 + bd + tail_dy, FUR_HI)

    # -- Torso ----------------------------------------------------------------
    rect(6, 25 + bd, 20, 9, FUR)            # main barrel
    rect(5, 26 + bd, 7, 8, FUR)             # haunch (rear hip)
    rect(20, 23 + bd, 8, 11, FUR)           # chest/shoulder (front)
    rect(6, 25 + bd, 22, 1, FUR_HI)         # back highlight
    rect(6, 32 + bd, 22, 2, FUR_DARK)       # belly shadow

    # -- Neck + head (front-right) --------------------------------------------
    rect(24, 19 + bd, 6, 8, FUR)            # neck
    rect(24, 15 + bd, 8, 8, FUR)            # head
    rect(24, 15 + bd, 8, 1, FUR_HI)         # crown highlight
    rect(29, 20 + bd, 3, 4, FUR)            # snout
    px(31, 21 + bd, NOSE)                   # nose tip
    px(31, 22 + bd, NOSE)
    rect(27, 23 + bd, 4, 1, MOUTH)          # mouth line

    # Ears (two pointed triangles)
    px(24, 13 + bd, FUR); px(25, 13 + bd, FUR); px(25, 14 + bd, FUR)
    px(23, 14 + bd, FUR_DARK)
    px(28, 13 + bd, FUR); px(29, 13 + bd, FUR); px(28, 14 + bd, FUR)
    px(30, 14 + bd, FUR_DARK)

    # Eye
    if blink:
        rect(27, 18 + bd, 2, 1, NOSE)
    else:
        rect(27, 17 + bd, 2, 2, EYE)
        px(27, 17 + bd, EYE_CORE)

    # -- Ghostly glow halo: faint pixels hugging the silhouette ---------------
    occupied = [[img.getpixel((x, y))[3] > 0 for y in range(FRAME_H)]
                for x in range(FRAME_W)]
    for x in range(FRAME_W):
        for y in range(FRAME_H):
            if occupied[x][y]:
                continue
            near = False
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < FRAME_W and 0 <= ny < FRAME_H and occupied[nx][ny]:
                        near = True
            if near:
                img.putpixel((x, y), GLOW)

    return img


# -- Frame definitions --------------------------------------------------------
idle_frames = [
    make_frame(body_dy=0,  tail_dy=0,  blink=False),
    make_frame(body_dy=-1, tail_dy=-1, blink=False),
    make_frame(body_dy=-1, tail_dy=-1, blink=False),
    make_frame(body_dy=0,  tail_dy=0,  blink=True),
]

walk_frames = [
    make_frame(body_dy=-1, fl_dx=2,  bl_dx=-2, tail_dy=-1),
    make_frame(body_dy=0,  fl_dx=0,  bl_dx=0,  tail_dy=0),
    make_frame(body_dy=-1, fl_dx=-2, bl_dx=2,  tail_dy=-1),
    make_frame(body_dy=0,  fl_dx=0,  bl_dx=0,  tail_dy=0),
]

# -- Assemble sheet -----------------------------------------------------------
sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
for col, frame in enumerate(idle_frames):
    sheet.paste(frame, (col * FRAME_W, 0))
for col, frame in enumerate(walk_frames):
    sheet.paste(frame, (col * FRAME_W, FRAME_H))

os.makedirs(OUT_DIR, exist_ok=True)
sheet.save(OUT_PATH)
idle_frames[0].save(STILL_PATH)
print(f"Saved {OUT_PATH}  ({sheet.width}x{sheet.height})")
print(f"Saved {STILL_PATH} ({idle_frames[0].width}x{idle_frames[0].height})")
