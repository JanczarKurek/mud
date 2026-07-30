"""
Generates the static item and prop sprites for the `hollow_bell` module.

  Small pickups / consumables (16x16, matching potion & apple):
    tallow_wax, bell_bronze_ore, deep_iron_shard, pit_tea, miners_draught,
    tallow_candle, greater_heal_potion, knells_shard, deeplisteners_ear,
    hollow_bell_charm
  Gear and heavy items (32x32, matching gen_balance_gear_sprites.py):
    bell_clapper, tallow_crown, bellwrights_hammer, singing_pick,
    recast_hollow_bell_item
  World props (32x32, matching ore_node):
    seam_face, deep_iron_vein, cracked_bell,
    foundry_furnace (cold.png + lit.png)

Chunky pixels, 2-3 shading levels per material, no anti-aliasing,
transparent background, small ground shadow on the standing props.

Run under nix-shell:
  nix-shell -p python3Packages.pillow --run "python3 scripts/gen_hollow_bell_items.py"
"""

from __future__ import annotations

import os

from PIL import Image

MODULE_DIR = "assets/modules/hollow_bell/overworld_objects"
BG = (0, 0, 0, 0)
SHADOW = (0, 0, 0, 70)

# ── shared palettes ───────────────────────────────────────────────────────────
BRONZE_D = (128, 108, 52, 255)
BRONZE = (198, 174, 96, 255)
BRONZE_H = (238, 224, 160, 255)
SEAM_D = (150, 168, 140, 255)
SEAM = (198, 214, 178, 255)
SEAM_H = (238, 248, 226, 255)
DEEP_D = (26, 28, 38, 255)
DEEP = (46, 50, 64, 255)
DEEP_H = (96, 112, 142, 255)
IRON_D = (72, 70, 68, 255)
IRON = (116, 112, 108, 255)
IRON_H = (170, 166, 160, 255)
WOOD_D = (72, 50, 30, 255)
WOOD = (118, 86, 52, 255)
WAX_D = (168, 148, 76, 255)
WAX = (222, 206, 148, 255)
WAX_H = (244, 236, 200, 255)
GLASS_D = (58, 44, 28, 255)


class C:
    def __init__(self, size: int):
        self.n = size
        self.img = Image.new("RGBA", (size, size), BG)

    def px(self, x, y, c):
        if 0 <= x < self.n and 0 <= y < self.n and c[3]:
            self.img.putpixel((int(x), int(y)), c)

    def rect(self, x, y, w, h, c):
        for dy in range(int(h)):
            for dx in range(int(w)):
                self.px(x + dx, y + dy, c)

    def ellipse(self, cx, cy, rx, ry, c):
        for dy in range(-int(ry), int(ry) + 1):
            for dx in range(-int(rx), int(rx) + 1):
                if rx and ry and (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) <= 1.0:
                    self.px(cx + dx, cy + dy, c)

    def shadow(self, cx, y, half):
        self.rect(cx - half, y, half * 2 + 1, 1, SHADOW)
        inner = max(half - 2, 1)
        self.rect(cx - inner, y + 1, inner * 2 + 1, 1, SHADOW)


def save(canvas: C, obj_id: str, name: str = "sprite.png") -> None:
    out_dir = os.path.join(MODULE_DIR, obj_id)
    os.makedirs(out_dir, exist_ok=True)
    canvas.img.save(os.path.join(out_dir, name))
    print(f"  {obj_id}/{name} {canvas.img.size}")


# ── small pickups (16x16) ─────────────────────────────────────────────────────
def tallow_wax():
    c = C(16)
    c.ellipse(8, 9, 5, 4, WAX)
    c.ellipse(7, 7, 3, 2, WAX_H)
    c.rect(3, 9, 11, 1, WAX_D)
    # waxed cloth wrap + string
    c.rect(3, 10, 11, 3, (196, 186, 158, 255))
    c.rect(3, 12, 11, 1, (152, 142, 118, 255))
    for x in range(3, 14, 3):
        c.px(x, 11, (110, 96, 70, 255))
    return c


def bell_bronze_ore():
    c = C(16)
    c.ellipse(8, 9, 5, 4, (72, 68, 62, 255))     # dark matrix rock
    c.rect(4, 6, 3, 3, SEAM)
    c.rect(9, 7, 3, 4, SEAM)
    c.rect(6, 10, 3, 2, SEAM_D)
    c.px(5, 6, SEAM_H)
    c.px(10, 7, SEAM_H)
    c.px(11, 9, BRONZE)
    c.px(6, 11, BRONZE)
    return c


def deep_iron_shard():
    c = C(16)
    # a jagged wedge
    for i in range(9):
        c.rect(4 + i // 2, 3 + i, 7 - i // 2, 1, DEEP)
    c.rect(5, 4, 2, 6, DEEP_H)
    c.rect(8, 7, 2, 4, DEEP_D)
    c.px(5, 3, DEEP_H)
    # frost beading
    c.px(3, 8, (196, 220, 236, 255))
    c.px(11, 6, (196, 220, 236, 255))
    c.px(10, 12, (196, 220, 236, 255))
    return c


def pit_tea():
    c = C(16)
    c.rect(4, 6, 8, 8, (158, 156, 150, 255))     # dented tin cup
    c.rect(4, 6, 8, 1, (196, 194, 188, 255))
    c.rect(4, 13, 8, 1, (110, 108, 104, 255))
    c.rect(5, 7, 6, 2, (74, 46, 24, 255))        # very dark tea
    c.px(6, 7, (118, 82, 46, 255))
    c.rect(12, 8, 2, 1, (158, 156, 150, 255))    # handle
    c.rect(13, 9, 1, 2, (158, 156, 150, 255))
    c.rect(12, 11, 2, 1, (158, 156, 150, 255))
    c.px(6, 6, BG)                               # the chip out of the rim
    # steam
    c.px(7, 4, (214, 214, 210, 160))
    c.px(8, 3, (214, 214, 210, 130))
    return c


def miners_draught():
    c = C(16)
    c.rect(6, 3, 4, 3, (104, 66, 38, 255))       # neck
    c.rect(6, 3, 4, 1, (58, 42, 24, 255))        # waxed cork
    c.ellipse(8, 10, 4, 5, (104, 66, 38, 255))   # squat brown bottle
    c.ellipse(6, 8, 2, 2, (146, 104, 66, 255))
    c.rect(5, 10, 7, 3, (206, 198, 176, 255))    # paper label
    c.rect(6, 11, 5, 1, (86, 72, 52, 255))       # ink
    return c


def tallow_candle():
    c = C(16)
    c.rect(6, 5, 4, 9, WAX)                      # uneven stub
    c.rect(6, 5, 1, 9, WAX_H)
    c.rect(9, 6, 1, 8, WAX_D)
    c.px(6, 8, WAX_D)
    c.px(9, 11, WAX_H)
    c.rect(7, 2, 1, 3, (48, 40, 34, 255))        # long black wick
    c.px(7, 1, (252, 196, 96, 255))
    return c


def greater_heal_potion():
    c = C(16)
    c.rect(6, 2, 4, 2, (188, 190, 198, 255))     # silver-wired stopper
    c.rect(6, 4, 4, 2, (152, 156, 166, 255))
    c.ellipse(8, 10, 5, 5, (172, 32, 44, 255))   # cut-glass flask
    c.ellipse(6, 8, 2, 2, (226, 96, 96, 255))
    c.rect(4, 10, 9, 1, (120, 18, 30, 255))
    c.px(11, 12, (226, 96, 96, 255))
    return c


def knells_shard():
    c = C(16)
    for i in range(8):                            # sliver of seam-bronze
        c.rect(7, 3 + i, 3 - i // 4, 1, SEAM)
    c.rect(7, 4, 1, 6, SEAM_H)
    c.px(8, 10, SEAM_D)
    # fine silver chain
    for x, y in ((5, 3), (4, 4), (3, 5), (11, 3), (12, 4), (13, 5)):
        c.px(x, y, (198, 202, 210, 255))
    return c


def deeplisteners_ear():
    c = C(16)
    c.ellipse(8, 9, 5, 5, (38, 40, 48, 255))     # polished black disc
    c.ellipse(8, 9, 3, 3, (56, 60, 72, 255))     # concentric ridges
    c.ellipse(8, 9, 1, 1, (96, 150, 196, 255))   # glow deep inside
    c.px(6, 7, (86, 92, 108, 255))
    c.rect(7, 2, 2, 3, (96, 72, 46, 255))        # leather thong
    return c


def hollow_bell_charm():
    c = C(16)
    c.ellipse(8, 8, 4, 4, BRONZE)                # tiny hand-bell
    c.rect(4, 8, 9, 3, BRONZE)
    c.rect(4, 11, 9, 1, BRONZE_D)
    c.ellipse(6, 6, 2, 2, BRONZE_H)
    c.rect(7, 3, 2, 2, BRONZE_D)                 # crown loop
    c.rect(6, 13, 5, 1, (168, 148, 84, 255))     # the ring band
    return c


# ── gear and heavy items (32x32) ──────────────────────────────────────────────
def bell_clapper():
    c = C(32)
    c.shadow(16, 27, 7)
    c.rect(14, 4, 4, 16, IRON)                   # shaft
    c.rect(14, 4, 1, 16, IRON_H)
    c.rect(17, 6, 1, 14, IRON_D)
    c.ellipse(16, 22, 6, 5, IRON)                # bulbous striking end
    c.ellipse(14, 20, 3, 2, IRON_H)              # polished mirror-bright
    c.ellipse(16, 24, 4, 2, IRON_D)
    c.rect(13, 2, 6, 2, IRON_D)                  # hanging eye
    c.px(20, 12, WAX)                            # wax spatter
    c.px(11, 17, WAX)
    return c


def tallow_crown():
    c = C(32)
    c.shadow(16, 26, 8)
    c.ellipse(16, 18, 9, 6, IRON)                # honest iron half-helm
    c.ellipse(13, 14, 4, 3, IRON_H)
    c.rect(7, 18, 19, 3, IRON_D)                 # brow band
    for x in (10, 14, 19, 23):                   # dents all over the crown
        c.px(x, 13, IRON_D)
    for i, x in enumerate((8, 13, 18, 24)):      # runnels of hardened wax
        c.rect(x, 16 + i % 2, 2, 7, WAX)
        c.px(x, 22 + i % 2, WAX_D)
    return c


def bellwrights_hammer():
    c = C(32)
    c.shadow(16, 28, 6)
    for i in range(20):                          # ash shaft
        c.rect(15, 8 + i, 3, 1, WOOD if i % 4 else WOOD_D)
    c.rect(14, 16, 5, 5, (86, 62, 38, 255))      # worn leather binding
    GREEN = (122, 148, 108, 255)
    GREEN_H = (162, 186, 146, 255)
    GREEN_D = (82, 104, 72, 255)
    c.rect(7, 4, 19, 8, GREEN)                   # heavy bronze head
    c.rect(7, 4, 19, 2, GREEN_H)
    c.rect(7, 10, 19, 2, GREEN_D)
    for x in range(9, 25, 3):                    # ring of tiny bell-marks
        c.px(x, 8, (68, 88, 62, 255))
    return c


def singing_pick():
    c = C(32)
    c.shadow(16, 28, 6)
    for i in range(20):                          # scarred ash haft
        c.rect(15, 9 + i, 3, 1, WOOD if i % 5 else WOOD_D)
    c.px(16, 14, (58, 40, 24, 255))
    # crystalline bell-bronze head, curved
    for i in range(9):
        c.rect(6 + i, 8 - abs(i - 4) // 2, 2, 2, SEAM)
        c.rect(18 + i, 4 + abs(i - 4) // 2, 2, 2, SEAM)
    c.px(6, 8, SEAM_H)
    c.px(26, 8, SEAM_H)
    c.rect(14, 6, 5, 4, SEAM_D)                  # collar
    return c


def recast_hollow_bell_item():
    c = C(32)
    c.shadow(16, 29, 10)
    c.ellipse(16, 20, 11, 8, BRONZE)             # waist-high bell, fresh cast
    c.rect(5, 20, 23, 6, BRONZE)
    c.rect(5, 26, 23, 2, BRONZE_D)               # lip
    c.ellipse(11, 15, 4, 3, BRONZE_H)            # highlight on the shoulder
    c.rect(9, 8, 15, 4, DEEP)                    # band of black deep-iron
    c.rect(9, 8, 15, 1, DEEP_H)
    c.rect(14, 4, 5, 4, BRONZE_D)                # crown loop
    c.rect(15, 22, 3, 6, IRON)                   # the old clapper, within
    c.ellipse(16, 28, 2, 2, IRON_D)
    return c


# ── world props (32x32) ───────────────────────────────────────────────────────
def seam_face():
    c = C(32)
    c.rect(2, 4, 28, 26, (62, 58, 56, 255))      # dark rock face
    c.rect(2, 4, 28, 2, (86, 80, 76, 255))
    for x, y, w, h in ((6, 9, 5, 12), (14, 7, 6, 16), (23, 11, 4, 11)):
        c.rect(x, y, w, h, SEAM)                 # pale crystalline ore
        c.rect(x, y, 2, h, SEAM_H)
        c.rect(x + w - 1, y + 2, 1, h - 3, SEAM_D)
    c.px(8, 12, BRONZE)                          # bronze veining
    c.px(17, 16, BRONZE)
    c.px(25, 18, BRONZE)
    c.rect(4, 28, 24, 2, (44, 42, 40, 255))      # rubble at the foot
    return c


def deep_iron_vein():
    c = C(32)
    c.rect(2, 4, 28, 26, (58, 54, 56, 255))      # older rock, greyer
    c.rect(2, 4, 28, 2, (80, 76, 78, 255))
    for i in range(13):                          # a black seam running crosswise
        c.rect(4 + i * 2, 8 + i, 4, 3, DEEP)
        c.px(4 + i * 2, 8 + i, DEEP_H)
    for x, y in ((7, 7), (14, 13), (22, 20), (26, 24), (10, 22)):
        c.px(x, y, (196, 220, 236, 255))         # frost beaded on the wall
    c.rect(4, 28, 24, 2, (40, 38, 40, 255))
    return c


def cracked_bell():
    c = C(32)
    c.shadow(16, 30, 11)
    DARK = (138, 126, 96, 255)                   # two centuries of hands
    DARK_H = (176, 164, 128, 255)
    DARK_D = (98, 88, 66, 255)
    c.rect(6, 3, 3, 8, IRON_D)                   # the iron frame
    c.rect(23, 3, 3, 8, IRON_D)
    c.rect(6, 3, 20, 2, IRON)
    c.ellipse(16, 18, 10, 8, DARK)               # the bell itself
    c.rect(6, 18, 21, 8, DARK)
    c.rect(6, 26, 21, 2, DARK_D)
    c.ellipse(12, 14, 3, 3, DARK_H)
    c.rect(14, 6, 4, 4, DARK_D)                  # crown
    # THE SPLIT, lip to crown.
    for i in range(18):
        c.px(19 + i // 6, 10 + i, (32, 28, 22, 255))
        if i % 3 == 0:
            c.px(20 + i // 6, 10 + i, (52, 46, 38, 255))
    # No tongue in it.
    return c


def foundry_furnace(lit: bool):
    c = C(32)
    c.shadow(16, 30, 12)
    BRICK_D = (94, 52, 34, 255)
    BRICK = (134, 76, 48, 255)
    BRICK_H = (166, 100, 66, 255)
    c.rect(3, 6, 26, 23, BRICK)                  # squat brick-and-iron drum
    c.rect(3, 6, 26, 2, BRICK_H)
    c.rect(3, 27, 26, 2, BRICK_D)
    for row in range(6):                         # coursing
        c.rect(3, 9 + row * 3, 26, 1, BRICK_D)
    c.rect(1, 4, 30, 3, IRON_D)                  # iron banding and hood
    c.rect(1, 4, 30, 1, IRON)
    c.rect(11, 0, 10, 5, IRON_D)                 # chimney
    # The mouth.
    if lit:
        c.rect(9, 15, 14, 11, (250, 176, 60, 255))
        c.rect(11, 17, 10, 8, (254, 224, 140, 255))
        c.rect(13, 19, 6, 5, (255, 250, 220, 255))
        for x in (6, 24):                        # light spilling on the brick
            c.rect(x, 16, 2, 8, (222, 150, 80, 255))
        c.rect(13, 0, 6, 2, (120, 100, 90, 200))  # heat haze at the chimney
    else:
        c.rect(9, 15, 14, 11, (34, 26, 22, 255))
        c.rect(11, 17, 10, 8, (22, 18, 16, 255))
        for x, y in ((12, 24), (16, 23), (19, 25)):
            c.px(x, y, (58, 52, 48, 255))        # clinker, two centuries of it
    return c


def main() -> None:
    print("hollow_bell item sprites:")
    for obj_id, fn in (
        ("tallow_wax", tallow_wax),
        ("bell_bronze_ore", bell_bronze_ore),
        ("deep_iron_shard", deep_iron_shard),
        ("pit_tea", pit_tea),
        ("miners_draught", miners_draught),
        ("tallow_candle", tallow_candle),
        ("greater_heal_potion", greater_heal_potion),
        ("knells_shard", knells_shard),
        ("deeplisteners_ear", deeplisteners_ear),
        ("hollow_bell_charm", hollow_bell_charm),
        ("bell_clapper", bell_clapper),
        ("tallow_crown", tallow_crown),
        ("bellwrights_hammer", bellwrights_hammer),
        ("singing_pick", singing_pick),
        ("recast_hollow_bell_item", recast_hollow_bell_item),
        ("seam_face", seam_face),
        ("deep_iron_vein", deep_iron_vein),
        ("cracked_bell", cracked_bell),
    ):
        save(fn(), obj_id)

    save(foundry_furnace(lit=False), "foundry_furnace", "cold.png")
    save(foundry_furnace(lit=True), "foundry_furnace", "lit.png")
    # The base `render.sprite_path` points at cold.png, but ship a sprite.png
    # too so the generic pickup/inspect paths always find one.
    save(foundry_furnace(lit=False), "foundry_furnace", "sprite.png")


if __name__ == "__main__":
    main()
