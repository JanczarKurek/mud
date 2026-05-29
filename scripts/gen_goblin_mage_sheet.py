"""
Generates assets/overworld_objects/goblin_mage/sheet.png and sprite.png.
Sheet layout: 4 columns x 2 rows, each frame 32x48 px
  Row 0: idle (4 frames, robe sway + staff crystal pulse + blink)
  Row 1: walk (4 frames, stride with staff thump)
The mage is a goblin variant: same green skin and pointy ears as the
classic goblin, but draped in a dark-purple hooded robe and gripping a
bone-tipped wooden staff with a glowing crystal on top.
"""

from PIL import Image, ImageDraw
import os

FRAME_W = 32
FRAME_H = 48
COLS = 4
ROWS = 2
OUT_DIR = "assets/overworld_objects/goblin_mage"
OUT_SHEET = os.path.join(OUT_DIR, "sheet.png")
OUT_SPRITE = os.path.join(OUT_DIR, "sprite.png")

# ----- Palette ----------------------------------------------------------------
BG          = (0,   0,   0,   0)
SKIN        = (92,  140,  52, 255)
SKIN_DARK   = (56,   96,  28, 255)
SKIN_HI     = (124, 180,  72, 255)
EYE         = (255, 220,  30, 255)
PUPIL       = (20,   20,  20, 255)
MOUTH       = (40,   20,  10, 255)
TOOTH       = (240, 230, 180, 255)

ROBE        = (62,   30,  82, 255)   # dark purple
ROBE_DARK   = (38,   16,  58, 255)
ROBE_HI     = (92,   52, 116, 255)
HOOD        = (44,   22,  70, 255)   # slightly darker than robe
HOOD_HI     = (78,   46,  98, 255)
TRIM        = (200, 170,  60, 255)   # golden trim
BELT_ROPE   = (140, 110,  50, 255)

WOOD        = (96,   58,  28, 255)   # staff shaft
WOOD_DARK   = (62,   36,  16, 255)
BONE        = (236, 224, 188, 255)   # staff tip
BONE_DARK   = (180, 168, 130, 255)
CRYSTAL     = (140, 220, 255, 255)   # glowing crystal
CRYSTAL_HI  = (220, 240, 255, 255)
CRYSTAL_GLO = (180, 230, 255, 110)   # translucent glow halo


def make_frame(
    body_dy=0,
    l_foot_dy=0,
    r_foot_dy=0,
    staff_dy=0,
    crystal_bright=0,  # 0 = base, 1 = brighter halo
    blink=False,
    show_left_sleeve=True,
):
    """One 32x48 goblin-mage frame."""
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c, dy=0):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry + dy, c)

    # ----- Feet / boots (just dark robe edge peeking) ------------------------
    lby = 42 + l_foot_dy
    rect(11, lby, 4, 2, ROBE_DARK)
    rby = 42 + r_foot_dy
    rect(17, rby, 4, 2, ROBE_DARK)

    # ----- Robe skirt (flares at the bottom) ---------------------------------
    bd = body_dy
    # Bottom hem: widest
    rect(7,  39+bd, 18, 4, ROBE)
    rect(7,  39+bd,  1, 4, ROBE_DARK)
    rect(24, 39+bd,  1, 4, ROBE_DARK)
    rect(7,  42+bd, 18, 1, ROBE_DARK)  # hem shadow
    # Mid-skirt
    rect(8,  34+bd, 16, 5, ROBE)
    rect(8,  34+bd,  1, 5, ROBE_DARK)
    rect(23, 34+bd,  1, 5, ROBE_DARK)
    # Golden trim along the bottom
    for i in range(0, 18, 3):
        px(7+i, 42+bd, TRIM)
        px(8+i, 42+bd, TRIM)

    # ----- Belt / rope tie ---------------------------------------------------
    rect(9, 32+bd, 14, 2, BELT_ROPE)
    px(15, 33+bd, TRIM)
    px(16, 33+bd, TRIM)

    # ----- Torso robe --------------------------------------------------------
    rect(9, 19+bd, 14, 13, ROBE)
    rect(9, 19+bd,  1, 13, ROBE_DARK)
    rect(22, 19+bd, 1, 13, ROBE_DARK)
    # Center seam / shading
    rect(15, 19+bd, 2, 13, ROBE_HI)
    # Front panel divider
    rect(13, 23+bd, 1, 9, ROBE_DARK)
    rect(18, 23+bd, 1, 9, ROBE_DARK)

    # ----- Left sleeve (hidden hand) -----------------------------------------
    if show_left_sleeve:
        rect(6, 22+bd, 4, 9, ROBE)
        rect(6, 22+bd, 1, 9, ROBE_DARK)
        rect(6, 30+bd, 4, 1, ROBE_DARK)  # cuff
        # tip of skin barely peeks
        px(8, 31+bd, SKIN)
        px(7, 31+bd, SKIN_DARK)

    # ----- Right sleeve + arm gripping staff ---------------------------------
    # Sleeve drops from the shoulder, then a green hand pokes out clutching the
    # staff shaft. Arm position is fixed (mage holds staff steady regardless of
    # walk cycle) so the staff bob comes from `staff_dy`.
    rect(22, 22+bd, 4, 8, ROBE)
    rect(25, 22+bd, 1, 8, ROBE_DARK)
    rect(22, 29+bd, 4, 1, ROBE_DARK)  # cuff
    # Hand (green skin)
    rect(23, 30+bd, 3, 3, SKIN)
    px(23, 32+bd, SKIN_DARK)

    # ----- Staff (held in right hand, vertical) ------------------------------
    sx = 26  # staff x column
    sdy = staff_dy
    # Shaft from the hand up past the head
    for y in range(2+sdy, 36+sdy):
        if 0 <= y < FRAME_H:
            px(sx,   y, WOOD)
            px(sx+1, y, WOOD_DARK)
    # Bone wrap below crystal
    rect(sx-1, 4+sdy, 4, 2, BONE)
    px(sx-1, 5+sdy, BONE_DARK)
    px(sx+2, 5+sdy, BONE_DARK)
    # Crystal mount (small bone claw cradling the gem)
    px(sx-1, 3+sdy, BONE_DARK)
    px(sx+2, 3+sdy, BONE_DARK)

    # Crystal (diamond shape)
    cx, cy = sx, 0+sdy
    # Halo glow first so it sits under the crystal
    if crystal_bright >= 1:
        for dy in range(-1, 4):
            for dx in range(-2, 4):
                tx, ty = cx + dx, cy + dy
                if 0 <= tx < FRAME_W and 0 <= ty < FRAME_H:
                    if img.getpixel((tx, ty)) == BG:
                        img.putpixel((tx, ty), CRYSTAL_GLO)
    # Crystal body
    px(cx, cy,     CRYSTAL_HI)
    px(cx+1, cy,   CRYSTAL_HI)
    rect(cx-1, cy+1, 4, 2, CRYSTAL)
    px(cx, cy+1,   CRYSTAL_HI)
    px(cx, cy+3,   CRYSTAL)
    px(cx+1, cy+3, CRYSTAL)

    # ----- Neck -------------------------------------------------------------
    rect(14, 16+bd, 4, 3, SKIN)
    px(14, 18+bd, SKIN_DARK)

    # ----- Head -------------------------------------------------------------
    hx, hy = 8, 4+bd
    rect(hx,   hy,   16, 14, SKIN)
    rect(hx,   hy,    1, 14, SKIN_DARK)   # left shadow
    rect(hx+15,hy,    1, 14, SKIN_DARK)   # right shadow
    rect(hx,   hy,   16,  1, SKIN_HI)     # top highlight

    # Ear bumps
    px(hx-1,   hy+5,  SKIN)
    px(hx-1,   hy+6,  SKIN)
    px(hx+16,  hy+5,  SKIN)
    px(hx+16,  hy+6,  SKIN)
    # Pointy ears
    px(hx-2,   hy+3,  SKIN)
    px(hx-2,   hy+4,  SKIN)
    px(hx-3,   hy+3,  SKIN_DARK)
    px(hx+18,  hy+3,  SKIN)
    px(hx+18,  hy+4,  SKIN)
    px(hx+19,  hy+3,  SKIN_DARK)

    # ----- Hood (drapes over head, leaving face exposed) --------------------
    # Hood sits across the top half of the head and on the shoulders.
    rect(hx-1, hy-1, 18, 4, HOOD)
    rect(hx-1, hy-1, 18, 1, HOOD_HI)
    # Drape down the sides of the head
    rect(hx-1, hy+2, 2, 9, HOOD)
    rect(hx+16, hy+2, 2, 9, HOOD)
    # Cowl on shoulders, falling onto torso
    rect(hx-2, hy+11, 20, 3, HOOD)
    rect(hx-2, hy+11, 1, 3, HOOD_HI)
    rect(hx+17, hy+11, 1, 3, HOOD_HI)
    # Golden trim along the hood's edge
    for i in range(-2, 18, 4):
        px(hx + i, hy+13, TRIM)
        px(hx + i + 1, hy+13, TRIM)

    # Eyes (peeking out from under the hood)
    if blink:
        rect(hx+2,  hy+7, 3, 1, PUPIL)
        rect(hx+11, hy+7, 3, 1, PUPIL)
    else:
        rect(hx+2,  hy+6, 4, 3, EYE)
        rect(hx+10, hy+6, 4, 3, EYE)
        rect(hx+3,  hy+7, 2, 2, PUPIL)
        rect(hx+11, hy+7, 2, 2, PUPIL)

    # Nose
    px(hx+7,  hy+10, SKIN_DARK)
    px(hx+8,  hy+10, SKIN_DARK)

    # Mouth / single tusk
    rect(hx+5, hy+12, 6, 2, MOUTH)
    px(hx+6,  hy+12, TOOTH)
    px(hx+9,  hy+12, TOOTH)

    return img


# ----- Frame definitions ------------------------------------------------------

# Idle: subtle bob + crystal pulse on frame 2 + blink on frame 3.
idle_frames = [
    make_frame(body_dy=0,  staff_dy=0,  crystal_bright=0, blink=False),
    make_frame(body_dy=-1, staff_dy=-1, crystal_bright=1, blink=False),
    make_frame(body_dy=-1, staff_dy=-1, crystal_bright=1, blink=False),
    make_frame(body_dy=0,  staff_dy=0,  crystal_bright=0, blink=True),
]

# Walk: 4-frame stride. Staff thumps in time with the trailing foot. Robe
# sways subtly so the silhouette shifts even though most of the body is
# covered. The mage keeps his left hand tucked into the sleeve.
walk_frames = [
    make_frame(body_dy=-1, l_foot_dy=-2, r_foot_dy=2,  staff_dy=0, crystal_bright=0),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  staff_dy=1, crystal_bright=1),
    make_frame(body_dy=-1, l_foot_dy=2,  r_foot_dy=-2, staff_dy=0, crystal_bright=0),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  staff_dy=1, crystal_bright=1),
]

# ----- Assemble sheet --------------------------------------------------------
sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
for col, frame in enumerate(idle_frames):
    sheet.paste(frame, (col * FRAME_W, 0))
for col, frame in enumerate(walk_frames):
    sheet.paste(frame, (col * FRAME_W, FRAME_H))

os.makedirs(OUT_DIR, exist_ok=True)
sheet.save(OUT_SHEET)
print(f"Saved {OUT_SHEET}  ({sheet.width}x{sheet.height})")

# Single-frame fallback sprite (first idle frame).
idle_frames[0].save(OUT_SPRITE)
print(f"Saved {OUT_SPRITE}  ({FRAME_W}x{FRAME_H})")
