"""
Generates static 32x32 PNG sprites for the usable-item demo pickups:

  wand_of_sparks       — slender rod with a glowing yellow-amber crystal tip
                          crackling with stored lightning
  rune_of_lesser_heal  — clay shard etched with a crimson healing glyph

Single `sprite.png` per object in its overworld_objects directory.
Style matches gen_gathering_sprites.py — chunky pixels, 2–3 shading levels,
no anti-aliasing, transparent background, small ground shadow.
"""

from __future__ import annotations

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


def make_wand_of_sparks() -> Image.Image:
    """Diagonal wand from lower-left to upper-right, crystal tip in the
    upper-right glowing yellow-amber with electric arcs."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 27, 7)

    wood_dark = (70, 45, 25, 255)
    wood = (130, 90, 55, 255)
    wood_hi = (190, 145, 95, 255)
    grip_dark = (50, 30, 15, 255)
    grip = (95, 60, 35, 255)
    band = (200, 175, 100, 255)
    band_dark = (140, 110, 50, 255)

    crystal_dark = (160, 110, 20, 255)
    crystal = (235, 185, 55, 255)
    crystal_hi = (255, 235, 140, 255)
    glow = (255, 240, 180, 200)

    spark = (255, 255, 220, 255)
    spark_dim = (200, 215, 255, 220)

    # Shaft: diagonal stroke from (7, 24) up-right to (21, 10)
    points = [
        (7, 24), (8, 23), (9, 22), (10, 21), (11, 20), (12, 19), (13, 18),
        (14, 17), (15, 16), (16, 15), (17, 14), (18, 13), (19, 12), (20, 11),
        (21, 10),
    ]
    for (x, y) in points:
        px(x, y, wood)
        px(x, y - 1, wood_hi)
        px(x + 1, y, wood_dark)
    # Grip wrap near the butt (lower-left, leather-darkened)
    for (x, y) in points[:5]:
        px(x, y, grip)
        px(x, y - 1, grip)
        px(x + 1, y, grip_dark)
    # Pommel cap
    px(6, 25, grip_dark)
    px(7, 25, grip_dark)

    # Ornamental band where grip meets the polished section
    px(11, 20, band)
    px(12, 19, band)
    px(11, 19, band_dark)
    px(12, 20, band_dark)

    # Crystal tip — small chunky gem at the upper end
    # Centered around (22, 8), roughly diamond-shaped
    rect(21, 7, 4, 4, crystal)
    # Top facet (highlight)
    px(22, 6, crystal)
    px(23, 6, crystal_hi)
    px(21, 7, crystal_hi)
    px(22, 7, crystal_hi)
    # Right facet (mid)
    px(24, 8, crystal_dark)
    px(24, 9, crystal_dark)
    # Bottom shadow
    px(22, 10, crystal_dark)
    px(23, 10, crystal_dark)
    px(21, 10, crystal_dark)
    # Bright core
    px(22, 8, crystal_hi)
    px(23, 7, crystal_hi)

    # Soft glow halo
    for (x, y) in [(20, 7), (20, 8), (25, 7), (25, 8), (22, 5), (23, 5),
                   (20, 9), (25, 9)]:
        px(x, y, glow)

    # Electric arcs spitting off the tip
    arcs = [
        (24, 4), (25, 5), (26, 4), (27, 3),
        (19, 5), (18, 4), (17, 5), (16, 3),
        (26, 8), (27, 9), (28, 7),
        (25, 11), (26, 12),
    ]
    for (x, y) in arcs:
        px(x, y, spark)
    # Fainter secondary sparks
    for (x, y) in [(28, 4), (15, 6), (29, 8), (24, 12), (27, 11)]:
        px(x, y, spark_dim)

    return img


def make_rune_of_lesser_heal() -> Image.Image:
    """A flat clay shard with a chipped edge, etched with a glowing crimson
    cross-shaped healing glyph at the center."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 25, 7)

    clay_dark = (110, 70, 50, 255)
    clay = (175, 125, 90, 255)
    clay_hi = (220, 175, 130, 255)
    crack = (90, 55, 35, 255)

    glyph_dark = (130, 25, 25, 255)
    glyph = (210, 55, 60, 255)
    glyph_hi = (255, 130, 130, 255)
    glow = (255, 200, 200, 200)

    # Shard silhouette — irregular hexagon, chipped lower-right corner
    rect(9, 11, 14, 11, clay)
    # Bevel the top corners
    px(9, 11, BG)
    px(22, 11, BG)
    px(8, 12, BG)
    px(23, 12, BG)
    px(9, 12, clay)
    px(22, 12, clay)
    # Bevel the bottom corners
    px(9, 21, clay_dark)
    px(22, 21, clay_dark)
    px(8, 20, BG)
    px(23, 20, BG)
    # Chipped lower-right
    px(22, 22, BG)
    px(21, 22, BG)
    px(20, 22, clay_dark)
    px(22, 21, BG)
    # Extended edges (slight bulges)
    px(8, 14, clay)
    px(8, 15, clay)
    px(8, 16, clay)
    px(8, 17, clay)
    px(23, 14, clay)
    px(23, 15, clay)
    px(23, 16, clay)
    px(23, 17, clay)
    px(23, 18, clay_dark)

    # Top-light highlights across the upper face
    rect(10, 11, 12, 1, clay_hi)
    rect(11, 12, 10, 1, clay_hi)
    px(9, 13, clay_hi)
    px(22, 13, clay_hi)
    # Bottom shadow
    rect(10, 21, 12, 1, clay_dark)
    rect(11, 20, 10, 1, clay_dark)

    # Surface cracks / texture flecks
    for (x, y) in [(11, 15), (12, 16), (19, 13), (20, 14), (13, 19), (18, 19)]:
        px(x, y, crack)

    # Healing glyph — a plus/cross centered on (16, 16)
    # Vertical bar
    rect(16, 13, 1, 7, glyph)
    px(16, 13, glyph_hi)
    px(16, 14, glyph_hi)
    px(16, 19, glyph_dark)
    # Horizontal bar
    rect(13, 16, 7, 1, glyph)
    px(13, 16, glyph_dark)
    px(19, 16, glyph_dark)
    px(14, 16, glyph_hi)
    px(15, 16, glyph_hi)
    # Center hot spot
    px(16, 16, glyph_hi)
    # Thicken the cross slightly so it reads at 32x32
    px(15, 14, glyph_dark)
    px(17, 14, glyph_dark)
    px(15, 18, glyph_dark)
    px(17, 18, glyph_dark)

    # Soft red glow halo around the glyph
    for (x, y) in [(14, 13), (18, 13), (12, 16), (20, 16),
                   (14, 19), (18, 19), (16, 12), (16, 20)]:
        px(x, y, glow)

    return img


def main() -> None:
    out_root = os.path.join("assets", "overworld_objects")
    targets = [
        ("wand_of_sparks",      make_wand_of_sparks),
        ("rune_of_lesser_heal", make_rune_of_lesser_heal),
    ]
    for type_id, maker in targets:
        out_dir = os.path.join(out_root, type_id)
        os.makedirs(out_dir, exist_ok=True)
        img = maker()
        path = os.path.join(out_dir, "sprite.png")
        img.save(path)
        print(f"Saved {path}")


if __name__ == "__main__":
    main()
