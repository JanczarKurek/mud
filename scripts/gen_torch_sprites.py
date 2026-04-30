"""
Generates assets/overworld_objects/torch/unlit.png and lit_sheet.png
- unlit.png: 32x32 static, wall sconce holding cold/charred head
- lit_sheet.png: 128x32 (4 cols x 1 row), animated flame above same sconce
"""

from PIL import Image
import os

W, H = 32, 32
COLS = 4
OUT_DIR = "assets/overworld_objects/torch"

BG          = (0,   0,   0,   0)
BRACKET     = ( 80,  82,  92, 255)   # iron sconce
BRACKET_HI  = (130, 132, 142, 255)
BRACKET_DK  = ( 45,  47,  55, 255)
RIVET       = ( 30,  30,  35, 255)
HANDLE      = ( 95,  60,  25, 255)   # wooden handle
HANDLE_HI   = (140,  92,  40, 255)
HANDLE_DK   = ( 60,  35,  10, 255)
CHAR        = ( 35,  28,  22, 255)   # charred unlit tip
CHAR_HI     = ( 70,  55,  40, 255)
EMBER       = (220,  80,  10, 255)
FLAME_LO    = (235, 120,  15, 255)
FLAME_MID   = (250, 195,  35, 255)
FLAME_TIP   = (255, 245, 160, 255)
SMOKE       = (180, 170, 160, 110)
GLOW        = (255, 200,  80,  55)   # very faint warm aura


def make_helpers(img):
    def px(x, y, c):
        if 0 <= x < W and 0 <= y < H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for dy in range(h):
            for dx in range(w):
                px(x + dx, y + dy, c)

    return px, rect


def draw_sconce(rect, px):
    """Wall-mounted iron sconce + wooden handle. Common to both states."""
    # Wall mounting plate (small, behind bracket) x:13-18, y:14-22
    rect(13, 14, 6, 9, BRACKET_DK)
    rect(13, 14, 6, 1, BRACKET)
    rect(13, 22, 6, 1, RIVET)
    px(14, 15, BRACKET_HI)
    px(17, 15, BRACKET_HI)
    # rivets
    px(14, 21, RIVET)
    px(17, 21, RIVET)

    # Cup/cradle (holds the torch head) — bowl shape
    # outer rim
    rect(11, 13, 10, 1, BRACKET)
    rect(10, 14,  1, 2, BRACKET)
    rect(21, 14,  1, 2, BRACKET)
    rect(11, 16, 10, 1, BRACKET_DK)
    # cup interior (slight darker)
    rect(12, 14, 8, 2, BRACKET_DK)
    # rim highlight
    rect(11, 13, 10, 1, BRACKET_HI)

    # Wooden handle (sticks down from cup)
    rect(14, 17, 4, 6, HANDLE)
    rect(14, 17, 1, 6, HANDLE_DK)
    rect(17, 17, 1, 6, HANDLE_DK)
    rect(15, 17, 2, 1, HANDLE_HI)
    # handle wrapping rings
    rect(14, 19, 4, 1, HANDLE_DK)
    rect(14, 22, 4, 1, HANDLE_DK)


def make_unlit():
    img = Image.new("RGBA", (W, H), BG)
    px, rect = make_helpers(img)
    draw_sconce(rect, px)

    # Charred / cold torch head poking up out of cup
    rect(13, 9, 6, 4, CHAR)
    rect(13, 9, 6, 1, CHAR_HI)
    # uneven scorched tip
    px(12, 11, CHAR)
    px(19, 10, CHAR)
    px(13,  8, CHAR)
    px(17,  8, CHAR)
    # a couple highlight dots (oily char shine)
    px(15, 10, CHAR_HI)
    px(17, 11, CHAR_HI)

    return img


def make_lit_frame(flame_h, x_off, smoke_show):
    """One frame of the lit animation."""
    img = Image.new("RGBA", (W, H), BG)
    px, rect = make_helpers(img)

    # Soft glow halo around flame (drawn first so flame paints on top)
    halo_cx = 16
    halo_cy = 8
    for dy in range(-5, 6):
        for dx in range(-6, 7):
            d2 = dx * dx + dy * dy
            if 16 <= d2 <= 30:
                px(halo_cx + dx, halo_cy + dy, GLOW)

    draw_sconce(rect, px)

    # Wick base in cup (smouldering bright ember red)
    rect(13, 11, 6, 2, EMBER)
    rect(13, 11, 6, 1, FLAME_LO)

    # Flame column rising from cup top (y ~ 11) upwards
    base_y = 10
    base_x = 13
    for i in range(flame_h):
        y = base_y - i
        # width tapers from 6 -> 1
        w = max(1, 6 - i)
        x = base_x + (6 - w) // 2 + (x_off if i > flame_h // 2 else 0)
        if i < 2:
            c = FLAME_LO
        elif i < flame_h - 2:
            c = FLAME_MID
        else:
            c = FLAME_TIP
        rect(x, y, w, 1, c)

    # Side wisps near base
    px(12 + x_off, base_y - 1, FLAME_LO)
    px(12 + x_off, base_y - 2, EMBER)
    px(19 - x_off, base_y - 1, FLAME_LO)
    px(19 - x_off, base_y - 2, EMBER)

    # Smoke wisp
    if smoke_show:
        sx = 16 + x_off
        for i in range(3):
            px(sx + (i % 2), base_y - flame_h - 1 - i, SMOKE)

    return img


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    unlit = make_unlit()
    unlit.save(os.path.join(OUT_DIR, "unlit.png"))
    print(f"Saved {OUT_DIR}/unlit.png ({unlit.width}×{unlit.height})")

    # Four flame frames with subtle variation
    frames = [
        make_lit_frame(flame_h=6, x_off=0,  smoke_show=False),
        make_lit_frame(flame_h=7, x_off=1,  smoke_show=True),
        make_lit_frame(flame_h=6, x_off=-1, smoke_show=False),
        make_lit_frame(flame_h=8, x_off=0,  smoke_show=True),
    ]
    sheet = Image.new("RGBA", (W * COLS, H), BG)
    for col, frame in enumerate(frames):
        sheet.paste(frame, (col * W, 0))
    sheet.save(os.path.join(OUT_DIR, "lit_sheet.png"))
    print(f"Saved {OUT_DIR}/lit_sheet.png ({sheet.width}×{sheet.height})")


if __name__ == "__main__":
    main()
