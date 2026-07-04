"""
Generates assets/overworld_objects/dire_wight/sheet.png  and  sprite.png
Sheet layout: 4 columns x 2 rows, each frame 32x48 px (standard mob size)
Sheet size: 128x96 px
  Row 0: idle  (4 frames - drifting sway, eye-flare on frame 3)
  Row 1: walk  (4 frames - gliding stride, shroud sway)
"""

from PIL import Image
import os

FRAME_W = 32
FRAME_H = 48
COLS = 4
ROWS = 2
OUT_SHEET = "assets/overworld_objects/dire_wight/sheet.png"
OUT_SPRITE = "assets/overworld_objects/dire_wight/sprite.png"

# -- Palette ---------------------------------------------------------------
BG = (0, 0, 0, 0)
SHROUD = (92, 108, 122, 255)       # grave-grey burial shroud
SHROUD_DARK = (58, 70, 82, 255)
SHROUD_HI = (128, 146, 160, 255)
SKIN = (168, 185, 190, 255)        # corpse-pale skin
SKIN_DARK = (118, 134, 140, 255)
EYE = (120, 220, 255, 255)         # cold blue glow
EYE_FLARE = (200, 245, 255, 255)
MOUTH = (40, 50, 58, 255)
FROST = (190, 225, 240, 200)       # frost motes at the hem
CLAW = (205, 215, 218, 255)


def make_frame(body_dy=0, hem_sway=0, l_arm_dy=0, r_arm_dy=0, flare=False):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for ry in range(h):
            for rx in range(w):
                px(x + rx, y + ry, c)

    bd = body_dy

    # -- Tattered shroud skirt (no feet — it drifts) -------------------------
    rect(10, 30 + bd, 12, 12, SHROUD_DARK)
    rect(11, 30 + bd, 10, 11, SHROUD)
    # Ragged hem, swaying
    for i, hx in enumerate((10, 13, 16, 19)):
        hem_y = 41 + bd + ((i + hem_sway) % 2)
        rect(hx + (hem_sway % 2), hem_y, 2, 2, SHROUD_DARK)
    # Frost motes drifting at the hem
    px(9, 44 + bd, FROST)
    px(15 + hem_sway, 45 + bd, FROST)
    px(22, 44 + bd, FROST)

    # -- Torso ---------------------------------------------------------------
    rect(10, 18 + bd, 12, 13, SHROUD_DARK)
    rect(11, 18 + bd, 10, 12, SHROUD)
    rect(12, 19 + bd, 4, 5, SHROUD_HI)
    # Rope belt
    rect(11, 28 + bd, 10, 1, SHROUD_DARK)

    # -- Arms (long, clawed) ---------------------------------------------------
    lay = 19 + bd + l_arm_dy
    rect(6, lay, 4, 12, SHROUD_DARK)
    rect(7, lay, 2, 11, SHROUD)
    rect(6, lay + 12, 4, 3, SKIN_DARK)      # bony hand
    px(6, lay + 15, CLAW)
    px(8, lay + 15, CLAW)
    ray = 19 + bd + r_arm_dy
    rect(22, ray, 4, 12, SHROUD_DARK)
    rect(23, ray, 2, 11, SHROUD)
    rect(22, ray + 12, 4, 3, SKIN_DARK)
    px(23, ray + 15, CLAW)
    px(25, ray + 15, CLAW)

    # -- Head (gaunt, hooded) ---------------------------------------------------
    hy = 6 + bd
    # Hood
    rect(10, hy, 12, 12, SHROUD_DARK)
    rect(11, hy + 1, 10, 10, SHROUD)
    rect(12, hy + 1, 4, 2, SHROUD_HI)
    # Face pit
    rect(12, hy + 4, 8, 7, MOUTH)
    rect(13, hy + 5, 6, 5, SKIN_DARK)
    rect(13, hy + 5, 6, 3, SKIN)
    # Glowing eyes
    eye = EYE_FLARE if flare else EYE
    px(14, hy + 6, eye)
    px(18, hy + 6, eye)
    if flare:
        px(13, hy + 6, EYE)
        px(19, hy + 6, EYE)
    # Sunken mouth
    rect(15, hy + 9, 3, 1, MOUTH)

    return img


def main() -> None:
    idle = [
        make_frame(0, 0, 0, 0),
        make_frame(1, 1, 0, 0),
        make_frame(1, 1, 0, 0, flare=True),
        make_frame(0, 0, 0, 0),
    ]
    walk = [
        make_frame(0, 0, 1, -1),
        make_frame(1, 1, 0, 0),
        make_frame(0, 0, -1, 1),
        make_frame(1, 1, 0, 0),
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
