"""
Generates assets/overworld_objects/iron_chest/closed.png and open.png
Two static 32x32 frames: closed lid with iron bands, open lid revealing gold.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_DIR = "assets/overworld_objects/iron_chest"

BG          = (0,   0,   0,   0)
WOOD        = (110,  70,  35, 255)   # warm dark wood
WOOD_HI     = (150,  98,  55, 255)
WOOD_DARK   = ( 70,  42,  18, 255)
IRON        = ( 90,  92, 100, 255)   # gunmetal iron band
IRON_HI     = (140, 142, 150, 255)
IRON_DARK   = ( 50,  52,  58, 255)
LOCK        = (200, 170,  60, 255)   # brassy lock plate
LOCK_DARK   = (130, 100,  30, 255)
KEYHOLE     = ( 25,  20,  10, 255)
GOLD        = (240, 210,  70, 255)   # treasure gold
GOLD_HI     = (255, 240, 150, 255)
GOLD_DARK   = (170, 140,  20, 255)
INNER       = ( 35,  25,  15, 255)   # interior shadow
SHADOW      = (  0,   0,   0,  70)   # ground shadow


def new_img():
    return Image.new("RGBA", (W, H), BG)


def make_helpers(img):
    def px(x, y, c):
        if 0 <= x < W and 0 <= y < H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for dy in range(h):
            for dx in range(w):
                px(x + dx, y + dy, c)

    return px, rect


def draw_ground_shadow(rect):
    # subtle ellipse under the chest
    for ox in range(-9, 10):
        rect(15 + ox, 27, 1, 1, SHADOW)
    for ox in range(-7, 8):
        rect(16 + ox, 28, 1, 1, SHADOW)


def make_closed():
    img = new_img()
    px, rect = make_helpers(img)
    draw_ground_shadow(rect)

    # ── Body (chest box) ────────────────────────────────────────────────────
    # base body x:5-26, y:14-26
    rect(5, 14, 22, 13, WOOD)
    # outer dark border
    rect(5, 14,  1, 13, WOOD_DARK)
    rect(26, 14, 1, 13, WOOD_DARK)
    rect(5, 26, 22, 1, WOOD_DARK)
    # vertical wood plank lines
    for x in (10, 15, 20):
        rect(x, 15, 1, 11, WOOD_DARK)

    # ── Lid (closed, sits on top, slight overhang) ─────────────────────────
    # lid x:4-27, y:8-15  (slightly wider than body)
    rect(4, 8, 24, 7, WOOD)
    rect(4, 8, 24, 1, WOOD_HI)         # top highlight
    rect(4, 14, 24, 1, WOOD_DARK)      # under-lid shadow
    rect(4, 8, 1, 7, WOOD_DARK)
    rect(27, 8, 1, 7, WOOD_DARK)

    # ── Iron bands (3 vertical) ─────────────────────────────────────────────
    for x in (8, 23):
        rect(x, 8, 2, 18, IRON)
        rect(x, 8, 1, 18, IRON_DARK)   # left edge shadow
        rect(x + 1, 8, 1, 1, IRON_HI)  # top highlight
    # central band passes around lock
    rect(15, 8, 2, 6, IRON)
    rect(15, 8, 1, 6, IRON_DARK)
    rect(15, 18, 2, 8, IRON)
    rect(15, 18, 1, 8, IRON_DARK)

    # rivets on the bands
    for (rx, ry) in [(8, 10), (8, 24), (23, 10), (23, 24)]:
        px(rx, ry, IRON_HI)
        px(rx + 1, ry + 1, IRON_DARK)

    # ── Lock plate (centered on body, just below lid seam) ──────────────────
    rect(13, 14, 6, 6, LOCK)
    rect(13, 14, 6, 1, GOLD_HI) if False else None
    rect(13, 14, 1, 6, LOCK_DARK)
    rect(18, 14, 1, 6, LOCK_DARK)
    rect(13, 19, 6, 1, LOCK_DARK)
    # keyhole
    rect(15, 16, 2, 1, KEYHOLE)
    px(15, 17, KEYHOLE)
    px(16, 17, KEYHOLE)
    px(15, 18, KEYHOLE)

    # iron strap on top of lid (front-facing band cap)
    rect(8, 8, 2, 1, IRON_HI)
    rect(23, 8, 2, 1, IRON_HI)

    return img


def make_open():
    img = new_img()
    px, rect = make_helpers(img)
    draw_ground_shadow(rect)

    # ── Body (same as closed, but lid is gone — show interior) ──────────────
    rect(5, 14, 22, 13, WOOD)
    rect(5, 14,  1, 13, WOOD_DARK)
    rect(26, 14, 1, 13, WOOD_DARK)
    rect(5, 26, 22, 1, WOOD_DARK)
    for x in (10, 15, 20):
        rect(x, 15, 1, 11, WOOD_DARK)

    # iron bands on body only
    for x in (8, 23):
        rect(x, 14, 2, 12, IRON)
        rect(x, 14, 1, 12, IRON_DARK)
    # rivets
    for (rx, ry) in [(8, 16), (8, 24), (23, 16), (23, 24)]:
        px(rx, ry, IRON_HI)

    # ── Open interior (top rim + dark hollow) ───────────────────────────────
    # Top edge of opening (x:5-27, y:13)
    rect(5, 13, 23, 1, WOOD_DARK)
    # interior shadow (visible through opening)
    rect(7, 14, 19, 5, INNER)
    # subtle inner highlight (back wall)
    rect(7, 14, 19, 1, (60, 40, 25, 255))

    # ── Lid (raised back, tilted away) ──────────────────────────────────────
    # back edge of lid (top, behind the opening)
    rect(3, 4, 26, 4, WOOD)
    rect(3, 4, 26, 1, WOOD_HI)
    rect(3, 7, 26, 1, WOOD_DARK)
    rect(3, 4, 1, 4, WOOD_DARK)
    rect(28, 4, 1, 4, WOOD_DARK)
    # iron bands across the raised lid
    for x in (8, 15, 23):
        rect(x, 4, 2, 4, IRON)
        rect(x, 4, 1, 4, IRON_DARK)
        px(x + 1, 4, IRON_HI)
    # back of lid casts a shadow line into the opening
    rect(5, 8, 22, 1, (40, 25, 12, 255))

    # hinge nubs on either side at the back of the chest
    px(5, 13, IRON_DARK)
    px(26, 13, IRON_DARK)

    # ── Treasure: gold coins piled inside ───────────────────────────────────
    # base of gold pile (rounded mound)
    rect(9, 18, 14, 4, GOLD)
    rect(9, 18, 14, 1, GOLD_HI)
    rect(9, 21, 14, 1, GOLD_DARK)
    rect(9, 18, 1, 4, GOLD_DARK)
    rect(22, 18, 1, 4, GOLD_DARK)
    # individual coin highlights
    for (cx, cy) in [(11, 19), (15, 19), (19, 19), (13, 20), (17, 20)]:
        px(cx, cy, GOLD_HI)

    # one coin spilling onto the rim
    px(20, 17, GOLD)
    px(20, 16, GOLD_HI)

    return img


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    closed = make_closed()
    closed.save(os.path.join(OUT_DIR, "closed.png"))
    print(f"Saved {OUT_DIR}/closed.png ({closed.width}×{closed.height})")

    opened = make_open()
    opened.save(os.path.join(OUT_DIR, "open.png"))
    print(f"Saved {OUT_DIR}/open.png ({opened.width}×{opened.height})")


if __name__ == "__main__":
    main()
