"""
Generates sprite PNGs for ranged-combat assets:
  - arrow/sprite.png       (32x32 pickup icon)
  - bolt/sprite.png        (32x32 pickup icon)
  - archer_goblin/sprite.png (32x48 standing sprite)
  - archer_goblin/sheet.png  (4x2, 32x48 frames: idle + walk)

(Bow and crossbow pickup icons live in gen_combat_gear_sprites.py.)

The archer_goblin is a green-skinned goblin with a gray tunic, carrying a
shortbow in its right hand.
"""

from PIL import Image
import os

BG = (0, 0, 0, 0)


def new_image(w, h):
    return Image.new("RGBA", (w, h), BG)


def draw_rect(img, x, y, w, h, color):
    for ry in range(h):
        for rx in range(w):
            xi, yi = x + rx, y + ry
            if 0 <= xi < img.width and 0 <= yi < img.height:
                img.putpixel((xi, yi), color)


def draw_px(img, x, y, color):
    if 0 <= x < img.width and 0 <= y < img.height:
        img.putpixel((x, y), color)


# ── Arrow sprite (32x32 diagonal arrow) ───────────────────────────────────────
def render_arrow():
    img = new_image(32, 32)
    SHAFT = (170, 150, 110, 255)
    SHAFT_DARK = (120, 100, 70, 255)
    TIP = (150, 150, 155, 255)
    TIP_DARK = (90, 90, 95, 255)
    FEATHER = (210, 200, 170, 255)
    FEATHER_DARK = (150, 140, 110, 255)
    # Diagonal from top-right (tip) to bottom-left (fletching).
    # Shaft: step 1 px at each y.
    for i in range(22):
        x = 22 - i
        y = 6 + i
        draw_px(img, x, y, SHAFT)
        draw_px(img, x - 1, y, SHAFT_DARK)
    # Arrow head (triangle at top-right).
    draw_px(img, 25, 3, TIP)
    draw_rect(img, 23, 4, 3, 1, TIP)
    draw_rect(img, 22, 5, 4, 1, TIP)
    draw_rect(img, 21, 6, 3, 1, TIP)
    draw_px(img, 26, 4, TIP_DARK)
    draw_px(img, 26, 5, TIP_DARK)
    # Fletching (bottom-left).
    for i in range(4):
        y = 26 + i - 2
        x = 4 + i
        draw_px(img, x, y, FEATHER)
        draw_px(img, x, y + 1, FEATHER_DARK)
        draw_px(img, x - 1, y + 1, FEATHER)
    return img


# ── Bolt sprite (32x32 shorter, stubbier) ─────────────────────────────────────
def render_bolt():
    img = new_image(32, 32)
    SHAFT = (120, 110, 92, 255)
    SHAFT_DARK = (80, 72, 58, 255)
    TIP = (150, 150, 155, 255)
    TIP_DARK = (90, 90, 95, 255)
    FIN = (180, 175, 160, 255)
    # Horizontal bolt centered vertically.
    for x in range(10, 24):
        draw_px(img, x, 15, SHAFT)
        draw_px(img, x, 16, SHAFT_DARK)
    # Tip (right side, triangular).
    draw_rect(img, 24, 14, 2, 3, TIP)
    draw_px(img, 26, 15, TIP)
    draw_px(img, 26, 16, TIP_DARK)
    draw_px(img, 25, 17, TIP_DARK)
    # Fins (left side).
    draw_rect(img, 7, 13, 3, 1, FIN)
    draw_rect(img, 7, 17, 3, 1, FIN)
    draw_px(img, 8, 14, FIN)
    draw_px(img, 8, 16, FIN)
    return img


# ── Archer Goblin frames (32x48) ──────────────────────────────────────────────
SKIN = (112, 176, 92, 255)
SKIN_DARK = (68, 108, 56, 255)
SKIN_HI = (148, 208, 116, 255)
EYE = (255, 220, 30, 255)
PUPIL = (20, 20, 20, 255)
MOUTH = (40, 20, 10, 255)
TUNIC = (80, 80, 92, 255)
TUNIC_DARK = (52, 52, 62, 255)
BELT = (80, 60, 40, 255)
PANTS = (56, 44, 30, 255)
BOOT = (40, 30, 18, 255)
BOOT_HI = (60, 46, 28, 255)
TOOTH = (240, 230, 180, 255)
CLAW = (200, 200, 120, 255)
BOW_WOOD = (140, 95, 48, 255)
BOW_DARK = (92, 58, 26, 255)
BOW_STRING = (230, 224, 200, 255)


def _goblin_body(img, body_dy, l_foot_dy, r_foot_dy, l_arm_dy, r_arm_dy, blink):
    def px(x, y, c):
        draw_px(img, x, y, c)

    def rect(x, y, w, h, c, dy=0):
        draw_rect(img, x, y + dy, w, h, c)

    # Boots
    lby = 38 + l_foot_dy
    rect(10, lby, 4, 6, BOOT)
    rect(10, lby, 4, 1, BOOT_HI)
    rect(9, lby + 2, 1, 3, BOOT)
    rby = 38 + r_foot_dy
    rect(18, rby, 4, 6, BOOT)
    rect(18, rby, 4, 1, BOOT_HI)
    rect(22, rby + 2, 1, 3, BOOT)

    bd = body_dy
    rect(10, 30 + bd, 4, 9, PANTS)
    rect(18, 30 + bd, 4, 9, PANTS)
    rect(14, 30 + bd, 4, 4, PANTS)
    rect(9, 28 + bd, 14, 3, BELT)
    # Tunic
    rect(9, 18 + bd, 14, 11, TUNIC)
    rect(9, 18 + bd, 1, 11, TUNIC_DARK)
    rect(22, 18 + bd, 1, 11, TUNIC_DARK)
    # Left arm (holds bow)
    lad = l_arm_dy
    rect(7, 20 + bd + lad, 3, 8, TUNIC)
    rect(7, 28 + bd + lad, 3, 3, SKIN)
    rect(7, 31 + bd + lad, 3, 2, CLAW)
    px(6, 29 + bd + lad, SKIN_DARK)
    # Right arm (draws string)
    rad = r_arm_dy
    rect(22, 20 + bd + rad, 3, 8, TUNIC)
    rect(22, 28 + bd + rad, 3, 3, SKIN)
    rect(22, 31 + bd + rad, 3, 2, CLAW)
    px(25, 29 + bd + rad, SKIN_DARK)
    # Neck
    rect(14, 15 + bd, 4, 4, SKIN)
    # Head
    hx, hy = 8, 4 + bd
    rect(hx, hy, 16, 14, SKIN)
    rect(hx, hy, 1, 14, SKIN_DARK)
    rect(hx + 15, hy, 1, 14, SKIN_DARK)
    rect(hx, hy, 16, 1, SKIN_HI)
    px(hx - 1, hy + 5, SKIN)
    px(hx - 1, hy + 6, SKIN)
    px(hx + 16, hy + 5, SKIN)
    px(hx + 16, hy + 6, SKIN)
    if blink:
        rect(hx + 2, hy + 5, 3, 1, PUPIL)
        rect(hx + 11, hy + 5, 3, 1, PUPIL)
    else:
        rect(hx + 2, hy + 4, 4, 4, EYE)
        rect(hx + 10, hy + 4, 4, 4, EYE)
        rect(hx + 3, hy + 5, 2, 2, PUPIL)
        rect(hx + 11, hy + 5, 2, 2, PUPIL)
    px(hx + 7, hy + 8, SKIN_DARK)
    px(hx + 8, hy + 8, SKIN_DARK)
    rect(hx + 3, hy + 10, 10, 2, MOUTH)
    px(hx + 5, hy + 10, TOOTH)
    px(hx + 10, hy + 10, TOOTH)
    # Ears
    px(hx - 2, hy + 3, SKIN)
    px(hx - 2, hy + 4, SKIN)
    px(hx - 3, hy + 3, SKIN_DARK)
    px(hx + 18, hy + 3, SKIN)
    px(hx + 18, hy + 4, SKIN)
    px(hx + 19, hy + 3, SKIN_DARK)

    # Shortbow held in left hand: vertical curved bow to the side of the arm.
    bow_by_y = {
        22: 5, 23: 4, 24: 3, 25: 3, 26: 3, 27: 3, 28: 3,
        29: 3, 30: 3, 31: 3, 32: 3, 33: 3, 34: 3, 35: 4, 36: 5,
    }
    for y, x in bow_by_y.items():
        draw_px(img, x, y + bd, BOW_DARK)
        draw_px(img, x + 1, y + bd, BOW_WOOD)
    # Nocks
    draw_px(img, 6, 21 + bd, BOW_DARK)
    draw_px(img, 6, 37 + bd, BOW_DARK)
    # Bowstring
    for y in range(22, 37):
        draw_px(img, 5, y + bd, BOW_STRING)


def render_archer_frame(body_dy=0, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0, blink=False):
    img = new_image(32, 48)
    _goblin_body(img, body_dy, l_foot_dy, r_foot_dy, l_arm_dy, r_arm_dy, blink)
    return img


def render_archer_sheet():
    FRAME_W, FRAME_H, COLS, ROWS = 32, 48, 4, 2
    sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
    idle = [
        render_archer_frame(body_dy=0),
        render_archer_frame(body_dy=-1),
        render_archer_frame(body_dy=-1),
        render_archer_frame(body_dy=0, blink=True),
    ]
    walk = [
        render_archer_frame(body_dy=-1, l_foot_dy=-3, r_foot_dy=2, l_arm_dy=3, r_arm_dy=-3),
        render_archer_frame(body_dy=1, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0),
        render_archer_frame(body_dy=-1, l_foot_dy=2, r_foot_dy=-3, l_arm_dy=-3, r_arm_dy=3),
        render_archer_frame(body_dy=1, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0),
    ]
    for col, frame in enumerate(idle):
        sheet.paste(frame, (col * FRAME_W, 0))
    for col, frame in enumerate(walk):
        sheet.paste(frame, (col * FRAME_W, FRAME_H))
    return sheet


OUTPUTS = [
    ("assets/overworld_objects/arrow/sprite.png", render_arrow),
    ("assets/overworld_objects/bolt/sprite.png", render_bolt),
    ("assets/overworld_objects/archer_goblin/sprite.png", render_archer_frame),
    ("assets/overworld_objects/archer_goblin/sheet.png", render_archer_sheet),
]

for path, fn in OUTPUTS:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img = fn()
    img.save(path)
    print(f"Saved {path}  ({img.width}x{img.height})")
