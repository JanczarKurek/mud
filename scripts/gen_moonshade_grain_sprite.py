"""
Generates assets/modules/haunted_mill/overworld_objects/moonshade_grain/sprite.png
A single static item sprite: a burlap sack tied at the neck, spilling pale grain
that glows faint silver-white (moonlight on water). 32×32, transparent bg.
Items use a single sprite_path (no idle/walk sheet), like apple/bronze_sword.
"""

from PIL import Image
import os

W = H = 32
OUT_PATH = "assets/modules/haunted_mill/overworld_objects/moonshade_grain/sprite.png"

BG        = (0, 0, 0, 0)
BURLAP    = (168, 140,  96, 255)
BURLAP_DK = (120,  98,  64, 255)
BURLAP_HI = (198, 172, 126, 255)
WEAVE     = (146, 120,  80, 255)
TIE       = (96,  76,  46, 255)
GRAIN     = (226, 232, 214, 255)   # pale moonshade grain
GRAIN_HI  = (245, 248, 240, 255)
GLOW      = (206, 224, 214, 70)    # soft luminous halo

img = Image.new("RGBA", (W, H), BG)


def px(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        # alpha-aware: let the glow sit under solid pixels
        base = img.getpixel((x, y))
        if c[3] == 255 or base[3] == 0:
            img.putpixel((x, y), c)


def rect(x, y, w, h, c):
    for ry in range(h):
        for rx in range(w):
            px(x + rx, y + ry, c)


# ── Luminous halo around the open mouth of the sack ─────────────────────────────
for ry in range(-3, 6):
    for rx in range(-4, 5):
        if rx * rx + ry * ry <= 16:
            img.putpixel((max(0, min(W - 1, 16 + rx)), max(0, min(H - 1, 9 + ry))), GLOW)

# ── Sack body (rounded, wider at the base) ──────────────────────────────────────
rect(8, 14, 16, 15, BURLAP)
rect(7, 17, 1, 10, BURLAP)            # left bulge
rect(24, 17, 1, 10, BURLAP)          # right bulge
rect(8, 14, 1, 15, BURLAP_DK)        # left shadow
rect(23, 14, 1, 15, BURLAP_DK)       # right shadow
rect(9, 27, 14, 2, BURLAP_DK)        # base shadow
rect(10, 14, 4, 1, BURLAP_HI)        # top-left sheen
# burlap weave hint
for wy in range(16, 27, 3):
    for wx in range(9, 24, 3):
        px(wx, wy, WEAVE)

# ── Neck / tie ──────────────────────────────────────────────────────────────────
rect(11, 11, 10, 3, BURLAP)
rect(10, 12, 12, 1, TIE)             # drawstring
px(9, 12, TIE)
px(22, 12, TIE)

# ── Grain spilling from the open mouth ──────────────────────────────────────────
rect(12, 7, 8, 4, GRAIN)
rect(13, 6, 6, 1, GRAIN)
rect(14, 5, 4, 1, GRAIN_HI)          # brightest grains at the top
px(13, 8, GRAIN_HI)
px(18, 8, GRAIN_HI)
px(15, 6, GRAIN_HI)
# a few stray glowing kernels
px(11, 9, GRAIN_HI)
px(21, 9, GRAIN_HI)
px(20, 11, GRAIN)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({img.width}×{img.height})")
