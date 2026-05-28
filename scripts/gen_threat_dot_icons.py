"""
Generates 16x16 threat-dot icons used in the Nearby NPCs panel:
  assets/ui/hud_indicators/dot_red.png      (NPC currently targeting the player)
  assets/ui/hud_indicators/dot_yellow.png   (hostile but not engaged)
  assets/ui/hud_indicators/dot_green.png    (passive)

Single-frame static sprites. Each orb is shaded as a glossy sphere lit from
the upper-left: bright specular cap, mid-tone body, deep shadow on the
bottom-right rim, plus a 1 px dark outline for separation against the panel
background.
"""

from PIL import Image
import os

SIZE = 16
OUT_DIR = "assets/ui/hud_indicators"
BG = (0, 0, 0, 0)


def new_img():
    return Image.new("RGBA", (SIZE, SIZE), BG)


def px(img, x, y, c):
    if 0 <= x < SIZE and 0 <= y < SIZE:
        img.putpixel((x, y), c)


def fill_orb(img, cx, cy, radius, outline, shadow, base, hi, spec):
    """Filled disc with five shading zones for a 3D ball look.

    Zones (by signed-distance bands from the rim and from the light vector):
      - outline: outermost 1 px ring
      - shadow:  back-lit arc on the bottom-right
      - base:    main body
      - hi:      front-lit cap on the upper-left
      - spec:    bright specular dot inside the highlight cap
    Light vector (-1, -1) — same convention as gen_hud_indicator_icons.py.
    """
    r = radius
    r2 = r * r
    for y in range(SIZE):
        for x in range(SIZE):
            dx = x - cx + 0.5
            dy = y - cy + 0.5
            d2 = dx * dx + dy * dy
            if d2 > r2:
                continue
            d = d2 ** 0.5
            # Rim band: 1 px ring at the edge → dark outline.
            if d > r - 1.0:
                px(img, x, y, outline)
                continue
            # Light-direction dot product (light from upper-left).
            norm_len = max(0.001, d)
            nx = dx / norm_len
            ny = dy / norm_len
            light_dot = -nx - ny
            # Distance from the highlight center (offset toward upper-left).
            hx = dx + 0.45 * r
            hy = dy + 0.45 * r
            h_dist = (hx * hx + hy * hy) ** 0.5
            if h_dist < 0.9:
                px(img, x, y, spec)
            elif light_dot > 0.55:
                px(img, x, y, hi)
            elif light_dot < -0.55:
                px(img, x, y, shadow)
            else:
                px(img, x, y, base)


# ── Color sets ──────────────────────────────────────────────────────────────
# Each tuple is (outline, shadow, base, hi, spec).
RED = (
    (90, 14, 14, 255),     # outline — deep maroon
    (155, 35, 35, 255),    # shadow
    (220, 60, 60, 255),    # base
    (245, 130, 120, 255),  # hi
    (255, 220, 210, 255),  # spec — near-white cap
)

YELLOW = (
    (115, 80, 18, 255),
    (185, 135, 25, 255),
    (235, 190, 50, 255),
    (250, 225, 130, 255),
    (255, 250, 215, 255),
)

GREEN = (
    (28, 80, 28, 255),
    (55, 140, 55, 255),
    (90, 200, 90, 255),
    (170, 235, 160, 255),
    (225, 255, 215, 255),
)


def gen_dot(colors):
    img = new_img()
    cx, cy = 8, 8
    fill_orb(img, cx, cy, radius=7.0, outline=colors[0], shadow=colors[1],
             base=colors[2], hi=colors[3], spec=colors[4])
    return img


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    gen_dot(RED).save(os.path.join(OUT_DIR, "dot_red.png"))
    gen_dot(YELLOW).save(os.path.join(OUT_DIR, "dot_yellow.png"))
    gen_dot(GREEN).save(os.path.join(OUT_DIR, "dot_green.png"))
    print(f"wrote {OUT_DIR}/dot_red.png, dot_yellow.png, dot_green.png")


if __name__ == "__main__":
    main()
