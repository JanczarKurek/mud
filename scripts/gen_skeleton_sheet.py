"""
Generates assets/overworld_objects/skeleton/sheet.png  and  sprite.png
Sheet layout: 4 columns × 2 rows, each frame 32×48 px
  Row 0: idle  (4 frames – slow eerie sway, jaw chatter, rib shift)
  Row 1: walk  (4 frames – lurching bony march)
"""

from PIL import Image
import os

FRAME_W = 32
FRAME_H = 48
COLS    = 4
ROWS    = 2
OUT_SHEET  = "assets/overworld_objects/skeleton/sheet.png"
OUT_SPRITE = "assets/overworld_objects/skeleton/sprite.png"

# ── Palette ────────────────────────────────────────────────────────────────────
BG         = (  0,   0,   0,   0)   # transparent
BONE       = (218, 210, 185, 255)   # main bone
BONE_DARK  = (150, 140, 118, 255)   # shadow / outline
BONE_HI    = (240, 235, 215, 255)   # highlight (knuckles, skull top)
GLOW       = (140, 210, 160, 255)   # faint ghostly eye glow
GLOW_DARK  = ( 40,  90,  55, 255)   # pupil glow
CLOTH      = ( 72,  58,  38, 255)   # tattered cloth strip (belt / loincloth)
CLOTH_DARK = ( 48,  36,  20, 255)
RUST       = (110,  60,  30, 255)   # rusty weapon / armour bit (optional accent)


def make_frame(body_dy=0, l_foot_dy=0, r_foot_dy=0,
               l_arm_dy=0, r_arm_dy=0, jaw_open=False):
    """
    Draw one 32×48 skeleton frame.
    body_dy      – vertical shift (breathing sway)
    l/r_foot_dy  – foot raise (walk cycle)
    l/r_arm_dy   – arm swing
    jaw_open     – draw open jaw (idle chatter)
    """
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry, c)

    bd = body_dy

    # ── Feet / ankle bones ────────────────────────────────────────────────────
    # Left foot
    lfy = 40 + l_foot_dy
    rect(10, lfy,     5, 5, BONE_DARK)
    rect(10, lfy,     5, 4, BONE)
    rect(10, lfy,     5, 1, BONE_HI)
    # toes
    px(9,  lfy+4, BONE); px(11, lfy+4, BONE); px(13, lfy+4, BONE)

    # Right foot
    rfy = 40 + r_foot_dy
    rect(17, rfy,     5, 5, BONE_DARK)
    rect(17, rfy,     5, 4, BONE)
    rect(17, rfy,     5, 1, BONE_HI)
    px(16, rfy+4, BONE); px(18, rfy+4, BONE); px(20, rfy+4, BONE)

    # ── Shin / tibia bones ────────────────────────────────────────────────────
    # Left shin (narrow bone column)
    rect(11, 32+bd, 3, 9, BONE_DARK)
    rect(12, 31+bd, 1, 9, BONE_HI)
    rect(11, 31+bd, 3, 9, BONE)
    # knee knob
    rect(10, 30+bd, 4, 2, BONE_HI)

    # Right shin
    rect(18, 32+bd, 3, 9, BONE_DARK)
    rect(19, 31+bd, 1, 9, BONE_HI)
    rect(18, 31+bd, 3, 9, BONE)
    rect(17, 30+bd, 4, 2, BONE_HI)

    # ── Pelvis / hip ─────────────────────────────────────────────────────────
    rect( 9, 27+bd, 14, 5, BONE_DARK)
    rect(10, 26+bd, 12, 5, BONE)
    rect(10, 26+bd, 12, 1, BONE_HI)
    # hip socket indentations
    px(11, 28+bd, BONE_DARK)
    px(20, 28+bd, BONE_DARK)
    # Tattered loin-cloth
    rect(10, 28+bd, 12, 4, CLOTH)
    rect(11, 28+bd, 10, 1, CLOTH_DARK)

    # ── Spine (a column of vertebra dots through torso) ───────────────────────
    for sy in range(18+bd, 27+bd, 2):
        px(15, sy, BONE_HI)
        px(16, sy, BONE_HI)

    # ── Ribcage ───────────────────────────────────────────────────────────────
    # Central sternum column
    rect(14, 16+bd, 4, 12, BONE_DARK)
    rect(14, 16+bd, 4, 11, BONE)
    # Rib pairs (3 pairs)
    for rib_y, indent in [(17, 0), (20, 1), (23, 2)]:
        ry = rib_y + bd
        # left rib
        rect(9+indent, ry, 6-indent, 2, BONE_DARK)
        rect(9+indent, ry, 5-indent, 1, BONE)
        # right rib
        rect(18, ry, 6-indent, 2, BONE_DARK)
        rect(18, ry, 5-indent, 1, BONE)

    # ── Left arm ──────────────────────────────────────────────────────────────
    lad = l_arm_dy
    # upper arm
    rect(7,  17+bd+lad, 3, 8, BONE_DARK)
    rect(7,  17+bd+lad, 2, 7, BONE)
    rect(7,  17+bd+lad, 2, 1, BONE_HI)
    # elbow knob
    rect(6,  24+bd+lad, 4, 2, BONE_HI)
    # forearm
    rect(7,  26+bd+lad, 3, 7, BONE_DARK)
    rect(7,  26+bd+lad, 2, 6, BONE)
    # finger bones
    px(7,  33+bd+lad, BONE); px(8,  33+bd+lad, BONE); px(9,  33+bd+lad, BONE)

    # ── Right arm ─────────────────────────────────────────────────────────────
    rad = r_arm_dy
    rect(22, 17+bd+rad, 3, 8, BONE_DARK)
    rect(22, 17+bd+rad, 2, 7, BONE)
    rect(22, 17+bd+rad, 2, 1, BONE_HI)
    rect(22, 24+bd+rad, 4, 2, BONE_HI)
    rect(22, 26+bd+rad, 3, 7, BONE_DARK)
    rect(22, 26+bd+rad, 2, 6, BONE)
    px(22, 33+bd+rad, BONE); px(23, 33+bd+rad, BONE); px(24, 33+bd+rad, BONE)

    # ── Neck vertebrae ────────────────────────────────────────────────────────
    rect(14, 12+bd, 4, 5, BONE_DARK)
    rect(14, 12+bd, 4, 4, BONE)
    px(15, 13+bd, BONE_HI); px(16, 13+bd, BONE_HI)

    # ── Skull ─────────────────────────────────────────────────────────────────
    hx = 9
    hy = 2 + bd
    # skull dome
    rect(hx,   hy,   14, 12, BONE_DARK)   # dark outline
    rect(hx+1, hy,   12, 11, BONE)        # skull face
    rect(hx+1, hy,   12,  2, BONE_HI)    # cranium highlight
    # cheekbones
    rect(hx,   hy+7,  2,  3, BONE_HI)
    rect(hx+13,hy+7,  2,  3, BONE_HI)
    # nasal cavity (dark triangle)
    px(hx+6, hy+7, BONE_DARK); px(hx+7, hy+7, BONE_DARK)
    px(hx+6, hy+8, BONE_DARK); px(hx+7, hy+8, BONE_DARK)
    # eye sockets (large dark holes with glow)
    rect(hx+2,  hy+3, 4, 4, BONE_DARK)   # left socket
    rect(hx+2,  hy+4, 3, 2, GLOW)        # glow
    px(hx+3, hy+4, GLOW_DARK)             # glow core
    rect(hx+9,  hy+3, 4, 4, BONE_DARK)   # right socket
    rect(hx+9,  hy+4, 3, 2, GLOW)
    px(hx+10,hy+4, GLOW_DARK)
    # teeth / jaw
    jaw_dy = 2 if jaw_open else 0
    rect(hx+2, hy+11+jaw_dy, 10, 3, BONE_DARK)
    rect(hx+3, hy+11+jaw_dy, 8,  2, BONE)
    # upper teeth
    for tx in range(hx+3, hx+11, 2):
        px(tx, hy+11, BONE_HI)
    # lower teeth (shift when jaw open)
    for tx in range(hx+3, hx+11, 2):
        px(tx, hy+13+jaw_dy, BONE_HI)

    return img


# ── Frame definitions ──────────────────────────────────────────────────────────

idle_frames = [
    make_frame(body_dy=0,  jaw_open=False),
    make_frame(body_dy=-1, jaw_open=False),
    make_frame(body_dy=-1, jaw_open=True),   # jaw chatters
    make_frame(body_dy=0,  jaw_open=False),
]

walk_frames = [
    make_frame(body_dy=-1, l_foot_dy=-3, r_foot_dy=2,  l_arm_dy=3,  r_arm_dy=-3),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  l_arm_dy=0,  r_arm_dy=0),
    make_frame(body_dy=-1, l_foot_dy=2,  r_foot_dy=-3, l_arm_dy=-3, r_arm_dy=3),
    make_frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  l_arm_dy=0,  r_arm_dy=0),
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

# ── Sprite (first idle frame, scaled 2×, cropped to 64×64) ───────────────────
sprite = idle_frames[0].resize((64, 96), Image.NEAREST)
sprite64 = sprite.crop((0, 0, 64, 64))
sprite64.save(OUT_SPRITE)
print(f"Saved {OUT_SPRITE}  ({sprite64.width}×{sprite64.height})")
