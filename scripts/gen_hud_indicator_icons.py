"""
Generates 16x16 HUD indicator icons:
  assets/ui/hud_indicators/sun.png
  assets/ui/hud_indicators/moon.png
  assets/ui/hud_indicators/cave.png

These ride a "porthole" orbit on the time-of-day button. No animation — the
sprite gets repositioned by UI code each frame, so each PNG is a single frame.
"""

from PIL import Image
import os

SIZE = 16
OUT_DIR = "assets/ui/hud_indicators"

BG = (0, 0, 0, 0)


def new_img():
    return Image.new("RGBA", (SIZE, SIZE), BG)


def rect(img, x, y, w, h, c):
    for ry in range(h):
        for rx in range(w):
            px(img, x + rx, y + ry, c)


def px(img, x, y, c):
    if 0 <= x < SIZE and 0 <= y < SIZE:
        img.putpixel((x, y), c)


def fill_disc(img, cx, cy, radius, c):
    r2 = radius * radius
    for y in range(SIZE):
        for x in range(SIZE):
            dx = x - cx + 0.5
            dy = y - cy + 0.5
            if dx * dx + dy * dy <= r2:
                px(img, x, y, c)


def fill_ellipse(img, cx, cy, rx, ry, c):
    for y in range(SIZE):
        for x in range(SIZE):
            dx = (x - cx + 0.5) / rx
            dy = (y - cy + 0.5) / ry
            if dx * dx + dy * dy <= 1.0:
                px(img, x, y, c)


def fill_disc_shade(img, cx, cy, radius, base, hi, shadow):
    """Filled disc with a hi-light arc top-left and shadow arc bottom-right."""
    r2 = radius * radius
    for y in range(SIZE):
        for x in range(SIZE):
            dx = x - cx + 0.5
            dy = y - cy + 0.5
            d2 = dx * dx + dy * dy
            if d2 <= r2:
                # vector toward "light from upper-left" = (-1, -1) normalized
                # dot(normal, light) > threshold -> highlight
                norm_len = max(0.001, (dx * dx + dy * dy) ** 0.5)
                nx = dx / norm_len
                ny = dy / norm_len
                light_dot = -nx - ny  # light direction (-1,-1)
                if light_dot > 0.7:
                    px(img, x, y, hi)
                elif light_dot < -0.5:
                    px(img, x, y, shadow)
                else:
                    px(img, x, y, base)


# ── Sun ────────────────────────────────────────────────────────────────────────
def gen_sun():
    img = new_img()
    SUN_BASE = (250, 195, 60, 255)
    SUN_HI = (255, 235, 140, 255)
    SUN_SHADOW = (205, 130, 30, 255)
    RAY = (255, 215, 90, 255)
    RAY_HI = (255, 245, 170, 255)

    cx, cy = 8, 8
    # main disc radius ~4.5
    fill_disc_shade(img, cx, cy, 4.5, SUN_BASE, SUN_HI, SUN_SHADOW)
    # short rays — 2 px stubs at N, S, E, W and diagonal singles
    # cardinals
    rect(img, 7, 0, 2, 2, RAY)
    rect(img, 7, 14, 2, 2, RAY)
    rect(img, 0, 7, 2, 2, RAY)
    rect(img, 14, 7, 2, 2, RAY)
    # diagonal single highlights
    px(img, 2, 2, RAY_HI)
    px(img, 13, 2, RAY_HI)
    px(img, 2, 13, RAY)
    px(img, 13, 13, RAY)
    # core sparkle
    px(img, 6, 6, SUN_HI)
    return img


# ── Moon ───────────────────────────────────────────────────────────────────────
def gen_moon():
    img = new_img()
    MOON_BASE = (210, 215, 235, 255)
    MOON_HI = (245, 248, 255, 255)
    MOON_SHADOW = (140, 150, 180, 255)
    CRATER = (155, 165, 195, 255)

    cx, cy = 8, 8
    # full disc body so it reads at distance
    fill_disc_shade(img, cx, cy, 6.5, MOON_BASE, MOON_HI, MOON_SHADOW)
    # carve a crescent shadow by overlapping a darker disc offset to the right
    SHADOW_FILL = (60, 70, 100, 0)  # transparent — carve away
    r2 = 6.0 * 6.0
    for y in range(SIZE):
        for x in range(SIZE):
            dx = x - (cx + 3) + 0.5
            dy = y - cy + 0.5
            if dx * dx + dy * dy <= r2:
                px(img, x, y, BG)
    # tiny crater dots on the visible crescent
    px(img, 5, 5, CRATER)
    px(img, 4, 9, CRATER)
    px(img, 6, 11, CRATER)
    return img


# ── Cave ───────────────────────────────────────────────────────────────────────
def gen_cave():
    img = new_img()
    ROCK = (110, 100, 95, 255)
    ROCK_HI = (150, 140, 130, 255)
    ROCK_SHADOW = (75, 65, 60, 255)
    OPENING = (15, 12, 18, 255)
    OPENING_HINT = (40, 30, 45, 255)
    GROUND = (95, 75, 55, 255)

    # Frame border of stone — slightly arched at top
    # outer rock silhouette: fill 2..15 wide, 1..15 tall
    rect(img, 1, 14, 14, 2, GROUND)  # ground line
    # stone arch — left pillar
    rect(img, 1, 4, 3, 11, ROCK)
    rect(img, 1, 4, 1, 11, ROCK_SHADOW)
    rect(img, 3, 4, 1, 4, ROCK_HI)
    # right pillar
    rect(img, 12, 4, 3, 11, ROCK)
    rect(img, 14, 4, 1, 11, ROCK_SHADOW)
    rect(img, 12, 4, 1, 4, ROCK_HI)
    # top arch (curved)
    rect(img, 4, 1, 8, 2, ROCK)
    rect(img, 4, 1, 8, 1, ROCK_HI)
    rect(img, 3, 2, 1, 2, ROCK)
    rect(img, 12, 2, 1, 2, ROCK)
    rect(img, 5, 3, 6, 1, ROCK)
    # dark opening
    rect(img, 4, 4, 8, 11, OPENING)
    # subtle hint of interior shade at bottom of opening
    rect(img, 5, 13, 6, 1, OPENING_HINT)
    # speckle on rock to break flatness
    px(img, 2, 7, ROCK_HI)
    px(img, 13, 9, ROCK_HI)
    px(img, 2, 11, ROCK_SHADOW)
    px(img, 13, 5, ROCK_SHADOW)
    return img


# ── Sneak (footprint) ───────────────────────────────────────────────────────────
def gen_sneak():
    """A barefoot footprint — the mode toggle for sneaking. Drawn asymmetric (a
    big toe and an arch notch) so it reads as a foot, not a blob."""
    img = new_img()
    SOLE = (122, 134, 156, 255)
    SOLE_HI = (170, 182, 202, 255)
    SOLE_SHADOW = (80, 90, 110, 255)

    # Sole: a stack of ellipses tapering from a wide ball to a narrow heel.
    fill_ellipse(img, 8, 6, 3.4, 2.6, SOLE)   # ball (wide)
    fill_ellipse(img, 8, 9, 2.7, 2.2, SOLE)   # arch
    fill_ellipse(img, 8, 12, 2.1, 2.0, SOLE)  # heel (narrow)
    # Carve the instep on the left so the outline curves like a real arch.
    px(img, 5, 9, BG)
    px(img, 5, 10, BG)
    px(img, 6, 10, BG)
    # Toes: a fat big toe on the left, three smaller ones arcing right.
    fill_disc(img, 5, 3, 1.1, SOLE)
    px(img, 7, 2, SOLE)
    px(img, 9, 2, SOLE)
    px(img, 11, 3, SOLE)
    # Depth: highlight along the inner edge, shadow on the outer/heel.
    px(img, 7, 5, SOLE_HI)
    px(img, 7, 8, SOLE_HI)
    px(img, 9, 7, SOLE_SHADOW)
    px(img, 9, 12, SOLE_SHADOW)
    return img


# ── Aware (eye) ──────────────────────────────────────────────────────────────────
def gen_aware():
    """An open eye — the mode toggle for heightened awareness."""
    img = new_img()
    OUTLINE = (58, 54, 70, 255)
    WHITE = (236, 240, 246, 255)
    IRIS = (74, 152, 212, 255)
    IRIS_HI = (140, 202, 240, 255)
    PUPIL = (22, 26, 36, 255)

    # Dark almond rim, then the white sclera inside it.
    fill_ellipse(img, 8, 8, 7.0, 4.0, OUTLINE)
    fill_ellipse(img, 8, 8, 6.1, 3.2, WHITE)
    # Iris with an upper-left highlight, then the pupil and a catchlight.
    fill_disc(img, 8, 8, 3.0, IRIS)
    fill_disc(img, 7, 7, 1.1, IRIS_HI)
    fill_disc(img, 8, 8, 1.4, PUPIL)
    px(img, 7, 7, WHITE)
    return img


# ── Retaliate (crossed swords) ──────────────────────────────────────────────────
def gen_retaliate():
    """Two crossed swords — the mode toggle for auto-retaliate. Grips at the
    bottom corners, blades crossing mid-icon, tips toward the top corners."""
    img = new_img()
    STEEL = (188, 198, 214, 255)
    STEEL_HI = (232, 238, 248, 255)
    GUARD = (198, 154, 62, 255)
    GRIP = (112, 76, 48, 255)

    # Blade A: bottom-left grip toward top-right tip; 2 px wide.
    for i in range(9):
        x, y = 4 + i, 12 - i
        px(img, x, y, STEEL)
        px(img, x + 1, y, STEEL)
    # Blade B: bottom-right grip toward top-left tip.
    for i in range(9):
        x, y = 11 - i, 12 - i
        px(img, x, y, STEEL)
        px(img, x - 1, y, STEEL)
    # Bright tips + an edge highlight along each blade's upper side.
    px(img, 13, 4, STEEL_HI)
    px(img, 12, 4, STEEL_HI)
    px(img, 2, 4, STEEL_HI)
    px(img, 3, 4, STEEL_HI)
    px(img, 10, 6, STEEL_HI)
    px(img, 5, 6, STEEL_HI)
    # Crossguards: short bars perpendicular to each blade, just above the grip.
    px(img, 3, 11, GUARD)
    px(img, 5, 13, GUARD)
    px(img, 12, 11, GUARD)
    px(img, 10, 13, GUARD)
    # Grips trailing off the bottom corners, ending in a pommel pixel.
    px(img, 3, 13, GRIP)
    px(img, 2, 14, GRIP)
    px(img, 12, 13, GRIP)
    px(img, 13, 14, GRIP)
    return img


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    gen_sun().save(os.path.join(OUT_DIR, "sun.png"))
    gen_moon().save(os.path.join(OUT_DIR, "moon.png"))
    gen_cave().save(os.path.join(OUT_DIR, "cave.png"))
    gen_sneak().save(os.path.join(OUT_DIR, "sneak.png"))
    gen_aware().save(os.path.join(OUT_DIR, "aware.png"))
    gen_retaliate().save(os.path.join(OUT_DIR, "retaliate.png"))
    print(f"wrote {OUT_DIR}/sun.png, moon.png, cave.png, sneak.png, aware.png, retaliate.png")


if __name__ == "__main__":
    main()
