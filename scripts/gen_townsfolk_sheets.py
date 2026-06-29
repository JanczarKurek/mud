"""
Generates the townsfolk sprite set under assets/overworld_objects/townsfolk/:

  sheet.png  – base anim, 4 cols × 2 rows (128×96): row0 idle, row1 walk
  work.png   – "working" pose, 4 cols × 1 row (128×48): hammer up/down cycle
  sleep.png  – "sleeping" pose, 2 cols × 1 row (64×48): slumped + Zzz bob
  sprite.png – single static idle frame (32×48) used as the no-animation fallback

A friendly villager: tan skin, brown hair, blue tunic under a leather apron.
Modeled on scripts/gen_goblin_sheet.py (same 32×48 rect-built humanoid) so the
townsfolk reads at the same scale and style as the other NPCs. The `work` and
`sleep` sheets are the per-state animation sheets driven by the routine system
(`ObjectState("working")` / `("sleeping")`) — see docs/yaml_formats.md.
"""

import os

from PIL import Image

FRAME_W = 32
FRAME_H = 48

OUT_DIR = "assets/overworld_objects/townsfolk"

# ── Palette ────────────────────────────────────────────────────────────────────
BG          = (0, 0, 0, 0)
SKIN        = (214, 165, 120, 255)
SKIN_DARK   = (172, 124,  86, 255)
SKIN_HI     = (235, 188, 146, 255)
HAIR        = (104,  68,  38, 255)
HAIR_HI     = (138,  94,  54, 255)
EYE_WHITE   = (238, 238, 240, 255)
PUPIL       = ( 40,  34,  30, 255)
MOUTH       = (150,  92,  74, 255)
TUNIC       = ( 64, 104, 138, 255)
TUNIC_DARK  = ( 44,  74, 100, 255)
APRON       = (158, 116,  74, 255)
APRON_DARK  = (120,  84,  52, 255)
BELT        = ( 92,  62,  30, 255)
PANTS       = ( 70,  58,  46, 255)
PANTS_DARK  = ( 50,  40,  32, 255)
BOOT        = ( 46,  34,  22, 255)
BOOT_HI     = ( 70,  52,  34, 255)
HAMMER_WOOD = (132,  92,  52, 255)
HAMMER_HEAD = (110, 112, 120, 255)
HAMMER_HI   = (160, 162, 170, 255)
SPARK       = (255, 214, 120, 255)
ZZZ         = (226, 232, 244, 255)


def make_frame(
    *,
    body_dy=0,
    l_foot_dy=0,
    r_foot_dy=0,
    l_arm_dy=0,
    r_arm_dy=0,
    blink=False,
    hammer=False,
    spark=False,
    zzz=0,
):
    """Draw one 32×48 townsfolk frame.

    body_dy / foot_dy / arm_dy – vertical offsets for breathing, stride, swing.
    blink   – closed eyes (idle blink, or held shut while sleeping).
    hammer  – draw a hammer in the right hand (work pose); follows r_arm_dy.
    spark   – draw a couple of struck-anvil sparks (work down-stroke).
    zzz     – 0 none, else size of the floating "Z" (sleep pose).
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

    # ── Boots ──────────────────────────────────────────────────────────────────
    lby = 40 + l_foot_dy
    rect(10, lby, 4, 5, BOOT)
    rect(10, lby, 4, 1, BOOT_HI)
    rby = 40 + r_foot_dy
    rect(18, rby, 4, 5, BOOT)
    rect(18, rby, 4, 1, BOOT_HI)

    # ── Legs / pants ───────────────────────────────────────────────────────────
    rect(10, 31 + bd, 4, 10, PANTS)
    rect(18, 31 + bd, 4, 10, PANTS)
    rect(14, 31 + bd, 4, 5, PANTS)
    rect(10, 31 + bd, 1, 10, PANTS_DARK)
    rect(21, 31 + bd, 1, 10, PANTS_DARK)

    # ── Belt ───────────────────────────────────────────────────────────────────
    rect(9, 29 + bd, 14, 2, BELT)

    # ── Tunic torso ────────────────────────────────────────────────────────────
    rect(9, 18 + bd, 14, 11, TUNIC)
    rect(9, 18 + bd, 1, 11, TUNIC_DARK)
    rect(22, 18 + bd, 1, 11, TUNIC_DARK)
    # Leather apron over the tunic front.
    rect(12, 20 + bd, 8, 11, APRON)
    rect(12, 20 + bd, 1, 11, APRON_DARK)
    rect(19, 20 + bd, 1, 11, APRON_DARK)
    rect(14, 18 + bd, 4, 2, APRON)  # bib

    # ── Left arm (down at side) ────────────────────────────────────────────────
    lad = l_arm_dy
    rect(7, 20 + bd + lad, 3, 8, TUNIC)
    rect(7, 28 + bd + lad, 3, 3, SKIN)  # forearm
    px(6, 27 + bd + lad, SKIN_DARK)

    # ── Right arm (raises for the work pose) ───────────────────────────────────
    rad = r_arm_dy
    rect(22, 20 + bd + rad, 3, 8, TUNIC)
    rect(22, 28 + bd + rad, 3, 3, SKIN)  # forearm + hand
    px(25, 27 + bd + rad, SKIN_DARK)

    if hammer:
        # Handle rises from the right hand; head sits at the top of the handle.
        hand_y = 30 + bd + rad
        rect(24, hand_y - 8, 2, 9, HAMMER_WOOD)  # handle
        rect(22, hand_y - 11, 6, 3, HAMMER_HEAD)  # head
        rect(22, hand_y - 11, 6, 1, HAMMER_HI)
        if spark:
            px(20, 38 + bd, SPARK)
            px(23, 39 + bd, SPARK)
            px(26, 37 + bd, SPARK)

    # ── Neck ───────────────────────────────────────────────────────────────────
    rect(14, 15 + bd, 4, 4, SKIN)

    # ── Head ───────────────────────────────────────────────────────────────────
    hx, hy = 9, 4 + bd
    rect(hx, hy, 14, 13, SKIN)
    rect(hx, hy, 1, 13, SKIN_DARK)
    rect(hx + 13, hy, 1, 13, SKIN_DARK)

    # Hair: cap over the top + sideburns.
    rect(hx, hy, 14, 4, HAIR)
    rect(hx, hy, 14, 1, HAIR_HI)
    rect(hx, hy, 1, 7, HAIR)
    rect(hx + 13, hy, 1, 7, HAIR)

    # Eyes
    if blink:
        rect(hx + 3, hy + 7, 3, 1, PUPIL)
        rect(hx + 9, hy + 7, 3, 1, PUPIL)
    else:
        rect(hx + 3, hy + 6, 3, 3, EYE_WHITE)
        rect(hx + 9, hy + 6, 3, 3, EYE_WHITE)
        rect(hx + 4, hy + 7, 2, 2, PUPIL)
        rect(hx + 10, hy + 7, 2, 2, PUPIL)

    # Nose + mouth
    px(hx + 7, hy + 9, SKIN_DARK)
    rect(hx + 5, hy + 11, 5, 1, MOUTH)

    # ── Floating Zzz (sleep) ───────────────────────────────────────────────────
    if zzz:
        zx, zy = 24, 2
        if zzz == 1:
            _draw_z(px, zx, zy, 3)
        else:
            _draw_z(px, zx - 1, zy - 1, 4)
            _draw_z(px, zx + 2, zy + 3, 2)

    return img


def _draw_z(px, x, y, s):
    """A tiny 'Z' glyph of side `s` at top-left (x, y)."""
    for i in range(s):
        px(x + i, y, ZZZ)          # top bar
        px(x + i, y + s - 1, ZZZ)  # bottom bar
        px(x + (s - 1 - i), y + i, ZZZ)  # diagonal


# ── Frame sets ──────────────────────────────────────────────────────────────────

IDLE_FRAMES = [
    make_frame(body_dy=0, blink=False),
    make_frame(body_dy=-1, blink=False),
    make_frame(body_dy=-1, blink=False),
    make_frame(body_dy=0, blink=True),
]

WALK_FRAMES = [
    make_frame(body_dy=-1, l_foot_dy=-3, r_foot_dy=2, l_arm_dy=3, r_arm_dy=-3),
    make_frame(body_dy=1, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0),
    make_frame(body_dy=-1, l_foot_dy=2, r_foot_dy=-3, l_arm_dy=-3, r_arm_dy=3),
    make_frame(body_dy=1, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0),
]

# Work: raise the hammer high, then strike down with sparks.
WORK_FRAMES = [
    make_frame(hammer=True, r_arm_dy=-7),
    make_frame(hammer=True, r_arm_dy=-3),
    make_frame(hammer=True, r_arm_dy=2, spark=True),
    make_frame(hammer=True, r_arm_dy=-3),
]

# Sleep: slumped, eyes shut, a bobbing pair of Z's.
SLEEP_FRAMES = [
    make_frame(body_dy=2, blink=True, zzz=1),
    make_frame(body_dy=3, blink=True, zzz=2),
]


def assemble(rows):
    cols = max(len(r) for r in rows)
    sheet = Image.new("RGBA", (FRAME_W * cols, FRAME_H * len(rows)), BG)
    for row_idx, frames in enumerate(rows):
        for col_idx, frame in enumerate(frames):
            sheet.paste(frame, (col_idx * FRAME_W, row_idx * FRAME_H))
    return sheet


def save(image, name):
    os.makedirs(OUT_DIR, exist_ok=True)
    path = os.path.join(OUT_DIR, name)
    image.save(path)
    print(f"Saved {path}  ({image.width}×{image.height})")


def main():
    save(assemble([IDLE_FRAMES, WALK_FRAMES]), "sheet.png")
    save(assemble([WORK_FRAMES]), "work.png")
    save(assemble([SLEEP_FRAMES]), "sleep.png")
    save(IDLE_FRAMES[0], "sprite.png")


if __name__ == "__main__":
    main()
