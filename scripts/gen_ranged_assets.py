"""
Generates sprite PNGs for ranged-combat assets:
  - arrow/sprite.png       (32x32 pickup icon)
  - bolt/sprite.png        (32x32 pickup icon)

(Bow and crossbow pickup icons live in gen_combat_gear_sprites.py. The
archer_goblin sheets moved to the 4-facing rig in gen_goblin_sheets.py.)
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


OUTPUTS = [
    ("assets/overworld_objects/arrow/sprite.png", render_arrow),
    ("assets/overworld_objects/bolt/sprite.png", render_bolt),
]

for path, fn in OUTPUTS:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img = fn()
    img.save(path)
    print(f"Saved {path}  ({img.width}x{img.height})")
