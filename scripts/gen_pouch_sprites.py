"""
Generates static 32x32 pouch sprites for `small_pouch` and `herb_pouch`.
"""

from __future__ import annotations

import os
from PIL import Image

W, H = 32, 32
BG = (0, 0, 0, 0)
SHADOW = (0, 0, 0, 70)


def new_img() -> Image.Image:
    return Image.new("RGBA", (W, H), BG)


def make_helpers(img: Image.Image):
    def px(x: int, y: int, c):
        if 0 <= x < W and 0 <= y < H:
            img.putpixel((x, y), c)

    def rect(x: int, y: int, w: int, h: int, c):
        for dy in range(h):
            for dx in range(w):
                px(x + dx, y + dy, c)

    return px, rect


def small_pouch() -> Image.Image:
    img = new_img()
    px, rect = make_helpers(img)
    LEATHER     = (128,  90,  56, 255)
    LEATHER_HI  = (170, 124,  78, 255)
    LEATHER_DK  = ( 80,  56,  32, 255)
    DRAW        = (200, 168, 100, 255)  # drawstring (tan rope)
    DRAW_DK     = (140, 110,  60, 255)

    # ground shadow
    rect(8, 27, 17, 1, SHADOW)
    rect(10, 28, 13, 1, SHADOW)

    # body of pouch — pear-shaped sack
    # widest at y=20
    rect(10, 14, 13, 13, LEATHER)
    rect(11, 13, 11, 1, LEATHER)
    rect(12, 12, 9, 1, LEATHER)
    # bottom curve
    rect(11, 26, 11, 1, LEATHER)
    rect(12, 27, 9, 1, LEATHER_DK)

    # outline / shadow on right & bottom
    rect(22, 14, 1, 13, LEATHER_DK)
    rect(10, 14, 1, 13, LEATHER_DK)
    rect(11, 26, 11, 1, LEATHER_DK)
    # highlight on left
    rect(11, 15, 1, 9, LEATHER_HI)
    px(12, 14, LEATHER_HI)

    # neck (cinched by drawstring)
    rect(13, 10, 7, 3, LEATHER)
    rect(13, 10, 1, 3, LEATHER_DK)
    rect(19, 10, 1, 3, LEATHER_DK)
    px(14, 10, LEATHER_HI)

    # drawstring loops
    rect(12, 9, 9, 1, DRAW)
    px(12, 9, DRAW_DK)
    px(20, 9, DRAW_DK)
    # tied loops sticking up
    px(13, 7, DRAW); px(13, 8, DRAW)
    px(19, 7, DRAW); px(19, 8, DRAW)
    px(13, 6, DRAW_DK)
    px(19, 6, DRAW_DK)

    # subtle stitch line
    for x in range(12, 22, 2):
        px(x, 24, LEATHER_DK)

    return img


def herb_pouch() -> Image.Image:
    img = new_img()
    px, rect = make_helpers(img)
    LINEN       = (180, 184, 130, 255)  # off-white linen with herb tinge
    LINEN_HI    = (215, 218, 168, 255)
    LINEN_DK    = (118, 124,  80, 255)
    HERB        = ( 92, 132,  60, 255)
    HERB_DK     = ( 56,  82,  36, 255)
    HERB_HI     = (140, 180,  90, 255)
    DRAW        = (160, 124,  80, 255)
    DRAW_DK     = (100,  72,  44, 255)

    # ground shadow
    rect(6, 28, 21, 1, SHADOW)
    rect(8, 29, 17, 1, SHADOW)

    # body — wider, two-pocket pouch
    rect(7, 13, 19, 15, LINEN)
    rect(7, 13, 19, 1, LINEN_HI)
    rect(7, 27, 19, 1, LINEN_DK)
    rect(7, 13, 1, 15, LINEN_DK)
    rect(25, 13, 1, 15, LINEN_DK)

    # vertical seam splitting the two compartments
    rect(16, 13, 1, 14, LINEN_DK)

    # neck cinch
    rect(10, 10, 13, 3, LINEN)
    rect(10, 10, 1, 3, LINEN_DK)
    rect(22, 10, 1, 3, LINEN_DK)
    rect(10, 10, 13, 1, LINEN_HI)

    # drawstring
    rect(9, 9, 15, 1, DRAW)
    px(9, 9, DRAW_DK)
    px(23, 9, DRAW_DK)
    # ties
    px(11, 7, DRAW); px(11, 8, DRAW)
    px(21, 7, DRAW); px(21, 8, DRAW)
    px(11, 6, DRAW_DK); px(21, 6, DRAW_DK)

    # herb sprigs poking out
    px(12, 11, HERB); px(12, 10, HERB); px(12,  9, HERB_HI)
    px(13, 10, HERB); px(13,  9, HERB)
    px(20, 11, HERB); px(20, 10, HERB); px(20,  9, HERB_HI)
    px(19, 10, HERB_DK)

    # decorative stitching on each pocket
    for x in range(9, 16, 2):
        px(x, 25, LINEN_DK)
    for x in range(18, 25, 2):
        px(x, 25, LINEN_DK)

    return img


def main() -> None:
    out_root = "assets/overworld_objects"
    os.makedirs(os.path.join(out_root, "small_pouch"), exist_ok=True)
    os.makedirs(os.path.join(out_root, "herb_pouch"), exist_ok=True)

    sp = small_pouch()
    sp.save(os.path.join(out_root, "small_pouch", "sprite.png"))
    print(f"Saved {out_root}/small_pouch/sprite.png")

    hp = herb_pouch()
    hp.save(os.path.join(out_root, "herb_pouch", "sprite.png"))
    print(f"Saved {out_root}/herb_pouch/sprite.png")


if __name__ == "__main__":
    main()
