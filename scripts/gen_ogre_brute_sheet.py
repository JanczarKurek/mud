"""
Generates assets/overworld_objects/ogre_brute/sheet.png  and  sprite.png
Sheet layout: 4 columns x 2 rows, each frame 64x80 px (large mob, like cyclops)
Sheet size: 256x160 px
  Row 0: idle  (4 frames - slow heave, blink, club rest)
  Row 1: walk  (4 frames - heavy stomp)
"""

from PIL import Image
import os

FRAME_W = 64
FRAME_H = 80
COLS = 4
ROWS = 2
OUT_SHEET = "assets/overworld_objects/ogre_brute/sheet.png"
OUT_SPRITE = "assets/overworld_objects/ogre_brute/sprite.png"

# -- Palette ---------------------------------------------------------------
BG = (0, 0, 0, 0)
SKIN = (150, 118, 82, 255)       # tawny ogre hide
SKIN_DARK = (98, 74, 48, 255)
SKIN_HI = (188, 152, 110, 255)
SKIN_MID = (128, 100, 68, 255)

EYE_WHITE = (235, 225, 190, 255)
EYE_PUPIL = (30, 15, 8, 255)
BROW = (70, 52, 30, 255)

TEETH = (232, 222, 180, 255)
MOUTH_DARK = (40, 22, 10, 255)

HIDE = (92, 62, 30, 255)         # fur loincloth + shoulder strap
HIDE_DARK = (60, 40, 18, 255)

CLUB = (100, 70, 30, 255)
CLUB_DARK = (62, 42, 14, 255)
CLUB_HI = (135, 100, 48, 255)
KNOT = (70, 48, 18, 255)

BELT = (58, 40, 16, 255)


def make_frame(body_dy=0, l_foot_dy=0, r_foot_dy=0,
               l_arm_dy=0, r_arm_dy=0, club_dy=0, blink=False):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry, c)

    bd = body_dy
    cd = club_dy + r_arm_dy

    # -- Club (behind the right arm, resting on the shoulder) ---------------
    rect(46, 8 + bd + cd, 5, 34, CLUB_DARK)
    rect(46, 8 + bd + cd, 4, 33, CLUB)
    rect(47, 9 + bd + cd, 2, 31, CLUB_HI)
    rect(43, 2 + bd + cd, 11, 9, CLUB_DARK)
    rect(44, 1 + bd + cd, 9, 9, CLUB)
    rect(45, 1 + bd + cd, 7, 2, CLUB_HI)
    for kx in (45, 48, 51):
        rect(kx, 4 + bd + cd, 2, 2, KNOT)

    # -- Feet ----------------------------------------------------------------
    lfy = 68 + l_foot_dy
    rect(14, lfy, 11, 8, SKIN_DARK)
    rect(14, lfy, 11, 7, SKIN)
    rect(14, lfy, 4, 2, SKIN_HI)          # toes
    rfy = 68 + r_foot_dy
    rect(37, rfy, 11, 8, SKIN_DARK)
    rect(37, rfy, 11, 7, SKIN)
    rect(44, rfy, 4, 2, SKIN_HI)

    # -- Legs ----------------------------------------------------------------
    rect(16, 54 + bd, 9, 15 + l_foot_dy - bd, SKIN_MID)
    rect(17, 54 + bd, 6, 14 + l_foot_dy - bd, SKIN)
    rect(38, 54 + bd, 9, 15 + r_foot_dy - bd, SKIN_MID)
    rect(39, 54 + bd, 6, 14 + r_foot_dy - bd, SKIN)

    # -- Loincloth ------------------------------------------------------------
    rect(14, 48 + bd, 34, 8, HIDE)
    rect(14, 54 + bd, 34, 2, HIDE_DARK)
    for tx in (18, 26, 34, 42):
        rect(tx, 56 + bd, 3, 3, HIDE_DARK)   # ragged fringe
    rect(14, 48 + bd, 34, 1, BELT)

    # -- Torso (massive barrel) -----------------------------------------------
    rect(12, 24 + bd, 38, 24, SKIN_DARK)
    rect(13, 24 + bd, 36, 23, SKIN)
    rect(15, 26 + bd, 12, 8, SKIN_HI)        # chest highlight
    rect(16, 40 + bd, 30, 3, SKIN_MID)       # belly crease
    rect(24, 34 + bd, 3, 2, SKIN_MID)        # navel
    # Shoulder strap (hide, diagonal)
    for i in range(20):
        px(16 + i, 25 + bd + i // 2, HIDE)
        px(17 + i, 25 + bd + i // 2, HIDE_DARK)

    # -- Arms ----------------------------------------------------------------
    lay = 26 + bd + l_arm_dy
    rect(4, lay, 9, 22, SKIN_DARK)
    rect(5, lay, 7, 21, SKIN)
    rect(5, lay, 3, 4, SKIN_HI)
    rect(4, lay + 20, 9, 6, SKIN_MID)        # fist
    ray = 26 + bd + r_arm_dy
    rect(49, ray, 9, 22, SKIN_DARK)
    rect(50, ray, 7, 21, SKIN)
    rect(54, ray, 3, 4, SKIN_HI)
    rect(49, ray - 4, 9, 6, SKIN)            # right hand gripping the club up top

    # -- Head ----------------------------------------------------------------
    hy = 6 + bd
    rect(20, hy, 22, 19, SKIN_DARK)
    rect(21, hy, 20, 18, SKIN)
    rect(22, hy + 1, 8, 4, SKIN_HI)
    # Ears
    rect(18, hy + 8, 3, 5, SKIN)
    rect(41, hy + 8, 3, 5, SKIN)
    # Brow
    rect(23, hy + 6, 16, 2, BROW)
    # Eyes (two small mean eyes)
    if blink:
        rect(25, hy + 9, 3, 1, SKIN_DARK)
        rect(34, hy + 9, 3, 1, SKIN_DARK)
    else:
        rect(25, hy + 8, 3, 2, EYE_WHITE)
        px(26, hy + 9, EYE_PUPIL)
        rect(34, hy + 8, 3, 2, EYE_WHITE)
        px(35, hy + 9, EYE_PUPIL)
    # Flat nose
    rect(29, hy + 11, 4, 3, SKIN_MID)
    # Mouth with under-bite tusks
    rect(24, hy + 15, 14, 2, MOUTH_DARK)
    px(25, hy + 14, TEETH)
    px(36, hy + 14, TEETH)
    rect(25, hy + 14, 2, 1, TEETH)
    rect(35, hy + 14, 2, 1, TEETH)

    return img


def main() -> None:
    idle = [
        make_frame(0, 0, 0, 0, 0, 0),
        make_frame(1, 0, 0, 0, 0, 1),
        make_frame(1, 0, 0, 0, 0, 1, blink=True),
        make_frame(0, 0, 0, 0, 0, 0),
    ]
    walk = [
        make_frame(0, -3, 0, 2, -2, 0),
        make_frame(1, 0, 0, 0, 0, 0),
        make_frame(0, 0, -3, -2, 2, 0),
        make_frame(1, 0, 0, 0, 0, 0),
    ]
    sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
    for i, frame in enumerate(idle):
        sheet.paste(frame, (i * FRAME_W, 0))
    for i, frame in enumerate(walk):
        sheet.paste(frame, (i * FRAME_W, FRAME_H))

    os.makedirs(os.path.dirname(OUT_SHEET), exist_ok=True)
    sheet.save(OUT_SHEET)
    idle[0].save(OUT_SPRITE)
    print(f"wrote {OUT_SHEET} and {OUT_SPRITE}")


if __name__ == "__main__":
    main()
