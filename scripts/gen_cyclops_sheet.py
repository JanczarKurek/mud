"""
Generates assets/overworld_objects/cyclops/sheet.png  and  sprite.png
Sheet layout: 4 columns × 2 rows, each frame 64×80 px  (LARGER than other mobs)
Sheet size: 256×160 px
  Row 0: idle  (4 frames – slow heave, single eye blink, club thump)
  Row 1: walk  (4 frames – thunderous stomp)
"""

from PIL import Image
import os

FRAME_W = 64
FRAME_H = 80
COLS    = 4
ROWS    = 2
OUT_SHEET  = "assets/overworld_objects/cyclops/sheet.png"
OUT_SPRITE = "assets/overworld_objects/cyclops/sprite.png"

# ── Palette ────────────────────────────────────────────────────────────────────
BG         = (  0,   0,   0,   0)
SKIN       = (112, 132,  84, 255)   # rocky grey-green skin
SKIN_DARK  = ( 68,  80,  48, 255)   # deep shadow / outline
SKIN_HI    = (148, 172, 110, 255)   # highlight
SKIN_MID   = ( 95, 112,  68, 255)   # mid-tone muscle shadow

EYE_WHITE  = (240, 230, 180, 255)   # single large eye – yellow sclera
EYE_IRIS   = (210,  80,  20, 255)   # fiery orange iris
EYE_PUPIL  = ( 25,  10,   5, 255)   # dark slit pupil

BROW       = ( 48,  56,  30, 255)   # heavy mono-brow

TEETH      = (232, 220, 175, 255)   # off-white tusks / teeth
MOUTH_DARK = ( 35,  20,   8, 255)   # inside mouth

LOIN       = ( 80,  55,  20, 255)   # rough loincloth / hide
LOIN_DARK  = ( 52,  34,  10, 255)

CLUB       = ( 95,  65,  25, 255)   # wooden club
CLUB_DARK  = ( 58,  38,  10, 255)
CLUB_HI    = (130,  95,  42, 255)
KNOT       = ( 68,  44,  14, 255)   # club knot / nail

BELT       = ( 60,  40,  14, 255)


def make_frame(body_dy=0, l_foot_dy=0, r_foot_dy=0,
               l_arm_dy=0, r_arm_dy=0, club_dy=0,
               blink=False, jaw_open=False):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry, c)

    bd = body_dy

    # ── Club (drawn behind right arm so it looks held) ────────────────────────
    # Club shaft
    cd = club_dy
    rect(44, 20+bd+cd, 5, 30, CLUB_DARK)
    rect(44, 20+bd+cd, 4, 29, CLUB)
    rect(45, 20+bd+cd, 2, 29, CLUB_HI)
    # Club head (knotted end, at top)
    rect(41, 12+bd+cd, 11, 10, CLUB_DARK)
    rect(42, 11+bd+cd,  9,  9, CLUB)
    rect(43, 11+bd+cd,  7,  2, CLUB_HI)
    # Nails / knots on club head
    for kx in (43, 46, 49):
        rect(kx, 14+bd+cd, 2, 2, KNOT)

    # ── Feet ─────────────────────────────────────────────────────────────────
    lby = 66 + l_foot_dy
    # Left foot (wide, flat, toe bumps)
    rect(12, lby,     10, 7, SKIN_DARK)
    rect(12, lby,     10, 6, SKIN)
    rect(12, lby,     10, 2, SKIN_HI)
    for tx in (12, 15, 18, 21):
        px(tx, lby+6, SKIN_HI)   # toe knuckles

    rby = 66 + r_foot_dy
    rect(38, rby,     10, 7, SKIN_DARK)
    rect(38, rby,     10, 6, SKIN)
    rect(38, rby,     10, 2, SKIN_HI)
    for tx in (38, 41, 44, 47):
        px(tx, rby+6, SKIN_HI)

    # ── Lower legs (thick pillars) ────────────────────────────────────────────
    # Left leg
    rect(13, 50+bd, 9, 17, SKIN_DARK)
    rect(13, 49+bd, 8, 17, SKIN)
    rect(14, 49+bd, 4, 17, SKIN_HI)
    rect(13, 62+bd, 9,  2, SKIN_MID)   # ankle crease

    # Right leg
    rect(38, 50+bd, 9, 17, SKIN_DARK)
    rect(38, 49+bd, 8, 17, SKIN)
    rect(38, 49+bd, 4, 17, SKIN_HI)
    rect(38, 62+bd, 9,  2, SKIN_MID)

    # ── Loincloth ─────────────────────────────────────────────────────────────
    rect( 8, 44+bd, 48, 10, LOIN_DARK)
    rect( 9, 43+bd, 46,  9, LOIN)
    rect(10, 43+bd, 44,  1, BELT)      # belt line
    # Loin flap triangles
    rect(12, 51+bd,  8,  6, LOIN)
    rect(40, 51+bd,  8,  6, LOIN)
    rect(26, 51+bd, 12,  8, LOIN)

    # ── Hips (wide pelvis) ────────────────────────────────────────────────────
    rect( 9, 41+bd, 46, 5, SKIN_DARK)
    rect(10, 40+bd, 44, 5, SKIN)
    rect(11, 40+bd, 42, 1, SKIN_HI)

    # ── Torso (massive barrel chest) ──────────────────────────────────────────
    rect( 7, 20+bd, 50, 22, SKIN_DARK)  # shadow outline
    rect( 8, 19+bd, 48, 22, SKIN)       # main torso
    rect(10, 19+bd, 44,  3, SKIN_HI)   # shoulder highlight
    # Pectoral muscle lines
    rect( 8, 30+bd, 48,  1, SKIN_MID)
    rect(30, 20+bd,  4, 22, SKIN_MID)  # sternum
    # Belly crease
    rect(14, 37+bd, 36,  2, SKIN_DARK)

    # ── Left arm (huge, hangs at side) ────────────────────────────────────────
    lad = l_arm_dy
    # upper arm
    rect(2,  22+bd+lad, 8, 14, SKIN_DARK)
    rect(2,  21+bd+lad, 7, 14, SKIN)
    rect(3,  21+bd+lad, 4, 14, SKIN_HI)
    # elbow
    rect(1,  34+bd+lad, 9,  3, SKIN_HI)
    # forearm
    rect(2,  37+bd+lad, 8, 13, SKIN_DARK)
    rect(2,  36+bd+lad, 7, 13, SKIN)
    # fist / knuckles
    rect(1,  49+bd+lad, 9,  6, SKIN_DARK)
    rect(1,  48+bd+lad, 8,  6, SKIN)
    for kx in (2, 4, 6, 8):
        px(kx, 53+bd+lad, SKIN_HI)

    # ── Right arm (raised, holding club) ──────────────────────────────────────
    rad = r_arm_dy
    rect(54, 18+bd+rad, 8, 14, SKIN_DARK)
    rect(54, 17+bd+rad, 7, 14, SKIN)
    rect(55, 17+bd+rad, 4, 14, SKIN_HI)
    rect(53, 30+bd+rad, 9,  3, SKIN_HI)  # elbow
    rect(54, 33+bd+rad, 8, 13, SKIN_DARK)
    rect(54, 32+bd+rad, 7, 13, SKIN)
    # fist gripping club
    rect(53, 45+bd+rad, 9,  6, SKIN_DARK)
    rect(53, 44+bd+rad, 8,  6, SKIN)
    for kx in (54, 56, 58, 60):
        px(kx, 49+bd+rad, SKIN_HI)

    # ── Neck (short stubby) ───────────────────────────────────────────────────
    rect(25, 14+bd, 14, 7, SKIN_DARK)
    rect(25, 13+bd, 13, 7, SKIN)
    rect(26, 13+bd,  8, 2, SKIN_HI)

    # ── Head (massive, roughly square) ───────────────────────────────────────
    hx = 12
    hy = 1 + bd
    rect(hx,   hy,   40, 14, SKIN_DARK)   # shadow outline
    rect(hx+1, hy,   38, 13, SKIN)        # head
    rect(hx+2, hy,   36,  3, SKIN_HI)    # brow highlight
    # Skull ridge
    rect(hx+2, hy,   36,  1, SKIN_HI)

    # ── Mono-brow ─────────────────────────────────────────────────────────────
    rect(hx+2, hy+5,  36, 3, BROW)
    # Brow ridge bump in centre
    rect(hx+16,hy+3,  10, 2, BROW)

    # ── Single large eye ──────────────────────────────────────────────────────
    ex = hx + 11
    ey = hy + 6
    if blink:
        rect(ex, ey+3, 20, 2, BROW)   # closed lid = heavy brow squint
    else:
        # eye white
        rect(ex,   ey,   20, 7, SKIN_DARK)   # outline
        rect(ex+1, ey,   18, 6, EYE_WHITE)
        # iris (large)
        rect(ex+5, ey+1, 10, 4, EYE_IRIS)
        # slit pupil
        rect(ex+9, ey+1,  2, 4, EYE_PUPIL)
        # corner highlights
        px(ex+2,  ey+1, EYE_WHITE)
        px(ex+15, ey+4, EYE_WHITE)

    # ── Nose (wide, flattened) ────────────────────────────────────────────────
    rect(hx+14,hy+10, 12, 3, SKIN_MID)
    px(hx+15, hy+11, SKIN_DARK)
    px(hx+22, hy+11, SKIN_DARK)

    # ── Mouth / jaw ──────────────────────────────────────────────────────────
    if jaw_open:
        rect(hx+8, hy+13, 26, 4, MOUTH_DARK)
        # upper teeth
        for tx in range(hx+9, hx+32, 4):
            rect(tx, hy+13, 3, 2, TEETH)
        # lower teeth
        for tx in range(hx+11, hx+32, 4):
            rect(tx, hy+15, 3, 2, TEETH)
    else:
        rect(hx+8, hy+13, 26, 2, MOUTH_DARK)
        # bottom jaw closed — just a line with tusk tips
        for tx in range(hx+9, hx+32, 4):
            px(tx, hy+13, TEETH)

    return img


# ── Frame definitions ──────────────────────────────────────────────────────────

idle_frames = [
    make_frame(body_dy=0,  club_dy=0,  blink=False, jaw_open=False),
    make_frame(body_dy=-1, club_dy=1,  blink=False, jaw_open=False),
    make_frame(body_dy=-1, club_dy=1,  blink=False, jaw_open=True),
    make_frame(body_dy=0,  club_dy=0,  blink=True,  jaw_open=False),
]

walk_frames = [
    make_frame(body_dy=-1, l_foot_dy=-4, r_foot_dy=3, l_arm_dy=4,  r_arm_dy=-4),
    make_frame(body_dy=2,  l_foot_dy=0,  r_foot_dy=0, l_arm_dy=0,  r_arm_dy=0),
    make_frame(body_dy=-1, l_foot_dy=3,  r_foot_dy=-4,l_arm_dy=-4, r_arm_dy=4),
    make_frame(body_dy=2,  l_foot_dy=0,  r_foot_dy=0, l_arm_dy=0,  r_arm_dy=0),
]

# ── Assemble sheet ─────────────────────────────────────────────────────────────
sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
for col, frame in enumerate(idle_frames):
    sheet.paste(frame, (col * FRAME_W, 0))
for col, frame in enumerate(walk_frames):
    sheet.paste(frame, (col * FRAME_W, FRAME_H))

os.makedirs(os.path.dirname(OUT_SHEET), exist_ok=True)
sheet.save(OUT_SHEET)
print(f"Saved {OUT_SHEET}  ({sheet.width}×{sheet.height})")

# ── Sprite (first idle frame, scaled 1.5×, crop to 96×96 → save as is) ────────
# For a "large" sprite preview we do a 1:1 save + a 2× thumbnail
sprite_full = idle_frames[0].copy()
sprite_full.save(OUT_SPRITE)
print(f"Saved {OUT_SPRITE}  ({sprite_full.width}×{sprite_full.height})")
