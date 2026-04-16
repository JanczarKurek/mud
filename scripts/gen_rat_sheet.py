"""
Generates assets/overworld_objects/rat/sheet.png  and  sprite.png
Sheet layout: 4 columns × 2 rows, each frame 32×48 px
  Row 0: idle  (4 frames – sitting upright, sniffing bob, ear twitch, blink)
  Row 1: walk  (4 frames – scurrying on all fours)
"""

from PIL import Image
import os

FRAME_W = 32
FRAME_H = 48
COLS    = 4
ROWS    = 2
OUT_SHEET  = "assets/overworld_objects/rat/sheet.png"
OUT_SPRITE = "assets/overworld_objects/rat/sprite.png"

# ── Palette ────────────────────────────────────────────────────────────────────
BG        = (  0,   0,   0,   0)   # transparent
FUR       = (130, 105,  80, 255)   # grey-brown main fur
FUR_DARK  = ( 82,  62,  40, 255)   # outline / shadow
FUR_HI    = (168, 142, 110, 255)   # fur highlight / back
BELLY     = (178, 152, 118, 255)   # lighter underbelly / muzzle
EAR_IN    = (210, 138, 140, 255)   # inner ear pink
EYE       = (195,  38,  38, 255)   # beady red
EYE_DARK  = ( 90,  10,  10, 255)   # pupil
NOSE      = (220, 148, 148, 255)   # pink nose
TAIL      = (112,  88,  64, 255)   # tail base
TAIL_DARK = ( 70,  52,  34, 255)   # tail outline / tip
CLAW      = (198, 182, 148, 255)   # pale claws


# ─────────────────────────────────────────────────────────────────────────────
#  IDLE FRAME  (sitting upright on haunches)
# ─────────────────────────────────────────────────────────────────────────────

def draw_idle(body_dy=0, ear_dx=0, blink=False):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry, c)

    bd = body_dy   # whole-body vertical shift

    # ── Tail (behind body, drawn first so body overlaps) ─────────────────────
    for ty, tx in enumerate(range(24, 29)):
        px(tx, 30 + ty + bd, TAIL)
        px(tx, 31 + ty + bd, TAIL)
    # curl tip
    rect(27, 38+bd, 2, 3, TAIL_DARK)
    rect(25, 40+bd, 2, 2, TAIL)

    # ── Hind haunches ────────────────────────────────────────────────────────
    # left haunch
    rect(8,  32+bd, 6, 10, FUR_DARK)
    rect(9,  31+bd, 5,  9, FUR)
    rect(10, 31+bd, 3,  2, FUR_HI)
    # right haunch
    rect(18, 32+bd, 6, 10, FUR_DARK)
    rect(18, 31+bd, 5,  9, FUR)
    rect(18, 31+bd, 3,  2, FUR_HI)
    # hind paws on ground
    rect(8,  40+bd, 6, 4, FUR_DARK)
    rect(9,  39+bd, 5, 4, FUR)
    px(9,  43+bd, CLAW); px(11, 43+bd, CLAW); px(13, 43+bd, CLAW)
    rect(18, 40+bd, 6, 4, FUR_DARK)
    rect(18, 39+bd, 5, 4, FUR)
    px(18, 43+bd, CLAW); px(20, 43+bd, CLAW); px(22, 43+bd, CLAW)

    # ── Body (round blob) ────────────────────────────────────────────────────
    rect(9,  22+bd, 14, 12, FUR_DARK)
    rect(10, 21+bd, 12, 11, FUR)
    rect(11, 21+bd, 10,  2, FUR_HI)
    # belly / chest (lighter front centre)
    rect(11, 26+bd,  9,  7, BELLY)

    # ── Front paws ───────────────────────────────────────────────────────────
    rect(10, 31+bd, 3, 7, FUR)
    rect(10, 37+bd, 3, 2, FUR_DARK)
    px(10, 38+bd, CLAW); px(12, 38+bd, CLAW)
    rect(19, 31+bd, 3, 7, FUR)
    rect(19, 37+bd, 3, 2, FUR_DARK)
    px(19, 38+bd, CLAW); px(21, 38+bd, CLAW)

    # ── Neck / throat ────────────────────────────────────────────────────────
    rect(13, 18+bd, 6, 5, FUR)
    rect(13, 19+bd, 6, 3, BELLY)

    # ── Head ─────────────────────────────────────────────────────────────────
    hx = 9
    hy = 5 + bd
    rect(hx,   hy,   14, 14, FUR_DARK)   # shadow outline
    rect(hx+1, hy,   12, 13, FUR)        # head
    rect(hx+2, hy,   10,  2, FUR_HI)    # top highlight
    # muzzle / snout (slightly elongated)
    rect(hx+2, hy+8,  10,  5, BELLY)
    rect(hx+2, hy+11, 10,  2, FUR_DARK)  # chin

    # ── Ears ─────────────────────────────────────────────────────────────────
    # left ear
    rect(hx,   hy-4, 4, 5, FUR)
    rect(hx+1, hy-3, 2, 3, EAR_IN)
    px(hx,   hy-4, FUR_DARK)
    # right ear (twitches via ear_dx)
    ex = hx + 10 + ear_dx
    rect(ex,   hy-4, 4, 5, FUR)
    rect(ex+1, hy-3, 2, 3, EAR_IN)
    px(ex+3, hy-4, FUR_DARK)

    # ── Eyes ─────────────────────────────────────────────────────────────────
    if blink:
        rect(hx+2, hy+4, 3, 1, FUR_DARK)
        rect(hx+9, hy+4, 3, 1, FUR_DARK)
    else:
        rect(hx+2, hy+3, 3, 3, EYE)
        px(hx+3,  hy+4, EYE_DARK)
        rect(hx+9, hy+3, 3, 3, EYE)
        px(hx+10, hy+4, EYE_DARK)

    # ── Nose ─────────────────────────────────────────────────────────────────
    rect(hx+4, hy+10, 5, 2, NOSE)
    # whisker dots (outer cheeks)
    px(hx,   hy+9, FUR_DARK)
    px(hx+1, hy+9, FUR_DARK)
    px(hx+13,hy+9, FUR_DARK)

    return img


# ─────────────────────────────────────────────────────────────────────────────
#  WALK FRAME  (scurrying on all fours, body elongated / low)
# ─────────────────────────────────────────────────────────────────────────────

def draw_walk(phase=0):
    """
    phase 0 – front-L / hind-R forward
    phase 1 – contact / neutral (body sags)
    phase 2 – front-R / hind-L forward
    phase 3 – contact / neutral (body sags)
    """
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry, c)

    # contact frames sag 1 px
    sag = 1 if phase in (1, 3) else 0
    by = 22 + sag   # body top y

    # ── Tail (trailing at left) ───────────────────────────────────────────────
    tail_pts = [
        (7, by+8), (6, by+7), (5, by+6), (4, by+5),
        (4, by+4), (5, by+3), (5, by+2), (4, by+1),
    ]
    for (tx, ty) in tail_pts:
        px(tx, ty, TAIL)
    for (tx, ty) in tail_pts[::2]:
        px(tx-1, ty, TAIL_DARK)

    # ── Body (elongated horizontal blob) ─────────────────────────────────────
    rect(7,  by+2, 18, 10, FUR_DARK)
    rect(8,  by+1, 16,  9, FUR)
    rect(9,  by+1, 14,  2, FUR_HI)
    # belly underside
    rect(8,  by+7, 16,  3, BELLY)

    # ── Four legs ────────────────────────────────────────────────────────────
    # Positions: front pair ~x 18-22, hind pair ~x 8-12
    # Each leg: upper seg (4 px tall), lower seg+paw (5 px tall)

    if phase == 0:
        # Front-L forward (raised), Front-R back; Hind-R forward, Hind-L back
        # Front-L (forward / raised)
        rect(21, by+9,  3, 4, FUR);  rect(21, by+12, 3, 5, FUR)
        px(21, by+16, CLAW); px(23, by+16, CLAW)
        # Front-R (trailing / lower)
        rect(16, by+11, 3, 4, FUR_DARK); rect(16, by+14, 3, 3, FUR_DARK)
        # Hind-R (forward / raised)
        rect(8,  by+8,  3, 5, FUR);  rect(8,  by+12, 3, 5, FUR)
        px(8,  by+16, CLAW); px(10, by+16, CLAW)
        # Hind-L (trailing / lower)
        rect(12, by+10, 3, 4, FUR_DARK); rect(12, by+13, 3, 3, FUR_DARK)

    elif phase == 2:
        # Opposite diagonal
        # Front-R (forward)
        rect(18, by+8,  3, 5, FUR);  rect(18, by+12, 3, 5, FUR)
        px(18, by+16, CLAW); px(20, by+16, CLAW)
        # Front-L (trailing)
        rect(22, by+11, 3, 4, FUR_DARK); rect(22, by+14, 3, 3, FUR_DARK)
        # Hind-L (forward)
        rect(11, by+8,  3, 5, FUR);  rect(11, by+12, 3, 5, FUR)
        px(11, by+16, CLAW); px(13, by+16, CLAW)
        # Hind-R (trailing)
        rect(8,  by+10, 3, 4, FUR_DARK); rect(8,  by+13, 3, 3, FUR_DARK)

    else:
        # Contact – all four legs planted vertically
        rect(21, by+9, 3, 8, FUR);  px(21, by+16, CLAW); px(23, by+16, CLAW)
        rect(16, by+9, 3, 8, FUR)
        rect(11, by+9, 3, 8, FUR);  px(11, by+16, CLAW); px(13, by+16, CLAW)
        rect(8,  by+9, 3, 8, FUR)

    # ── Head (front / right end of body) ─────────────────────────────────────
    hx = 20
    hy = by - 6
    rect(hx,   hy,   12, 10, FUR_DARK)
    rect(hx+1, hy,   10,  9, FUR)
    rect(hx+2, hy,    8,  2, FUR_HI)
    # snout forward (right)
    rect(hx+7, hy+3,  5,  4, BELLY)
    rect(hx+9, hy+5,  3,  2, NOSE)
    # eye (single visible eye on this side)
    rect(hx+2, hy+3, 3, 3, EYE)
    px(hx+3, hy+4, EYE_DARK)
    # ear on top
    rect(hx+2, hy-3, 3, 4, FUR)
    rect(hx+3, hy-2, 1, 2, EAR_IN)
    # whisker dot
    px(hx+11, hy+5, FUR_DARK)

    return img


# ── Frame lists ────────────────────────────────────────────────────────────────
idle_frames = [
    draw_idle(body_dy=0,  ear_dx=0,  blink=False),   # neutral sit
    draw_idle(body_dy=-1, ear_dx=0,  blink=False),   # sniff up
    draw_idle(body_dy=-1, ear_dx=1,  blink=False),   # ear twitch
    draw_idle(body_dy=0,  ear_dx=0,  blink=True),    # blink / nose twitch
]

walk_frames = [
    draw_walk(phase=0),   # front-L / hind-R forward
    draw_walk(phase=1),   # contact
    draw_walk(phase=2),   # front-R / hind-L forward
    draw_walk(phase=3),   # contact
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

# ── Sprite (first idle frame, scaled 2×) ──────────────────────────────────────
sprite = idle_frames[0].resize((64, 96), Image.NEAREST)
# Crop to 64×64 (take the middle/upper portion showing the rat)
sprite64 = sprite.crop((0, 0, 64, 64))
sprite64.save(OUT_SPRITE)
print(f"Saved {OUT_SPRITE}  ({sprite64.width}×{sprite64.height})")
