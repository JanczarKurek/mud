"""
Generates assets/overworld_objects/lever/off.png and on.png
Wall-mounted iron lever; two static 32x32 frames.
"""

from PIL import Image
import os

W, H = 32, 32
OUT_DIR = "assets/overworld_objects/lever"

BG          = (0,   0,   0,   0)
PLATE       = ( 95,  95, 105, 255)   # iron mounting plate
PLATE_HI    = (140, 142, 152, 255)
PLATE_DARK  = ( 55,  55,  65, 255)
SHAFT       = (130, 130, 140, 255)   # polished iron shaft
SHAFT_HI    = (190, 192, 200, 255)
SHAFT_DARK  = ( 70,  70,  78, 255)
KNOB        = (180, 140,  40, 255)   # brass knob
KNOB_HI     = (240, 210,  90, 255)
KNOB_DARK   = (110,  82,  18, 255)
GLOW        = (230, 200,  90, 200)   # subtle warm glow when activated
RIVET       = ( 50,  50,  58, 255)
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


def draw_plate(rect, px):
    # Mounting plate: rounded rectangle, x:11-20, y:10-22
    rect(12, 10, 8, 12, PLATE)
    # rounded corners — chip the four corner pixels
    px(12, 10, BG); px(19, 10, BG)
    px(12, 21, BG); px(19, 21, BG)
    # bevel
    rect(12, 11, 8, 1, PLATE_HI)
    rect(12, 21, 8, 1, PLATE_DARK)
    rect(12, 11, 1, 10, PLATE_HI)
    rect(19, 11, 1, 10, PLATE_DARK)
    # rivets in the four corners
    for (rx, ry) in [(13, 12), (18, 12), (13, 20), (18, 20)]:
        px(rx, ry, RIVET)
    # central pivot circle (where the lever rotates)
    rect(15, 15, 2, 2, RIVET)


def make_off():
    """Lever pointing DOWN-LEFT (off position)."""
    img = new_img()
    px, rect = make_helpers(img)

    # Ground shadow (just a faint hint below the plate)
    for ox in range(-3, 4):
        px(15 + ox, 27, SHADOW)

    draw_plate(rect, px)

    # Shaft: from pivot (15,16) angling down-left to about (10, 24)
    # Drawn as thick diagonal line, 2 px wide
    shaft_pts = [
        (15, 16), (14, 17), (14, 18), (13, 19),
        (12, 20), (12, 21), (11, 22), (10, 23),
    ]
    for (x, y) in shaft_pts:
        px(x, y, SHAFT)
        px(x + 1, y, SHAFT)
        px(x, y - 1, SHAFT_HI)
        px(x + 1, y + 1, SHAFT_DARK)

    # Knob at the end (lower-left tip)
    rect( 8, 23, 4, 3, KNOB)
    px( 8, 23, BG); px(11, 25, BG)  # round corners
    rect( 8, 23, 4, 1, KNOB_HI)
    rect( 8, 25, 4, 1, KNOB_DARK)
    px( 9, 24, KNOB_HI)

    return img


def make_on():
    """Lever pointing UP-RIGHT (on position) with a faint warm glow."""
    img = new_img()
    px, rect = make_helpers(img)

    for ox in range(-3, 4):
        px(15 + ox, 27, SHADOW)

    draw_plate(rect, px)

    # Shaft: from pivot (16,15) angling up-right to about (22, 7)
    shaft_pts = [
        (16, 15), (17, 14), (17, 13), (18, 12),
        (19, 11), (19, 10), (20,  9), (21,  8),
    ]
    for (x, y) in shaft_pts:
        px(x, y, SHAFT)
        px(x + 1, y, SHAFT)
        px(x, y - 1, SHAFT_HI)
        px(x + 1, y + 1, SHAFT_DARK)

    # Knob at upper-right tip
    rect(21, 6, 4, 3, KNOB)
    px(21, 6, BG); px(24, 8, BG)
    rect(21, 6, 4, 1, KNOB_HI)
    rect(21, 8, 4, 1, KNOB_DARK)
    px(22, 7, KNOB_HI)

    # Subtle warm glow around knob (semi-transparent)
    for (gx, gy) in [(20, 5), (25, 5), (26, 6), (26, 8), (25, 9), (20, 9), (19, 7)]:
        px(gx, gy, GLOW)

    return img


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    off = make_off()
    off.save(os.path.join(OUT_DIR, "off.png"))
    print(f"Saved {OUT_DIR}/off.png ({off.width}×{off.height})")

    on = make_on()
    on.save(os.path.join(OUT_DIR, "on.png"))
    print(f"Saved {OUT_DIR}/on.png ({on.width}×{on.height})")


if __name__ == "__main__":
    main()
