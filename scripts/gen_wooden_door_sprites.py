"""
Generates assets/overworld_objects/wooden_door/closed.png and open.png
Heavy wooden door, two static 32x32 frames.
- closed: solid door with planks, hinges, ring handle.
- open: dark passable doorway with door swung inward.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_DIR = "assets/overworld_objects/wooden_door"

BG          = (0,   0,   0,   0)
STONE       = (105, 100,  92, 255)   # door frame stones
STONE_HI    = (145, 140, 130, 255)
STONE_DARK  = ( 65,  62,  55, 255)
WOOD        = (110,  70,  35, 255)   # plank wood
WOOD_HI     = (150,  98,  55, 255)
WOOD_DARK   = ( 70,  42,  18, 255)
WOOD_GRAIN  = ( 90,  55,  22, 255)
IRON        = ( 75,  78,  88, 255)   # hinge / ring iron
IRON_HI     = (130, 132, 142, 255)
IRON_DARK   = ( 40,  42,  50, 255)
RING        = (180, 145,  45, 255)   # brass ring handle
RING_HI     = (240, 200,  85, 255)
RING_DARK   = (115,  90,  20, 255)
DARK_VOID   = ( 18,  14,  10, 255)   # interior darkness through doorway
SHADOW      = (  0,   0,   0,  60)


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


def draw_stone_frame(rect, px):
    """Stone door frame surrounding the doorway."""
    # left jamb (x:3-7, y:3-29)
    rect(3, 3, 5, 27, STONE)
    rect(3, 3, 1, 27, STONE_DARK)
    rect(7, 3, 1, 27, STONE_DARK)
    # right jamb
    rect(24, 3, 5, 27, STONE)
    rect(24, 3, 1, 27, STONE_DARK)
    rect(28, 3, 1, 27, STONE_DARK)
    # top lintel
    rect(3, 3, 26, 4, STONE)
    rect(3, 3, 26, 1, STONE_HI)
    rect(3, 6, 26, 1, STONE_DARK)
    # crude block seams
    for sx in (10, 16, 22):
        rect(sx, 3, 1, 4, STONE_DARK)
    rect(8, 12, 1, 1, STONE_DARK)
    rect(8, 20, 1, 1, STONE_DARK)
    rect(23, 12, 1, 1, STONE_DARK)
    rect(23, 20, 1, 1, STONE_DARK)


def make_closed():
    img = new_img()
    px, rect = make_helpers(img)
    draw_stone_frame(rect, px)

    # Door panel (x:8-23, y:7-29)
    rect(8, 7, 16, 22, WOOD)
    # outer door bevel
    rect(8, 7, 16, 1, WOOD_HI)        # top highlight
    rect(8, 28, 16, 1, WOOD_DARK)     # bottom shadow
    rect(8, 7, 1, 22, WOOD_HI)        # left highlight
    rect(23, 7, 1, 22, WOOD_DARK)     # right shadow

    # Plank seams (vertical) — 4 planks across
    for sx in (12, 16, 20):
        rect(sx, 8, 1, 20, WOOD_DARK)
        rect(sx + 1, 8, 1, 20, WOOD_GRAIN)

    # Cross planks / iron straps (top + bottom)
    rect(8, 9, 16, 2, IRON)
    rect(8, 9, 16, 1, IRON_HI)
    rect(8, 10, 16, 1, IRON_DARK)
    rect(8, 25, 16, 2, IRON)
    rect(8, 25, 16, 1, IRON_HI)
    rect(8, 26, 16, 1, IRON_DARK)

    # Iron strap rivets
    for x in (9, 14, 18, 22):
        px(x, 9, IRON_DARK)
        px(x, 26, IRON_DARK)

    # Hinges (left side, top + bottom)
    rect( 8, 11, 3, 3, IRON)
    px(  8, 11, IRON_HI)
    px( 10, 13, IRON_DARK)
    rect( 8, 22, 3, 3, IRON)
    px(  8, 22, IRON_HI)
    px( 10, 24, IRON_DARK)

    # Brass ring handle (right side, mid-height)
    # ring outline (donut)
    ring_pts = [
        (19, 17), (20, 17), (21, 17),
        (18, 18),                     (22, 18),
        (18, 19),                     (22, 19),
        (18, 20),                     (22, 20),
        (19, 21), (20, 21), (21, 21),
    ]
    for (x, y) in ring_pts:
        px(x, y, RING)
    # highlight
    px(19, 17, RING_HI)
    px(20, 17, RING_HI)
    px(18, 18, RING_HI)
    # shadow
    px(22, 20, RING_DARK)
    px(21, 21, RING_DARK)
    px(20, 21, RING_DARK)
    # back-plate behind ring
    px(20, 16, IRON)
    px(20, 15, IRON_DARK)

    return img


def make_open():
    """Door swung inward — show stone frame, dark interior, edge of door panel."""
    img = new_img()
    px, rect = make_helpers(img)
    draw_stone_frame(rect, px)

    # Doorway opening: dark void inside (x:8-23, y:7-29)
    rect(8, 7, 16, 22, DARK_VOID)
    # Threshold (slightly lit floor at the bottom of the opening)
    rect(8, 27, 16, 2, (45, 38, 30, 255))
    rect(8, 27, 16, 1, (75, 60, 40, 255))

    # Inner stone shadow (depth cue along edges)
    rect(8, 7, 1, 22, (35, 30, 25, 255))
    rect(23, 7, 1, 22, (35, 30, 25, 255))
    rect(8, 7, 16, 1, (35, 30, 25, 255))

    # Open door panel — sliver visible on the right, pushed inward
    # (suggests the door swung inward to the right)
    rect(20, 8, 4, 20, WOOD)
    rect(20, 8, 1, 20, WOOD_HI)        # leading edge
    rect(23, 8, 1, 20, WOOD_DARK)
    rect(20, 8, 4, 1, WOOD_HI)
    rect(20, 27, 4, 1, WOOD_DARK)
    # plank seam on the open panel
    rect(22, 9, 1, 18, WOOD_DARK)
    # iron strap stub on the visible panel
    rect(20, 10, 4, 1, IRON)
    rect(20, 25, 4, 1, IRON)
    px(21, 10, IRON_HI)
    px(21, 25, IRON_HI)

    # Hinges on the left jamb (door has swung off them; show empty hinge plates)
    rect(7, 11, 2, 3, IRON_DARK)
    rect(7, 22, 2, 3, IRON_DARK)
    px(8, 11, IRON)
    px(8, 22, IRON)

    # A faint warm light spilling in from outside near top of opening
    px(12, 8, (60, 50, 35, 255))
    px(13, 8, (60, 50, 35, 255))

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
