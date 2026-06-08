"""
Generates a static 32x32 PNG sprite for the `poison_flask` pickup:

  poison_flask  — a round-bottom apothecary vial with a cork stopper, filled
                  with viscous near-black venom that catches a sickly
                  green/purple sheen. A pale highlight reads it as glass.

Single `sprite.png` in the object's overworld_objects directory.
Style matches gen_casting_item_sprites.py — chunky pixels, 2-3 shading levels,
no anti-aliasing, transparent background, small ground shadow.
"""

from __future__ import annotations

import math
import os

from PIL import Image

W, H = 32, 32
BG = (0, 0, 0, 0)
SHADOW = (0, 0, 0, 70)


def new_img() -> Image.Image:
    return Image.new("RGBA", (W, H), BG)


def helpers(img: Image.Image):
    def px(x: int, y: int, c):
        if 0 <= x < W and 0 <= y < H:
            img.putpixel((x, y), c)

    def rect(x: int, y: int, w: int, h: int, c):
        for dy in range(h):
            for dx in range(w):
                px(x + dx, y + dy, c)

    return px, rect


def ground_shadow(rect, cx: int, cy: int, half: int) -> None:
    rect(cx - half, cy, half * 2 + 1, 1, SHADOW)
    inner = max(half - 2, 1)
    rect(cx - inner, cy + 1, inner * 2 + 1, 1, SHADOW)


def make_poison_flask() -> Image.Image:
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 27, 6)

    glass_dark = (34, 48, 46, 255)
    glass = (92, 120, 116, 150)        # translucent empty glass (air gap)
    glass_hi = (205, 232, 228, 225)
    poison_dark = (16, 11, 22, 255)
    poison = (34, 26, 44, 255)
    poison_sheen = (70, 100, 66, 255)  # sickly green meniscus
    poison_hi = (108, 78, 134, 255)    # purple sheen glints
    cork = (124, 88, 52, 255)
    cork_dark = (84, 58, 32, 255)
    cork_hi = (162, 122, 78, 255)

    cx = 16
    bcy, br = 20, 6        # round bulb centre + radius
    liquid_top = 16        # rows >= this inside the glass hold poison

    # Membership sets so we can outline cleanly afterwards.
    bulb = set()
    for y in range(bcy - br, bcy + br + 1):
        dy = y - bcy
        half = int(round(math.sqrt(max(br * br - dy * dy, 0))))
        for x in range(cx - half, cx + half + 1):
            bulb.add((x, y))

    neck = set()
    for y in range(8, 15):           # narrow neck rising out of the bulb
        for x in range(cx - 1, cx + 2):
            neck.add((x, y))

    glassware = bulb | neck

    # Fill: glass above the liquid line, poison below it.
    for (x, y) in glassware:
        if y >= liquid_top:
            px(x, y, poison if (x + y) % 5 else poison_dark)
        else:
            px(x, y, glass)

    # Meniscus — a thin sickly-green band where the venom meets the air.
    for x in range(cx - 5, cx + 6):
        if (x, liquid_top) in glassware:
            px(x, liquid_top, poison_sheen)

    # A couple of suspended glints / bubbles in the venom.
    for (x, y) in [(14, 19), (18, 22), (15, 23)]:
        if (x, y) in glassware:
            px(x, y, poison_hi)

    # Dark glass outline: any glassware pixel touching empty space.
    for (x, y) in glassware:
        for (nx, ny) in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if (nx, ny) not in glassware:
                px(x, y, glass_dark)
                break

    # Glass highlight streak down the upper-left curve.
    for (x, y) in [(12, 16), (11, 17), (11, 18), (12, 19), (13, 21), (15, 9), (15, 10)]:
        if (x, y) in glassware:
            px(x, y, glass_hi)

    # Cork stopper — sits on top of the neck, slightly wider.
    rect(cx - 2, 5, 5, 3, cork)
    rect(cx - 2, 5, 5, 1, cork_hi)        # lit top face
    px(cx - 2, 7, cork_dark)
    px(cx + 2, 7, cork_dark)
    rect(cx - 1, 7, 3, 1, cork_dark)      # shadow where cork meets neck
    px(cx + 2, 6, cork_dark)              # right-side shade

    return img


def main() -> None:
    out_dir = os.path.join("assets", "overworld_objects", "poison_flask")
    os.makedirs(out_dir, exist_ok=True)
    path = os.path.join(out_dir, "sprite.png")
    make_poison_flask().save(path)
    print(f"Saved {path}")


if __name__ == "__main__":
    main()
