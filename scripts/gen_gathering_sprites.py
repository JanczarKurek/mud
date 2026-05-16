"""
Generates static 32x32 PNG sprites for the gathering content set:

Tools (held items, small footprint):
  fishing_rod, pickaxe, herb_knife

Gathered drops (small inventory pickups):
  raw_fish, green_herb, iron_ore

World fixtures (resource nodes, fill more of the tile):
  fishing_spot, herb_patch, ore_node

Each object gets a single `sprite.png` in its overworld_objects directory.
The style matches the existing static items in the project — chunky pixels,
2-3 shading levels, no anti-aliasing, transparent background.
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


# ---------- tools ---------- #


def make_fishing_rod() -> Image.Image:
    """Diagonal wooden rod from lower-left to upper-right, with a thin line
    dangling toward the bottom-right corner."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 27, 7)

    wood_dark = (96, 58, 28, 255)
    wood = (150, 100, 60, 255)
    wood_hi = (200, 150, 100, 255)
    grip = (60, 35, 20, 255)
    line = (235, 235, 215, 255)
    hook = (160, 160, 170, 255)

    # Rod shaft from (6, 24) up-right to (24, 6) — 1 px wide stroke with a
    # 1 px highlight running alongside on the upper side.
    points = [
        (6, 24), (7, 23), (8, 22), (9, 21), (10, 20), (11, 19), (12, 18),
        (13, 17), (14, 16), (15, 15), (16, 14), (17, 13), (18, 12), (19, 11),
        (20, 10), (21, 9), (22, 8), (23, 7), (24, 6),
    ]
    for (x, y) in points:
        px(x, y, wood)
        px(x, y - 1, wood_hi)
        px(x + 1, y, wood_dark)
    # Grip wrap near the butt (lower-left)
    for (x, y) in points[:5]:
        px(x, y, grip)
        px(x, y - 1, grip)

    # Tip cap
    px(24, 6, wood_hi)
    px(25, 6, wood_hi)

    # Fishing line dropping from the tip toward the bottom-right
    line_path = [(25, 7), (26, 8), (26, 10), (26, 12), (26, 15), (26, 18), (26, 21), (26, 24)]
    for (x, y) in line_path:
        px(x, y, line)
    # Hook at the end
    px(26, 25, hook)
    px(27, 25, hook)
    px(27, 26, hook)
    return img


def make_pickaxe() -> Image.Image:
    """Vertical wooden haft topped with a horizontal iron head."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 27, 7)

    iron_dark = (70, 70, 80, 255)
    iron = (130, 130, 140, 255)
    iron_hi = (190, 190, 200, 255)
    wood_dark = (96, 58, 28, 255)
    wood = (150, 100, 60, 255)
    wood_hi = (200, 150, 100, 255)

    # Wooden haft (vertical, slightly off-center)
    rect(15, 9, 2, 17, wood)
    rect(15, 9, 1, 17, wood_hi)
    rect(16, 9, 1, 17, wood_dark)
    # Haft cap at bottom
    rect(14, 25, 4, 2, wood_dark)
    px(15, 26, wood)
    px(16, 26, wood)

    # Iron head — wide curved bar
    # Center collar
    rect(13, 7, 6, 3, iron)
    rect(13, 7, 6, 1, iron_hi)
    rect(13, 9, 6, 1, iron_dark)
    # Left point
    rect(8, 8, 5, 2, iron)
    px(8, 8, iron_dark)
    px(7, 9, iron)
    px(8, 9, iron_dark)
    rect(8, 8, 5, 1, iron_hi)
    # Right point
    rect(19, 8, 5, 2, iron)
    px(23, 8, iron_dark)
    px(24, 9, iron)
    px(23, 9, iron_dark)
    rect(19, 8, 5, 1, iron_hi)
    # Pointy tips
    px(6, 9, iron_dark)
    px(25, 9, iron_dark)

    return img


def make_herb_knife() -> Image.Image:
    """Small curved blade with a leather-wrapped handle."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 25, 6)

    blade_dark = (130, 130, 145, 255)
    blade = (200, 205, 215, 255)
    blade_hi = (240, 245, 250, 255)
    grip_dark = (80, 50, 30, 255)
    grip = (140, 90, 55, 255)
    grip_hi = (190, 140, 95, 255)
    pommel = (60, 60, 65, 255)

    # Handle — diagonal grip lower-left
    handle = [(10, 22), (11, 21), (12, 20), (13, 19), (14, 18)]
    for (x, y) in handle:
        px(x, y, grip)
        px(x - 1, y, grip_dark)
        px(x + 1, y, grip_hi)
    # Pommel
    px(9, 23, pommel)
    px(10, 23, pommel)
    # Bolster between handle and blade
    px(14, 17, pommel)
    px(15, 17, pommel)

    # Curved blade arcing up and to the right
    blade_pts = [
        (15, 16), (16, 15), (17, 14), (18, 13), (19, 13), (20, 12), (21, 12),
        (22, 12), (23, 13), (23, 14),
    ]
    for (x, y) in blade_pts:
        px(x, y, blade)
        px(x, y - 1, blade_hi)
        px(x, y + 1, blade_dark)
    # Sharpen the tip
    px(23, 12, blade_hi)
    px(23, 13, blade_hi)
    return img


# ---------- drops ---------- #


def make_raw_fish() -> Image.Image:
    """A small fish lying on its side, head left, tail right."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 24, 8)

    body_dark = (50, 90, 120, 255)
    body = (110, 160, 195, 255)
    body_hi = (180, 215, 235, 255)
    belly = (220, 230, 230, 255)
    fin = (75, 115, 145, 255)
    eye = (20, 20, 25, 255)
    eye_hi = (240, 240, 240, 255)

    # Body silhouette
    rect(9, 17, 14, 5, body)
    rect(10, 16, 12, 1, body)
    rect(10, 22, 12, 1, body)
    # Highlight along the top
    rect(10, 17, 12, 1, body_hi)
    # Shadow along the bottom
    rect(10, 21, 12, 1, body_dark)
    # Belly
    rect(11, 22, 10, 1, belly)

    # Head taper (left)
    px(9, 17, body_dark)
    px(9, 21, body_dark)
    px(8, 19, body)
    px(8, 20, body)

    # Tail fin (right)
    px(23, 17, fin)
    px(24, 16, fin)
    px(24, 17, fin)
    px(23, 22, fin)
    px(24, 22, fin)
    px(24, 23, fin)
    px(23, 19, fin)
    px(23, 20, fin)

    # Dorsal fin
    rect(13, 15, 6, 1, fin)
    px(14, 14, fin)
    px(16, 14, fin)

    # Eye near the head
    px(11, 19, eye)
    px(11, 18, eye_hi)
    return img


def make_green_herb() -> Image.Image:
    """A short sprig: central stem with paired leaves."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 26, 5)

    stem_dark = (40, 90, 40, 255)
    stem = (60, 130, 60, 255)
    leaf_dark = (50, 110, 50, 255)
    leaf = (90, 175, 80, 255)
    leaf_hi = (140, 220, 130, 255)

    # Central stem
    rect(16, 12, 1, 13, stem)
    rect(15, 12, 1, 13, stem_dark)
    # Stem base / soil splash
    px(15, 25, stem_dark)
    px(16, 25, stem)
    px(17, 25, stem_dark)

    # Lower-left leaf
    leaf_l1 = [(14, 22), (13, 21), (12, 21), (11, 20), (12, 22), (13, 22)]
    for (x, y) in leaf_l1:
        px(x, y, leaf)
    px(11, 20, leaf_dark)
    px(13, 21, leaf_hi)
    # Lower-right leaf
    leaf_r1 = [(18, 22), (19, 21), (20, 21), (21, 20), (19, 22), (20, 22)]
    for (x, y) in leaf_r1:
        px(x, y, leaf)
    px(21, 20, leaf_dark)
    px(19, 21, leaf_hi)

    # Mid leaves
    leaf_l2 = [(14, 17), (13, 16), (12, 16), (13, 17)]
    for (x, y) in leaf_l2:
        px(x, y, leaf)
    px(12, 16, leaf_dark)
    px(13, 17, leaf_hi)
    leaf_r2 = [(18, 17), (19, 16), (20, 16), (19, 17)]
    for (x, y) in leaf_r2:
        px(x, y, leaf)
    px(20, 16, leaf_dark)
    px(19, 17, leaf_hi)

    # Top leaves
    leaf_top = [(14, 13), (15, 12), (17, 12), (18, 13), (15, 13), (17, 13)]
    for (x, y) in leaf_top:
        px(x, y, leaf)
    px(14, 13, leaf_dark)
    px(18, 13, leaf_dark)
    px(16, 11, leaf_hi)
    px(16, 12, leaf_hi)
    return img


def make_iron_ore() -> Image.Image:
    """A chunky rock with rust-colored vein streaks."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 25, 7)

    rock_dark = (60, 55, 55, 255)
    rock = (115, 105, 100, 255)
    rock_hi = (170, 160, 155, 255)
    rust_dark = (110, 55, 30, 255)
    rust = (170, 90, 50, 255)
    rust_hi = (220, 140, 90, 255)

    # Main blob — irregular pentagon-ish silhouette
    rect(11, 16, 10, 8, rock)
    px(10, 18, rock); px(10, 19, rock); px(10, 20, rock)
    px(21, 18, rock); px(21, 19, rock); px(21, 20, rock)
    px(12, 15, rock); px(13, 15, rock); px(15, 15, rock); px(17, 15, rock); px(18, 15, rock)
    px(12, 24, rock); px(13, 24, rock); px(15, 24, rock); px(17, 24, rock); px(18, 24, rock)

    # Top highlights
    rect(12, 16, 8, 1, rock_hi)
    px(11, 17, rock_hi)
    px(10, 18, rock_hi)
    # Bottom shadow
    rect(12, 23, 8, 1, rock_dark)
    px(20, 22, rock_dark)
    px(21, 20, rock_dark)
    px(10, 20, rock_dark)

    # Rust veins
    px(13, 18, rust_dark)
    px(14, 18, rust)
    px(15, 17, rust_hi)
    px(15, 18, rust)
    px(16, 19, rust)
    px(17, 19, rust_dark)

    px(18, 21, rust)
    px(19, 21, rust_dark)
    px(17, 22, rust_hi)

    px(12, 21, rust_dark)
    px(13, 22, rust)
    return img


# ---------- world fixtures ---------- #


def make_fishing_spot() -> Image.Image:
    """Concentric ripple rings on water — wide footprint."""
    img = new_img()
    px, rect = helpers(img)

    water_dark = (40, 90, 130, 200)
    water = (80, 140, 190, 230)
    water_hi = (170, 215, 240, 255)
    crest = (235, 245, 255, 255)

    cx, cy = 16, 18

    # Outer ring (broken arc)
    outer = [
        (cx - 10, cy), (cx + 10, cy),
        (cx - 9, cy - 4), (cx + 9, cy - 4),
        (cx - 9, cy + 4), (cx + 9, cy + 4),
        (cx - 6, cy - 7), (cx + 6, cy - 7),
        (cx - 6, cy + 7), (cx + 6, cy + 7),
        (cx - 3, cy - 8), (cx + 3, cy - 8),
        (cx - 3, cy + 8), (cx + 3, cy + 8),
    ]
    for (x, y) in outer:
        px(x, y, water_dark)
    # Soft fill between outer arcs
    for (x, y) in [(cx - 8, cy - 2), (cx + 8, cy - 2), (cx - 8, cy + 2), (cx + 8, cy + 2),
                   (cx - 7, cy - 5), (cx + 7, cy - 5), (cx - 7, cy + 5), (cx + 7, cy + 5)]:
        px(x, y, water)

    # Middle ring
    middle = [
        (cx - 6, cy), (cx + 6, cy),
        (cx - 5, cy - 3), (cx + 5, cy - 3),
        (cx - 5, cy + 3), (cx + 5, cy + 3),
        (cx - 3, cy - 5), (cx + 3, cy - 5),
        (cx - 3, cy + 5), (cx + 3, cy + 5),
        (cx, cy - 6), (cx, cy + 6),
    ]
    for (x, y) in middle:
        px(x, y, water)
        px(x, y - 1 if y < cy else y + 1, water_hi)

    # Inner crest — bright dot cluster
    rect(cx - 1, cy - 1, 3, 3, water_hi)
    px(cx, cy, crest)
    px(cx - 1, cy, crest)
    px(cx + 1, cy, crest)
    px(cx, cy - 1, crest)
    px(cx, cy + 1, crest)
    return img


def make_herb_patch() -> Image.Image:
    """A lush leafy bush filling most of the tile."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 27, 10)

    bush_dark = (30, 75, 35, 255)
    bush = (55, 130, 55, 255)
    bush_mid = (80, 165, 70, 255)
    bush_hi = (130, 210, 110, 255)
    stem = (90, 60, 35, 255)
    flower = (220, 230, 140, 255)

    # Base shadow blob
    rect(6, 22, 21, 4, bush_dark)
    # Mid-body
    rect(5, 18, 22, 6, bush)
    rect(8, 14, 17, 5, bush)
    rect(11, 11, 11, 4, bush)

    # Highlights — top-left favored
    for (x, y, c) in [
        (8, 14, bush_mid), (9, 13, bush_mid), (10, 13, bush_mid),
        (12, 11, bush_mid), (13, 11, bush_hi), (14, 11, bush_hi),
        (15, 10, bush_hi), (16, 10, bush_hi), (17, 11, bush_mid),
        (6, 19, bush_mid), (7, 18, bush_mid),
        (20, 15, bush_mid), (21, 15, bush_mid),
        (18, 12, bush_hi), (19, 13, bush_mid),
    ]:
        px(x, y, c)

    # Leaf detail flecks
    for (x, y) in [(10, 17), (13, 17), (17, 17), (20, 18), (23, 19),
                   (6, 22), (10, 22), (15, 23), (20, 22), (24, 22),
                   (12, 14), (16, 13), (19, 16), (8, 16)]:
        px(x, y, bush_mid)

    # Darker shadow flecks at base
    for (x, y) in [(7, 25), (12, 25), (18, 25), (23, 25),
                   (9, 24), (16, 24), (21, 24)]:
        px(x, y, bush_dark)

    # Three small herb sprigs poking up — pale flower-like tips
    for cx in (10, 16, 22):
        px(cx, 9, stem)
        px(cx, 8, stem)
        px(cx - 1, 8, flower)
        px(cx + 1, 8, flower)
        px(cx, 7, flower)
    return img


def make_ore_node() -> Image.Image:
    """A craggy grey outcrop with rust-colored vein streaks. Bulky tile-filling
    silhouette that reads as 'mineable rock'."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 28, 11)

    rock_dark = (60, 55, 55, 255)
    rock = (115, 105, 100, 255)
    rock_mid = (145, 135, 130, 255)
    rock_hi = (185, 175, 170, 255)
    rust_dark = (110, 55, 30, 255)
    rust = (170, 90, 50, 255)
    rust_hi = (220, 140, 90, 255)

    # Wide irregular base
    rect(4, 22, 25, 5, rock)
    rect(6, 19, 21, 3, rock)
    rect(9, 15, 15, 4, rock)
    rect(12, 11, 9, 4, rock)
    rect(14, 8, 5, 3, rock)

    # Silhouette nibbles to break the rectangles
    for (x, y) in [(3, 24), (3, 25), (28, 24), (28, 25), (29, 25),
                   (5, 21), (27, 21), (8, 17), (24, 17), (11, 13), (21, 13),
                   (13, 9), (19, 9)]:
        px(x, y, rock)
    # Outline shading
    for (x, y) in [(3, 26), (29, 26), (4, 26), (28, 26),
                   (5, 23), (27, 23), (8, 20), (26, 20),
                   (11, 16), (23, 16), (13, 12), (20, 12)]:
        px(x, y, rock_dark)

    # Top-light highlights
    for (x, y) in [(14, 8), (15, 8), (16, 8),
                   (13, 9), (14, 9), (17, 9),
                   (12, 12), (13, 11), (16, 11),
                   (9, 16), (10, 15), (11, 15),
                   (6, 20), (7, 19), (8, 19)]:
        px(x, y, rock_hi)
    for (x, y) in [(18, 9), (19, 10), (20, 12), (21, 13),
                   (22, 16), (23, 17), (24, 17),
                   (25, 20), (26, 21)]:
        px(x, y, rock_mid)

    # Bottom shadow
    for (x, y) in [(5, 26), (6, 26), (7, 26), (20, 26), (21, 26), (22, 26),
                   (8, 25), (9, 25), (23, 25), (24, 25), (25, 25)]:
        px(x, y, rock_dark)

    # Rust veins — diagonal streaks across the face
    veins = [
        (10, 18, rust_dark), (11, 18, rust), (12, 18, rust),
        (13, 17, rust_hi), (14, 17, rust), (15, 17, rust),
        (16, 16, rust_dark), (17, 16, rust),
        (18, 15, rust), (19, 15, rust_dark),

        (12, 22, rust), (13, 22, rust_dark),
        (14, 23, rust_hi), (15, 23, rust),
        (16, 23, rust), (17, 22, rust_dark),
        (19, 22, rust),
        (22, 21, rust_dark), (23, 21, rust),

        (15, 11, rust),
        (16, 12, rust_dark),
        (17, 13, rust_hi),
    ]
    for (x, y, c) in veins:
        px(x, y, c)
    return img


# ---------- driver ---------- #


def main() -> None:
    out_root = os.path.join("assets", "overworld_objects")
    targets = [
        ("fishing_rod",  make_fishing_rod),
        ("pickaxe",      make_pickaxe),
        ("herb_knife",   make_herb_knife),
        ("raw_fish",     make_raw_fish),
        ("green_herb",   make_green_herb),
        ("iron_ore",     make_iron_ore),
        ("fishing_spot", make_fishing_spot),
        ("herb_patch",   make_herb_patch),
        ("ore_node",     make_ore_node),
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
