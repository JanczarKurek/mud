"""
Generates assets/overworld_objects/iron_chest/closed.png and open.png
Two static 48×24 frames (bottom-anchored, half a tile tall): iron-banded
wooden chest viewed from a low three-quarter angle. The chest's footprint
sits on the lower edge of the canvas; the lid rises above for the open state.
"""

from PIL import Image
import os

W, H = 48, 24
OUT_DIR = "assets/overworld_objects/iron_chest"

BG          = (  0,   0,   0,   0)
WOOD        = (110,  70,  35, 255)
WOOD_HI     = (150,  98,  55, 255)
WOOD_DARK   = ( 70,  42,  18, 255)
IRON        = ( 90,  92, 100, 255)
IRON_HI     = (140, 142, 150, 255)
IRON_DARK   = ( 50,  52,  58, 255)
LOCK        = (200, 170,  60, 255)
LOCK_DARK   = (130, 100,  30, 255)
KEYHOLE     = ( 25,  20,  10, 255)
GOLD        = (240, 210,  70, 255)
GOLD_HI     = (255, 240, 150, 255)
GOLD_DARK   = (170, 140,  20, 255)
INNER       = ( 35,  25,  15, 255)
SHADOW      = (  0,   0,   0,  70)


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
    # thin elliptical shadow hugging the chest base
    for ox in range(-19, 20):
        rect(24 + ox, 22, 1, 1, SHADOW)
    for ox in range(-16, 17):
        rect(24 + ox, 23, 1, 1, SHADOW)


def draw_body(rect, px):
    """Box body: spans most of the canvas vertically, sits on bottom."""
    # body: x 4..43, y 8..21
    rect(4, 8, 40, 14, WOOD)
    # outer borders
    rect(4,  8,  1, 14, WOOD_DARK)
    rect(43, 8,  1, 14, WOOD_DARK)
    rect(4, 21, 40,  1, WOOD_DARK)
    # vertical plank lines
    for x in (12, 20, 28, 36):
        rect(x, 9, 1, 12, WOOD_DARK)
    # subtle top highlight on body front
    rect(5, 8, 38, 1, WOOD_HI)


def draw_body_iron(rect, px):
    """Iron bands wrapping the chest body, NOT the lid."""
    # left & right corner bands
    for x in (5, 41):
        rect(x, 8, 2, 14, IRON)
        rect(x, 8, 1, 14, IRON_DARK)
        px(x + 1, 8, IRON_HI)
    # center band (will pass behind lock on closed state)
    rect(23, 8, 2, 14, IRON)
    rect(23, 8, 1, 14, IRON_DARK)
    # rivets along bands
    for (rx, ry) in [(5, 11), (5, 19), (23, 11), (23, 19), (41, 11), (41, 19)]:
        px(rx, ry, IRON_HI)
        px(rx + 1, ry + 1, IRON_DARK)


def make_closed():
    img = new_img()
    px, rect = make_helpers(img)
    draw_ground_shadow(rect)

    # ── Body sits on bottom of canvas ──────────────────────────────────────
    draw_body(rect, px)

    # ── Lid (closed) — overhangs the body slightly on x ────────────────────
    # lid: x 3..44, y 2..9
    rect(3, 2, 42, 7, WOOD)
    rect(3, 2, 42, 1, WOOD_HI)          # top edge highlight
    rect(3, 8, 42, 1, WOOD_DARK)        # under-lid shadow line
    rect(3, 2,  1, 7, WOOD_DARK)
    rect(44, 2, 1, 7, WOOD_DARK)

    # iron straps that wrap top of lid (continuation of body bands)
    for x in (5, 23, 41):
        rect(x, 2, 2, 7, IRON)
        rect(x, 2, 1, 7, IRON_DARK)
        px(x + 1, 2, IRON_HI)

    # body iron bands (after lid so they read in front of plank lines)
    draw_body_iron(rect, px)

    # ── Lock plate (centered on body, just under the lid seam) ─────────────
    rect(21, 11, 6, 6, LOCK)
    rect(21, 11, 6, 1, GOLD_HI)
    rect(21, 11, 1, 6, LOCK_DARK)
    rect(26, 11, 1, 6, LOCK_DARK)
    rect(21, 16, 6, 1, LOCK_DARK)
    # keyhole
    px(23, 13, KEYHOLE)
    px(24, 13, KEYHOLE)
    px(23, 14, KEYHOLE)
    px(24, 14, KEYHOLE)
    px(23, 15, KEYHOLE)

    return img


def make_open():
    img = new_img()
    px, rect = make_helpers(img)
    draw_ground_shadow(rect)

    # ── Body (with no lid; show interior rim across the top) ──────────────
    draw_body(rect, px)
    draw_body_iron(rect, px)

    # Top rim of opening
    rect(4, 8, 40, 1, WOOD_DARK)
    # Interior shadow visible through opening
    rect(6, 9, 36, 4, INNER)
    # back-wall subtle highlight
    rect(6, 9, 36, 1, (60, 40, 25, 255))

    # ── Lid (raised, tilted back) — drawn above the body, top of canvas ───
    # back-edge slab
    rect(2, 0, 44, 3, WOOD)
    rect(2, 0, 44, 1, WOOD_HI)
    rect(2, 2, 44, 1, WOOD_DARK)
    rect(2, 0,  1, 3, WOOD_DARK)
    rect(45, 0, 1, 3, WOOD_DARK)
    # iron straps across raised lid
    for x in (5, 23, 41):
        rect(x, 0, 2, 3, IRON)
        rect(x, 0, 1, 3, IRON_DARK)
        px(x + 1, 0, IRON_HI)
    # shadow cast by raised lid into the opening
    rect(4, 9, 40, 1, (40, 25, 12, 255))

    # ── Treasure: gold coins piled inside (compressed for half-tile) ──────
    rect(9, 13, 30, 4, GOLD)
    rect(9, 13, 30, 1, GOLD_HI)
    rect(9, 16, 30, 1, GOLD_DARK)
    rect(9, 13, 1, 4, GOLD_DARK)
    rect(38, 13, 1, 4, GOLD_DARK)
    for (cx, cy) in [(13, 14), (19, 14), (25, 14), (31, 14), (16, 15), (22, 15), (28, 15)]:
        px(cx, cy, GOLD_HI)

    # one coin spilling onto the front rim
    px(32, 11, GOLD)
    px(32, 10, GOLD_HI)

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
