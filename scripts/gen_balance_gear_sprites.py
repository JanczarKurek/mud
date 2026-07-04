"""
Generates static 32x32 PNG sprites for the balance-retune gear batch:

  iron_sword, steel_sword, dagger, longbow,
  chain_helmet, chain_armor, chain_legs, plate_armor, tower_shield

Each object gets a single `sprite.png` in its overworld_objects directory.
Style matches gen_combat_gear_sprites.py — chunky pixels, 2-3 shading levels
per material, no anti-aliasing, transparent background, small ground shadow.
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


# Shared metal palettes.
IRON_DARK = (85, 88, 98, 255)
IRON = (140, 145, 158, 255)
IRON_HI = (195, 200, 214, 255)
STEEL_DARK = (105, 112, 130, 255)
STEEL = (170, 178, 198, 255)
STEEL_HI = (230, 236, 250, 255)
EDGE = (250, 252, 255, 255)
GRIP_DARK = (50, 30, 15, 255)
GRIP = (110, 70, 35, 255)
GRIP_HI = (160, 110, 65, 255)
GUARD_DARK = (70, 72, 82, 255)
GUARD = (120, 124, 138, 255)
GUARD_HI = (175, 180, 196, 255)
LEATHER_DARK = (60, 40, 22, 255)
LEATHER = (110, 76, 42, 255)
LEATHER_HI = (155, 112, 66, 255)


def diagonal_sword(blade_dark, blade, blade_hi, guard_c, pommel_c, long=True) -> Image.Image:
    """Shared diagonal-sword builder: grip lower-left, blade to upper-right."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 27, 8)

    # Pommel
    px(5, 26, GRIP_DARK)
    px(6, 26, pommel_c)
    px(6, 27, GRIP_DARK)
    px(7, 26, pommel_c)

    # Grip
    for (x, y) in [(7, 25), (8, 24), (9, 23), (10, 22)]:
        px(x, y, GRIP)
        px(x - 1, y, GRIP_DARK)
        px(x, y + 1, GRIP_DARK)
        px(x + 1, y, GRIP_HI)
    px(8, 23, GRIP_DARK)

    # Crossguard (perpendicular bar)
    for (x, y) in [(9, 23), (10, 22), (11, 21), (12, 20), (13, 19)]:
        px(x, y, guard_c)
    px(11, 21, GUARD_HI)
    px(12, 20, GUARD_HI)
    px(9, 24, GUARD_DARK)
    px(13, 20, GUARD_DARK)

    # Blade
    tip = 25 if long else 22
    blade_pts = [(11 + i, 21 - i) for i in range(1, tip - 11 + 1)]
    for (x, y) in blade_pts:
        px(x + 1, y, blade_dark)
        px(x, y, blade)
        px(x, y - 1, blade_hi)
        px(x - 1, y - 1, EDGE) if (x + y) % 3 == 0 else None
    # Tip pixel
    tx, ty = blade_pts[-1]
    px(tx + 1, ty - 1, EDGE)
    return img


def make_iron_sword() -> Image.Image:
    return diagonal_sword(IRON_DARK, IRON, IRON_HI, GUARD, IRON_HI)


def make_steel_sword() -> Image.Image:
    return diagonal_sword(STEEL_DARK, STEEL, STEEL_HI, GUARD_HI, STEEL_HI)


def make_dagger() -> Image.Image:
    """Short blade, slim profile, wicked point."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 26, 5)

    # Grip (short, wrapped)
    for (x, y) in [(11, 24), (12, 23)]:
        px(x, y, GRIP)
        px(x - 1, y, GRIP_DARK)
        px(x + 1, y, GRIP_HI)
    px(10, 25, GRIP_DARK)  # pommel

    # Small guard
    px(12, 22, GUARD)
    px(13, 22, GUARD_HI)
    px(13, 23, GUARD_DARK)

    # Blade (short, needle point)
    for i, (x, y) in enumerate([(13 + j, 21 - j) for j in range(6)]):
        px(x, y, STEEL)
        px(x + 1, y, STEEL_DARK)
        px(x, y - 1, STEEL_HI)
    px(19, 15, EDGE)
    px(20, 14, EDGE)
    return img


def make_longbow() -> Image.Image:
    """Tall yew bow: C-curve stave with a taut string."""
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 28, 6)

    wood_dark = (70, 46, 22, 255)
    wood = (125, 85, 45, 255)
    wood_hi = (170, 125, 70, 255)
    string_c = (225, 220, 200, 255)

    # Stave — tall arc from (12,4) bulging right to (18,15) back to (12,26)
    stave = [
        (12, 4), (13, 5), (14, 6), (15, 7), (16, 8), (17, 9), (17, 10),
        (18, 11), (18, 12), (18, 13), (18, 14), (18, 15), (18, 16),
        (18, 17), (18, 18), (17, 19), (17, 20), (16, 21), (15, 22),
        (14, 23), (13, 24), (12, 25),
    ]
    for (x, y) in stave:
        px(x, y, wood)
        px(x - 1, y, wood_dark)
        px(x + 1, y, wood_hi)
    # Grip wrap at the middle
    for y in (13, 14, 15):
        px(18, y, LEATHER)
        px(17, y, LEATHER_DARK)
        px(19, y, LEATHER_HI)
    # String — straight line between nocks
    for y in range(5, 25):
        px(11, y, string_c)
    px(12, 4, wood_hi)
    px(12, 25, wood_dark)
    return img


def chain_texture(px, x0, y0, w, h):
    """Riveted-mail texture: alternating light/dark pixels."""
    for dy in range(h):
        for dx in range(w):
            c = IRON_HI if (dx + dy) % 2 == 0 else IRON
            px(x0 + dx, y0 + dy, c)


def make_chain_helmet() -> Image.Image:
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 26, 7)

    # Dome
    rect(11, 10, 10, 3, IRON)
    rect(10, 12, 12, 4, IRON)
    rect(12, 9, 8, 1, IRON_HI)
    rect(13, 8, 6, 1, IRON_HI)
    # Coif skirt (mail texture)
    chain_texture(px, 10, 16, 12, 6)
    rect(9, 16, 1, 5, IRON_DARK)
    rect(22, 16, 1, 5, IRON_DARK)
    # Face opening
    rect(13, 13, 6, 5, (30, 26, 30, 255))
    rect(13, 12, 6, 1, IRON_DARK)
    # Rim shading
    rect(10, 15, 12, 1, IRON_DARK)
    return img


def make_chain_armor() -> Image.Image:
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 27, 8)

    # Torso hauberk
    chain_texture(px, 10, 8, 12, 14)
    # Shoulders
    chain_texture(px, 8, 8, 3, 4)
    chain_texture(px, 21, 8, 3, 4)
    rect(8, 12, 3, 1, IRON_DARK)
    rect(21, 12, 3, 1, IRON_DARK)
    # Skirt split
    chain_texture(px, 10, 22, 5, 4)
    chain_texture(px, 17, 22, 5, 4)
    # Neck hole + belt
    rect(13, 7, 6, 2, (30, 26, 30, 255))
    rect(10, 17, 12, 2, LEATHER)
    rect(10, 18, 12, 1, LEATHER_DARK)
    # Outline shading
    rect(9, 9, 1, 13, IRON_DARK)
    rect(22, 9, 1, 13, IRON_DARK)
    return img


def make_chain_legs() -> Image.Image:
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 28, 7)

    # Waist band
    rect(10, 8, 12, 2, LEATHER)
    rect(10, 9, 12, 1, LEATHER_DARK)
    # Hips
    chain_texture(px, 10, 10, 12, 5)
    # Legs
    chain_texture(px, 10, 15, 5, 11)
    chain_texture(px, 17, 15, 5, 11)
    rect(9, 10, 1, 16, IRON_DARK)
    rect(22, 10, 1, 16, IRON_DARK)
    rect(15, 15, 2, 8, BG)  # split between the legs
    # Cuffs
    rect(10, 25, 5, 1, IRON_DARK)
    rect(17, 25, 5, 1, IRON_DARK)
    return img


def make_plate_armor() -> Image.Image:
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 28, 8)

    # Breastplate body
    rect(10, 8, 12, 15, STEEL)
    rect(10, 8, 12, 2, STEEL_HI)
    rect(10, 21, 12, 2, STEEL_DARK)
    rect(9, 9, 1, 13, STEEL_DARK)
    rect(22, 9, 1, 13, STEEL_DARK)
    # Central ridge
    for y in range(9, 22):
        px(16, y, STEEL_HI)
        px(15, y, STEEL)
    # Pauldrons
    rect(7, 8, 3, 5, STEEL)
    rect(22, 8, 3, 5, STEEL)
    rect(7, 8, 3, 1, STEEL_HI)
    rect(22, 8, 3, 1, STEEL_HI)
    rect(7, 12, 3, 1, STEEL_DARK)
    rect(22, 12, 3, 1, STEEL_DARK)
    # Neck hole
    rect(13, 7, 6, 2, (30, 26, 30, 255))
    # Rivets
    for (rx, ry) in [(11, 10), (20, 10), (11, 19), (20, 19)]:
        px(rx, ry, STEEL_HI)
    # Waist fauld strips
    rect(10, 23, 12, 1, STEEL)
    rect(10, 24, 12, 1, STEEL_DARK)
    return img


def make_tower_shield() -> Image.Image:
    img = new_img()
    px, rect = helpers(img)
    ground_shadow(rect, 16, 29, 8)

    wood_dark = (72, 58, 40, 255)
    wood = (118, 98, 68, 255)
    wood_hi = (156, 132, 94, 255)

    # Tall body — nearly full height
    rect(10, 4, 12, 23, wood)
    rect(10, 4, 12, 1, wood_hi)
    rect(10, 26, 12, 1, wood_dark)
    rect(9, 5, 1, 21, wood_dark)
    rect(22, 5, 1, 21, wood_dark)
    # Vertical plank seams
    for y in range(5, 26):
        px(14, y, wood_dark)
        px(18, y, wood_dark)
    # Steel rim + center boss
    rect(10, 5, 12, 1, IRON)
    rect(10, 25, 12, 1, IRON)
    rect(14, 13, 4, 4, IRON)
    rect(15, 14, 2, 2, IRON_HI)
    px(14, 13, IRON_DARK)
    px(17, 16, IRON_DARK)
    # Corner rivets
    for (rx, ry) in [(11, 6), (20, 6), (11, 24), (20, 24)]:
        px(rx, ry, IRON_HI)
    return img


MAKERS = {
    "iron_sword": make_iron_sword,
    "steel_sword": make_steel_sword,
    "dagger": make_dagger,
    "longbow": make_longbow,
    "chain_helmet": make_chain_helmet,
    "chain_armor": make_chain_armor,
    "chain_legs": make_chain_legs,
    "plate_armor": make_plate_armor,
    "tower_shield": make_tower_shield,
}


def main() -> None:
    for obj_id, maker in MAKERS.items():
        out_dir = os.path.join("assets", "overworld_objects", obj_id)
        os.makedirs(out_dir, exist_ok=True)
        out = os.path.join(out_dir, "sprite.png")
        maker().save(out)
        print(f"wrote {out}")


if __name__ == "__main__":
    main()
