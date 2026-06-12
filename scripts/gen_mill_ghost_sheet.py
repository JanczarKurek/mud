"""
Generates assets/modules/haunted_mill/overworld_objects/mill_ghost/sheet.png
A translucent pale-blue badger ghost (Old Maple, the drowned miller) in a
flour-dusted apron, stooped and sad. No feet — the lower body trails off into
wispy tendrils that drift.

Sheet layout: 4 columns × 2 rows, each frame 32×48 px
  Row 0: idle (gentle bob + alpha shimmer + blink)
  Row 1: "walk" (a drifting bob — ghosts don't stride; apron + arms + wisps sway)
"""

from PIL import Image, ImageDraw
import os

FRAME_W = 32
FRAME_H = 48
COLS = 4
ROWS = 2
OUT_PATH = "assets/modules/haunted_mill/overworld_objects/mill_ghost/sheet.png"

# ── Palette (low alpha = ghostly/translucent) ───────────────────────────────────
BG          = (0, 0, 0, 0)
A           = 165                       # base ghost alpha
GHOST       = (192, 216, 238, A)        # pale blue body
GHOST_DK    = (150, 180, 212, A)        # shadow
GHOST_HI    = (226, 240, 255, A + 20)   # highlight
STRIPE      = (78,  94, 122, A + 25)    # badger facial stripes / back
SNOUT       = (210, 226, 244, A)        # pale muzzle
NOSE        = (60,  72,  98, A + 40)
EYE         = (44,  56,  84, A + 60)    # soft, sad dark eyes
EYE_HI      = (210, 226, 248, A + 40)
APRON       = (224, 224, 210, A + 15)   # flour-white apron
APRON_DK    = (188, 188, 174, A + 15)
APRON_DUST  = (240, 240, 230, A + 25)   # flour smudges
TIE         = (150, 150, 138, A + 15)
PAW         = (170, 196, 222, A)


def make_frame(body_dy=0, l_arm_dy=0, r_arm_dy=0, wisp=0, blink=False, shimmer=0):
    """One 32×48 ghost-badger frame.
    body_dy  – vertical drift of the whole apparition
    l/r_arm_dy – arm sway
    wisp     – tendril phase (0/1/2) for the trailing lower body
    blink    – half-closed sad eyes
    shimmer  – +/- alpha nudge for the idle flicker
    """
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def shift(c):
        if shimmer == 0:
            return c
        r, g, b, a = c
        return (r, g, b, max(0, min(255, a + shimmer)))

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), shift(c))

    def rect(x, y, w, h, c, dy=0):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry + dy, c)

    bd = body_dy

    # ── Wispy trailing tail (no feet) ───────────────────────────────────────────
    # Three tendrils that splay differently per `wisp` phase.
    base_y = 37 + bd
    tendrils = {
        0: [(12, 0), (16, 2), (20, 0)],
        1: [(11, 2), (16, 0), (21, 2)],
        2: [(13, 1), (16, 3), (19, 1)],
    }[wisp]
    for (tx, extra) in tendrils:
        h = 5 + extra
        for i in range(h):
            w = 3 if i < h - 2 else 1
            # taper + fade toward the tip
            fade = int(A * (1 - i / (h + 1)))
            rect(tx - w // 2, base_y + i, w, 1, (GHOST_DK[0], GHOST_DK[1], GHOST_DK[2], max(30, fade)))

    # ── Apron skirt ─────────────────────────────────────────────────────────────
    rect(10, 30 + bd, 13, 8, APRON, 0)
    rect(10, 30 + bd, 1, 8, APRON_DK, 0)
    rect(22, 30 + bd, 1, 8, APRON_DK, 0)
    rect(10, 37 + bd, 13, 1, APRON_DK, 0)
    px(14, 34 + bd, APRON_DUST)            # flour smudges
    px(18, 33 + bd, APRON_DUST)
    px(16, 36 + bd, APRON_DUST)

    # ── Torso (hunched/stooped: leans forward, rounded back) ─────────────────────
    rect(10, 19 + bd, 13, 12, GHOST, 0)
    rect(10, 19 + bd, 1, 12, STRIPE, 0)    # rounded back shadow (left)
    rect(9, 20 + bd, 1, 9, GHOST_DK, 0)    # extra back hunch
    rect(22, 19 + bd, 1, 12, GHOST_DK, 0)

    # Apron bib over the chest
    rect(13, 20 + bd, 7, 11, APRON, 0)
    rect(13, 20 + bd, 7, 1, APRON_DK, 0)
    rect(14, 19 + bd, 1, 2, TIE, 0)        # neck ties
    rect(18, 19 + bd, 1, 2, TIE, 0)
    px(16, 26 + bd, APRON_DUST)

    # ── Arms (hang forward, end in soft paws) ────────────────────────────────────
    lad = l_arm_dy
    rect(8, 21 + bd + lad, 3, 8, GHOST, 0)
    rect(8, 28 + bd + lad, 3, 2, PAW, 0)
    rad = r_arm_dy
    rect(21, 21 + bd + rad, 3, 8, GHOST, 0)
    rect(21, 28 + bd + rad, 3, 2, PAW, 0)

    # ── Neck (short, tilted forward) ─────────────────────────────────────────────
    rect(13, 16 + bd, 5, 4, GHOST, 0)

    # ── Head (badger: pale with two dark stripes, pointed snout) ─────────────────
    hx, hy = 9, 5 + bd
    rect(hx, hy, 14, 12, GHOST, 0)
    rect(hx, hy, 14, 1, GHOST_HI, 0)       # top highlight
    rect(hx, hy, 1, 12, GHOST_DK, 0)
    rect(hx + 13, hy, 1, 12, GHOST_DK, 0)

    # Badger stripes: two dark bands from the brow down over each eye
    rect(hx + 2, hy + 1, 2, 8, STRIPE, 0)
    rect(hx + 10, hy + 1, 2, 8, STRIPE, 0)
    # White blaze down the centre + muzzle
    rect(hx + 5, hy + 1, 4, 11, SNOUT, 0)

    # Snout tip (points down — stooped, looking at the floor)
    rect(hx + 5, hy + 11, 4, 2, SNOUT, 0)
    rect(hx + 6, hy + 12, 2, 1, NOSE, 0)

    # Sad eyes, set in the stripes, looking down
    if blink:
        rect(hx + 2, hy + 6, 3, 1, EYE, 0)
        rect(hx + 9, hy + 6, 3, 1, EYE, 0)
    else:
        rect(hx + 2, hy + 5, 3, 3, EYE, 0)
        rect(hx + 9, hy + 5, 3, 3, EYE, 0)
        px(hx + 2, hy + 5, EYE_HI)         # tiny catch-light
        px(hx + 9, hy + 5, EYE_HI)

    # Ears (small rounded, pale-rimmed)
    rect(hx, hy - 1, 3, 2, GHOST, 0)
    rect(hx + 11, hy - 1, 3, 2, GHOST, 0)
    px(hx, hy - 1, GHOST_DK)
    px(hx + 13, hy - 1, GHOST_DK)

    return img


# ── Frames ───────────────────────────────────────────────────────────────────────
idle_frames = [
    make_frame(body_dy=0,  wisp=0, shimmer=0,   blink=False),
    make_frame(body_dy=-1, wisp=1, shimmer=15,  blink=False),
    make_frame(body_dy=-1, wisp=2, shimmer=-10, blink=False),
    make_frame(body_dy=0,  wisp=1, shimmer=0,   blink=True),
]

# "Walk": a gentle drift — bob + apron/arm/wisp sway, no foot stride.
walk_frames = [
    make_frame(body_dy=-1, l_arm_dy=2,  r_arm_dy=-2, wisp=1, shimmer=10),
    make_frame(body_dy=1,  l_arm_dy=0,  r_arm_dy=0,  wisp=2, shimmer=0),
    make_frame(body_dy=-1, l_arm_dy=-2, r_arm_dy=2,  wisp=0, shimmer=10),
    make_frame(body_dy=1,  l_arm_dy=0,  r_arm_dy=0,  wisp=1, shimmer=0),
]

# ── Assemble ───────────────────────────────────────────────────────────────────────
sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
for col, frame in enumerate(idle_frames):
    sheet.paste(frame, (col * FRAME_W, 0))
for col, frame in enumerate(walk_frames):
    sheet.paste(frame, (col * FRAME_W, FRAME_H))

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
sheet.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({sheet.width}×{sheet.height})")
