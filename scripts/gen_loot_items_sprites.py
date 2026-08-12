"""
Generates the item sprites for the loot content batch.

  Food (16x16, matching apple & berries):
    bread_loaf, cheese_wedge, forest_nuts, dried_meat, honeycomb,
    travel_ration, ale_mug
  Potions (16x16, matching potion):
    minor_heal_potion, heal_potion, mana_draught
  Trinkets — sell-fodder, no equip slot (16x16):
    rat_tail, cracked_bone, goblin_ear, sheep_wool, carved_bone_die,
    raw_hide, glass_bead_string, wolf_pelt, bent_silver_spoon,
    tarnished_locket, giant_tooth, ember_shard, amber_lump, grave_ring

All 16x16 flat top-down decals: per docs/sprite_style.md, dropped items are
ground decals and are the one category that is *not* drawn in the sheared
cabinet projection. Chunky pixels, 2-3 shading levels per material, no
anti-aliasing, transparent background. Deterministic — no `random`.

Run under nix-shell:
  nix-shell -p python3Packages.pillow --run "python3 scripts/gen_loot_items_sprites.py"
"""

from __future__ import annotations

import os

from PIL import Image

OBJ_DIR = "assets/overworld_objects"
BG = (0, 0, 0, 0)

# ── shared palettes ───────────────────────────────────────────────────────────
CRUST_D = (128, 88, 40, 255)
CRUST = (176, 128, 62, 255)
CRUST_H = (214, 172, 108, 255)
CRUMB = (232, 210, 168, 255)

CHEESE_D = (194, 156, 56, 255)
CHEESE = (226, 196, 96, 255)
CHEESE_H = (244, 226, 152, 255)
RIND = (160, 120, 44, 255)

NUT_D = (92, 66, 38, 255)
NUT = (142, 106, 66, 255)
NUT_H = (188, 154, 108, 255)

MEAT_D = (86, 36, 28, 255)
MEAT = (132, 62, 48, 255)
MEAT_H = (176, 100, 78, 255)

HONEY_D = (176, 122, 24, 255)
HONEY = (228, 176, 52, 255)
HONEY_H = (250, 214, 118, 255)

CLOTH_D = (124, 108, 78, 255)
CLOTH = (168, 148, 112, 255)
CLOTH_H = (206, 190, 158, 255)

WOOD_D = (96, 60, 28, 255)
WOOD = (140, 92, 44, 255)
WOOD_H = (182, 134, 78, 255)
ALE_D = (128, 74, 22, 255)
ALE = (172, 108, 44, 255)
FOAM = (238, 228, 202, 255)

GLASS_D = (58, 52, 60, 255)
GLASS_H = (206, 214, 226, 200)
CORK = (172, 138, 88, 255)

BONE_D = (168, 160, 136, 255)
BONE = (214, 208, 188, 255)
BONE_H = (244, 240, 226, 255)

SILVER_D = (128, 132, 142, 255)
SILVER = (192, 196, 204, 255)
SILVER_H = (236, 240, 246, 255)

HIDE_D = (98, 70, 44, 255)
HIDE = (136, 100, 68, 255)
HIDE_H = (172, 138, 100, 255)

PELT_D = (84, 80, 74, 255)
PELT = (118, 112, 104, 255)
PELT_H = (162, 156, 148, 255)

WOOL_D = (196, 190, 178, 255)
WOOL = (232, 228, 218, 255)
WOOL_H = (250, 248, 244, 255)


class C:
    def __init__(self, size: int = 16):
        self.n = size
        self.img = Image.new("RGBA", (size, size), BG)

    def px(self, x, y, c):
        if 0 <= x < self.n and 0 <= y < self.n and c[3]:
            self.img.putpixel((int(x), int(y)), c)

    def clear(self, x, y):
        """Punch a hole. `px` deliberately ignores fully-transparent colors so
        that shading passes can no-op, so cutting a hole needs its own door."""
        if 0 <= x < self.n and 0 <= y < self.n:
            self.img.putpixel((int(x), int(y)), BG)

    def clear_ellipse(self, cx, cy, rx, ry):
        for dy in range(-int(ry), int(ry) + 1):
            for dx in range(-int(rx), int(rx) + 1):
                if rx and ry and (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) <= 1.0:
                    self.clear(cx + dx, cy + dy)

    def clear_rect(self, x, y, w, h):
        for dy in range(int(h)):
            for dx in range(int(w)):
                self.clear(x + dx, y + dy)

    def rect(self, x, y, w, h, c):
        for dy in range(int(h)):
            for dx in range(int(w)):
                self.px(x + dx, y + dy, c)

    def ellipse(self, cx, cy, rx, ry, c):
        for dy in range(-int(ry), int(ry) + 1):
            for dx in range(-int(rx), int(rx) + 1):
                if rx and ry and (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) <= 1.0:
                    self.px(cx + dx, cy + dy, c)

    def line(self, x0, y0, x1, y1, c):
        """Integer Bresenham — keeps diagonals crisp and un-antialiased."""
        x0, y0, x1, y1 = int(x0), int(y0), int(x1), int(y1)
        dx, dy = abs(x1 - x0), -abs(y1 - y0)
        sx = 1 if x0 < x1 else -1
        sy = 1 if y0 < y1 else -1
        err = dx + dy
        while True:
            self.px(x0, y0, c)
            if x0 == x1 and y0 == y1:
                break
            e2 = 2 * err
            if e2 >= dy:
                err += dy
                x0 += sx
            if e2 <= dx:
                err += dx
                y0 += sy


def save(canvas: C, obj_id: str, name: str = "sprite.png") -> None:
    out_dir = os.path.join(OBJ_DIR, obj_id)
    os.makedirs(out_dir, exist_ok=True)
    canvas.img.save(os.path.join(out_dir, name))
    print(f"  {obj_id}/{name} {canvas.img.size}")


def flask(body, dark, high, wide=False):
    """Shared potion/vial body: corked neck, sloped shoulders, round belly."""
    c = C()
    rx = 5 if wide else 4
    # cork, then the neck below it
    c.rect(6, 1, 4, 2, CORK)
    c.rect(6, 1, 4, 1, (198, 168, 118, 255))
    c.rect(7, 3, 2, 3, GLASS_D)
    c.px(7, 3, (98, 92, 100, 255))
    # shoulders: widen a row at a time from the neck out to the belly
    for i, w in enumerate(range(2, rx * 2, 2)):
        c.rect(8 - w // 2, 6 + i, w, 1, body)
    # belly
    c.ellipse(8, 11, rx, 3, body)
    c.rect(8 - rx + 1, 14, rx * 2 - 1, 1, dark)
    c.ellipse(7, 10, rx - 2, 1, high)
    # glass glint down the left shoulder
    c.px(8 - rx + 1, 9, GLASS_H)
    c.px(8 - rx + 1, 10, GLASS_H)
    return c


# ── food (16x16) ──────────────────────────────────────────────────────────────
def bread_loaf():
    c = C()
    c.ellipse(8, 9, 6, 4, CRUST)
    c.ellipse(7, 7, 4, 2, CRUST_H)
    c.rect(2, 11, 13, 1, CRUST_D)
    c.rect(3, 12, 11, 1, CRUST_D)
    # the three slashes across the top
    for x in (5, 8, 11):
        c.line(x - 1, 6, x + 1, 9, CRUMB)
    return c


def cheese_wedge():
    c = C()
    # a wedge: tall at the rind edge, tapering to the point
    for i in range(9):
        c.rect(3 + i, 5 + i // 2, 1, 8 - i // 2, CHEESE)
    c.rect(3, 5, 1, 8, RIND)
    c.rect(4, 5, 1, 8, CHEESE_H)
    # holes
    c.px(6, 8, CHEESE_D)
    c.px(7, 10, CHEESE_D)
    c.px(9, 9, CHEESE_D)
    c.rect(3, 13, 9, 1, RIND)
    return c


def forest_nuts():
    c = C()
    for cx, cy in ((5, 9), (10, 8), (8, 12)):
        c.ellipse(cx, cy, 3, 2, NUT)
        c.ellipse(cx - 1, cy - 1, 1, 1, NUT_H)
        c.rect(cx - 3, cy + 1, 7, 1, NUT_D)
        c.px(cx + 3, cy - 1, NUT_D)  # the little stem nub
    return c


def dried_meat():
    c = C()
    # A strip that curls: narrow, leaning, and ragged along both edges rather
    # than a filled rectangle (which read as a book).
    rows = ((6, 4), (5, 4), (5, 5), (4, 5), (4, 5), (5, 4), (5, 5), (6, 4), (6, 4), (7, 3))
    for i, (x, w) in enumerate(rows):
        c.rect(x, 3 + i, w, 1, MEAT)
        c.px(x, 3 + i, MEAT_H)                    # lit left edge
        c.px(x + w - 1, 3 + i, MEAT_D)            # shaded right edge
    # the grain of the muscle, running with the curl
    for i in range(1, 9, 2):
        c.px(rows[i][0] + 2, 3 + i, MEAT_D)
    c.rect(7, 13, 3, 1, MEAT_D)
    return c


def honeycomb():
    c = C()
    # A broken-off slab, squarer than a disc so the cell grid reads.
    c.rect(2, 3, 12, 9, HONEY)
    c.rect(2, 3, 12, 1, HONEY_H)
    c.rect(2, 11, 12, 1, HONEY_D)
    c.clear(2, 3)                                 # chipped corners
    c.clear(13, 3)
    c.clear(2, 11)
    # cells: three per row, second row offset half a cell
    for row, (y, x0) in enumerate(((4, 3), (8, 5))):
        for i in range(3):
            x = x0 + i * 4
            c.rect(x, y, 3, 3, HONEY_D)
            c.rect(x, y, 3, 1, (140, 96, 16, 255))   # dark cell mouth
            c.px(x, y + 2, HONEY_H)
    # honey running off the bottom edge
    c.px(6, 12, HONEY)
    c.px(6, 13, HONEY_H)
    c.px(10, 12, HONEY)
    return c


def travel_ration():
    c = C()
    c.rect(3, 5, 10, 8, CLOTH)          # waxed block
    c.rect(3, 5, 10, 1, CLOTH_H)
    c.rect(3, 12, 10, 1, CLOTH_D)
    c.rect(3, 5, 1, 8, CLOTH_H)
    c.rect(12, 5, 1, 8, CLOTH_D)
    # twine cross
    c.rect(7, 5, 2, 8, (110, 88, 56, 255))
    c.rect(3, 8, 10, 1, (110, 88, 56, 255))
    c.px(7, 8, (72, 58, 36, 255))
    return c


def ale_mug():
    c = C()
    c.rect(4, 5, 8, 9, WOOD)            # tankard body
    c.rect(4, 5, 8, 1, WOOD_H)
    c.rect(4, 13, 8, 1, WOOD_D)
    c.rect(5, 6, 6, 6, ALE)
    c.rect(5, 6, 6, 2, FOAM)            # head
    c.px(6, 5, FOAM)
    c.rect(5, 11, 6, 1, ALE_D)
    # stave lines + handle
    c.rect(7, 5, 1, 9, WOOD_D)
    c.rect(12, 7, 2, 1, WOOD)
    c.rect(13, 8, 1, 2, WOOD)
    c.rect(12, 10, 2, 1, WOOD)
    return c


# ── potions (16x16) ───────────────────────────────────────────────────────────
def minor_heal_potion():
    return flask((204, 72, 84, 255), (152, 40, 52, 255), (238, 130, 138, 255))


def heal_potion():
    return flask((186, 40, 56, 255), (128, 22, 34, 255), (226, 96, 110, 255), wide=True)


def mana_draught():
    return flask((64, 112, 214, 255), (36, 68, 154, 255), (128, 172, 244, 255))


# ── trinkets (16x16) ──────────────────────────────────────────────────────────
def rat_tail():
    c = C()
    TAIL_D = (96, 74, 68, 255)
    TAIL = (128, 104, 96, 255)
    TAIL_H = (170, 146, 136, 255)
    # A loose coil, drawn thick-to-thin so it reads as tapering rather than as
    # a length of rope. Root at bottom-left, tip curling up and over.
    seg = [
        ((3, 13), (6, 13), 2),
        ((6, 13), (10, 12), 2),
        ((10, 12), (12, 9), 1),
        ((12, 9), (10, 6), 1),
        ((10, 6), (7, 5), 1),
        ((7, 5), (5, 3), 1),
    ]
    for (x0, y0), (x1, y1), thick in seg:
        c.line(x0, y0, x1, y1, TAIL)
        if thick > 1:
            c.line(x0, y0 - 1, x1, y1 - 1, TAIL)
    c.rect(2, 12, 2, 3, TAIL_D)                   # cut stump
    c.px(2, 12, (150, 76, 70, 255))               # a little blood at the cut
    for x, y in ((7, 12), (11, 10), (10, 7)):     # scale banding
        c.px(x, y, TAIL_H)
    c.px(5, 3, TAIL_H)                            # tip
    return c


def cracked_bone():
    c = C()
    c.rect(4, 7, 8, 3, BONE)                      # shaft
    c.rect(4, 7, 8, 1, BONE_H)
    c.rect(4, 9, 8, 1, BONE_D)
    for cx in (3, 12):                            # knuckle ends
        c.ellipse(cx, 6, 2, 2, BONE)
        c.ellipse(cx, 10, 2, 2, BONE)
        c.px(cx - 1, 5, BONE_H)
    c.line(5, 8, 10, 8, BONE_D)                   # the split
    c.px(7, 7, BONE_D)
    return c


def goblin_ear():
    c = C()
    EAR_D = (72, 110, 52, 255)
    EAR = (110, 158, 82, 255)
    EAR_H = (156, 200, 126, 255)
    # Long and swept: a fat lobe at the bottom drawing up to a point at the
    # top-right. Widths are hand-listed so the taper is deliberate.
    widths = [(4, 6), (4, 6), (5, 5), (5, 5), (6, 4), (6, 3), (7, 3), (8, 2), (9, 2), (10, 1)]
    for i, (x, w) in enumerate(widths):
        c.rect(x, 13 - i, w, 1, EAR)
    c.rect(4, 12, 2, 2, EAR_H)                    # lit edge of the lobe
    c.rect(8, 7, 1, 3, EAR_D)                     # shaded inner fold
    c.rect(7, 9, 1, 3, EAR_D)
    c.clear_rect(9, 5, 2, 2)                      # the notch bitten out
    c.px(8, 5, EAR_D)
    c.px(10, 4, EAR_H)                            # tip
    return c


def sheep_wool():
    c = C()
    for cx, cy, r in ((6, 8, 3), (10, 7, 3), (8, 11, 3), (11, 11, 2)):
        c.ellipse(cx, cy, r, r - 1, WOOL)
    for cx, cy in ((5, 7), (9, 6), (7, 10)):
        c.px(cx, cy, WOOL_H)
    c.rect(4, 12, 9, 1, WOOL_D)
    c.px(12, 9, WOOL_D)
    return c


def carved_bone_die():
    c = C()
    c.rect(4, 4, 9, 9, BONE)
    c.rect(4, 4, 9, 1, BONE_H)
    c.rect(4, 4, 1, 9, BONE_H)
    c.rect(12, 4, 1, 9, BONE_D)
    c.rect(4, 12, 9, 1, BONE_D)
    for x, y in ((6, 6), (10, 6), (8, 8), (6, 10), (10, 10)):   # five pips
        c.px(x, y, (44, 40, 36, 255))
    return c


def raw_hide():
    c = C()
    # A rolled hide seen end-on: a stubby cylinder with the spiral of the roll
    # showing on the near face. The visible spiral is what separates this from
    # a crate at 16px.
    c.rect(6, 4, 8, 8, HIDE)                      # the barrel of the roll
    c.rect(6, 4, 8, 1, HIDE_H)
    c.rect(6, 11, 8, 1, HIDE_D)
    c.px(13, 4, HIDE_D)
    c.px(13, 11, HIDE_D)
    # near end-face, lighter, with the spiral wound into it
    c.ellipse(5, 8, 3, 4, HIDE_H)
    c.ellipse(5, 8, 2, 2, HIDE)
    c.px(5, 8, HIDE_D)
    c.px(6, 7, HIDE_D)
    c.px(4, 9, HIDE_D)
    # the loose flap of the outer edge, still lifting off the barrel
    c.rect(9, 3, 4, 1, HIDE_H)
    c.px(13, 3, HIDE)
    for x, y in ((9, 7), (12, 9)):                # stiff creases
        c.px(x, y, HIDE_D)
    return c


def glass_bead_string():
    c = C()
    beads = [
        (4, 6, (92, 168, 188, 255)),
        (6, 4, (188, 96, 108, 255)),
        (9, 4, (208, 184, 72, 255)),
        (11, 6, (108, 160, 96, 255)),
        (12, 9, (92, 168, 188, 255)),
        (10, 11, (152, 108, 184, 255)),
        (7, 12, (208, 184, 72, 255)),
        (5, 10, (188, 96, 108, 255)),
    ]
    for i in range(len(beads)):                   # the thong, drawn under
        x0, y0, _ = beads[i]
        x1, y1, _ = beads[(i + 1) % len(beads)]
        c.line(x0, y0, x1, y1, (86, 62, 40, 255))
    for x, y, col in beads:
        c.ellipse(x, y, 1, 1, col)
        c.px(x - 1, y - 1, (255, 255, 255, 150))
    return c


def wolf_pelt():
    c = C()
    # A skin pegged out flat: narrow snout at the top, four legs sticking out
    # at the diagonals, tail at the bottom. The silhouette is the whole tell —
    # a filled blob just reads as a crate.
    body = ((7, 2), (6, 4), (5, 6), (4, 8), (4, 8), (5, 6), (5, 6), (6, 4), (7, 2))
    for i, (x, w) in enumerate(body):
        c.rect(x, 2 + i, w, 1, PELT)
    # legs, splayed out from the flanks
    c.rect(1, 5, 3, 2, PELT)
    c.rect(12, 5, 3, 2, PELT)
    c.rect(2, 9, 3, 2, PELT)
    c.rect(11, 9, 3, 2, PELT)
    c.rect(7, 11, 2, 4, PELT)                     # tail
    c.px(7, 14, PELT_H)
    # ears at the snout end
    c.px(6, 1, PELT_D)
    c.px(9, 1, PELT_D)
    c.rect(6, 4, 4, 2, PELT_H)                    # pale ruff at the shoulder
    c.rect(5, 9, 6, 1, PELT_D)                    # shaded haunch
    for x in (6, 9):                              # guard-hair flecks
        c.px(x, 7, PELT_D)
        c.px(x, 3, PELT_H)
    return c


def bent_silver_spoon():
    c = C()
    c.ellipse(5, 5, 2, 3, SILVER)                 # bowl
    c.ellipse(5, 4, 1, 1, SILVER_H)
    c.rect(3, 3, 1, 4, SILVER_D)
    # handle, kinked hard at the middle — this is the "bent" part
    c.line(6, 8, 9, 10, SILVER)
    c.line(9, 10, 8, 12, SILVER)
    c.line(8, 12, 12, 13, SILVER)
    c.px(9, 10, SILVER_H)
    c.px(8, 12, SILVER_D)
    return c


def tarnished_locket():
    c = C()
    BRASS_D = (88, 82, 60, 255)
    BRASS = (154, 146, 112, 255)
    BRASS_H = (196, 188, 150, 255)
    # bail + a stub of chain, so it reads as jewellery rather than a coin
    c.rect(7, 1, 3, 1, BRASS)
    c.px(6, 2, BRASS)
    c.px(10, 2, BRASS)
    c.rect(7, 3, 3, 1, BRASS)
    c.clear(8, 2)                                 # the eye of the bail
    c.ellipse(8, 9, 5, 5, BRASS)
    c.ellipse(6, 7, 2, 2, BRASS_H)                # sheen, top-left
    c.rect(4, 9, 9, 1, BRASS_D)                   # the hinge seam
    c.rect(4, 10, 9, 1, (176, 168, 132, 255))     # lip below the seam
    c.px(3, 9, BRASS_D)
    for x, y in ((5, 12), (11, 6), (10, 12), (6, 5)):   # tarnish blooms
        c.px(x, y, (66, 62, 44, 255))
    return c


def giant_tooth():
    c = C()
    ENAMEL = (226, 216, 186, 255)
    ROOT = (192, 178, 146, 255)
    # A molar seen from the side. The crown has to dominate — give it most of
    # the canvas and keep the roots short stubs, or the thing reads as legs.
    for i, (x, w) in enumerate(((3, 10), (2, 12), (2, 12), (2, 12), (3, 10), (4, 8))):
        c.rect(x, 1 + i, w, 1, ENAMEL)
    c.rect(3, 1, 10, 1, BONE_H)                   # lit top of the crown
    c.rect(2, 2, 3, 3, BONE_H)
    c.rect(11, 3, 2, 3, BONE_D)
    c.rect(5, 7, 6, 1, BONE_D)                    # gumline
    # roots: two short stubs that taper to points
    for i, w in enumerate((3, 3, 2, 1)):
        c.rect(4, 8 + i, w, 1, ROOT)
        c.rect(11 - w, 8 + i, w, 1, ROOT)
    c.px(4, 8, (208, 196, 164, 255))
    # the crack down the crown, and the worn flat of the chewing surface
    c.line(8, 2, 8, 6, (176, 164, 132, 255))
    c.px(9, 4, (176, 164, 132, 255))
    c.rect(5, 1, 2, 1, (250, 246, 236, 255))
    return c


def ember_shard():
    c = C()
    # jagged wedge of cooled slag with live heat down one fissure
    for i in range(10):
        c.rect(4 + i // 3, 3 + i, 8 - i // 2, 1, (62, 44, 40, 255))
    c.rect(5, 5, 2, 6, (96, 70, 60, 255))
    c.line(8, 4, 6, 11, (240, 124, 44, 255))      # the glowing fissure
    c.px(8, 5, (252, 208, 120, 255))
    c.px(7, 8, (252, 208, 120, 255))
    c.px(10, 6, (240, 124, 44, 255))
    c.px(3, 10, (240, 124, 44, 160))              # heat shimmer
    c.px(12, 4, (240, 124, 44, 120))
    return c


def amber_lump():
    c = C()
    AMBER_D = (170, 110, 24, 255)
    AMBER = (224, 158, 48, 255)
    AMBER_H = (250, 210, 122, 255)
    # A rounded nugget with two flat facets, so it reads as resin rather than
    # as a sun. Drawn row by row for a slightly irregular silhouette.
    for i, (x, w) in enumerate(
        ((5, 6), (4, 8), (3, 10), (3, 10), (3, 11), (4, 10), (5, 8), (6, 5))
    ):
        c.rect(x, 4 + i, w, 1, AMBER)
    c.rect(5, 4, 4, 1, AMBER_H)
    c.rect(4, 5, 3, 2, AMBER_H)                   # the lit facet
    c.rect(11, 8, 2, 2, AMBER_D)                  # the shaded facet
    c.rect(5, 11, 7, 1, AMBER_D)
    # the insect, suspended dead centre: body, then legs
    c.rect(7, 8, 3, 2, (74, 44, 16, 255))
    c.px(6, 7, (74, 44, 16, 255))
    c.px(10, 7, (74, 44, 16, 255))
    c.px(6, 10, (74, 44, 16, 255))
    c.px(10, 10, (74, 44, 16, 255))
    return c


def grave_ring():
    c = C()
    BAND = (178, 182, 172, 255)
    BAND_D = (112, 116, 108, 255)
    BAND_H = (222, 226, 218, 255)
    c.ellipse(8, 9, 5, 5, BAND)
    c.clear_ellipse(8, 9, 3, 3)                   # the hole through the band
    c.ellipse(6, 6, 2, 1, BAND_H)                 # highlight on the shoulder
    c.rect(6, 13, 5, 1, BAND_D)                   # shaded underside
    c.px(4, 11, BAND_D)
    c.px(12, 11, BAND_D)
    for x, y in ((11, 6), (5, 12), (12, 10)):     # graveyard pitting
        c.px(x, y, (92, 96, 90, 255))
    # the cold coming off it
    c.px(8, 3, (206, 226, 240, 130))
    c.px(13, 7, (206, 226, 240, 110))
    return c


def main():
    print("loot item sprites:")
    for obj_id, fn in (
        ("bread_loaf", bread_loaf),
        ("cheese_wedge", cheese_wedge),
        ("forest_nuts", forest_nuts),
        ("dried_meat", dried_meat),
        ("honeycomb", honeycomb),
        ("travel_ration", travel_ration),
        ("ale_mug", ale_mug),
        ("minor_heal_potion", minor_heal_potion),
        ("heal_potion", heal_potion),
        ("mana_draught", mana_draught),
        ("rat_tail", rat_tail),
        ("cracked_bone", cracked_bone),
        ("goblin_ear", goblin_ear),
        ("sheep_wool", sheep_wool),
        ("carved_bone_die", carved_bone_die),
        ("raw_hide", raw_hide),
        ("glass_bead_string", glass_bead_string),
        ("wolf_pelt", wolf_pelt),
        ("bent_silver_spoon", bent_silver_spoon),
        ("tarnished_locket", tarnished_locket),
        ("giant_tooth", giant_tooth),
        ("ember_shard", ember_shard),
        ("amber_lump", amber_lump),
        ("grave_ring", grave_ring),
    ):
        save(fn(), obj_id)


if __name__ == "__main__":
    main()
