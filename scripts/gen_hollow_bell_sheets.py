"""
Generates the animated character sheets for the `hollow_bell` module.

  marten_coalbright, sister_wick, tobin_ashfoot, hettie_marl, grandam_bellow
  tallow_drip, sump_crawler, wax_wight, seam_shrike, hollow_spawn
  cinderjack, knell, deeplistener

Sheet layout follows the project convention (see gen_goblin_sheet.py):
  4 columns x 2 rows
  Row 0: idle (4 frames, subtle bob)
  Row 1: walk (4 frames, stride cycle)

Normal characters use 32x48 frames; the three bosses use 64x80, matching the
cyclops precedent for oversized creatures. Each object also gets a static
`sprite.png` (frame 0 of the idle row) for the no-animation fallback path.

Run under nix-shell:
  nix-shell -p python3Packages.pillow --run "python3 scripts/gen_hollow_bell_sheets.py"
"""

from __future__ import annotations

import os

from PIL import Image

MODULE_DIR = "assets/modules/hollow_bell/overworld_objects"
BG = (0, 0, 0, 0)
SHADOW = (0, 0, 0, 60)


# ── drawing helpers ───────────────────────────────────────────────────────────
class Canvas:
    def __init__(self, w: int, h: int):
        self.w = w
        self.h = h
        self.img = Image.new("RGBA", (w, h), BG)

    def px(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h and c[3]:
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

    def shadow(self, cx, base_y, half):
        self.rect(cx - half, base_y, half * 2 + 1, 1, SHADOW)
        inner = max(half - 2, 1)
        self.rect(cx - inner, base_y + 1, inner * 2 + 1, 1, SHADOW)


def write(sheet: Image.Image, frame0: Image.Image, obj_id: str) -> None:
    out_dir = os.path.join(MODULE_DIR, obj_id)
    os.makedirs(out_dir, exist_ok=True)
    sheet.save(os.path.join(out_dir, "sheet.png"))
    frame0.save(os.path.join(out_dir, "sprite.png"))
    print(f"  {obj_id}: sheet {sheet.size}, sprite {frame0.size}")


def build(obj_id: str, fw: int, fh: int, draw_frame) -> None:
    """draw_frame(canvas, row, col) paints one frame."""
    sheet = Image.new("RGBA", (fw * 4, fh * 2), BG)
    first = None
    for row in range(2):
        for col in range(4):
            c = Canvas(fw, fh)
            draw_frame(c, row, col)
            sheet.paste(c.img, (col * fw, row * fh))
            if row == 0 and col == 0:
                first = c.img
    write(sheet, first, obj_id)


# Idle bob and walk stride offsets, shared by every biped below.
IDLE_BOB = [0, -1, 0, 1]
STRIDE = [0, -2, 0, 2]


# ── beastfolk villagers (32x48) ───────────────────────────────────────────────
def biped(
    fur, fur_dark, fur_hi, cloth, cloth_dark, accent, *, ears="round", eye=(24, 20, 18, 255)
):
    """Return a draw_frame for a 32x48 upright beastfolk character."""

    def draw(c: Canvas, row: int, col: int):
        bob = IDLE_BOB[col] if row == 0 else 0
        stride = STRIDE[col] if row == 1 else 0

        c.shadow(16, 45, 6)

        # legs / boots
        c.rect(12, 36 + max(stride, 0), 4, 8, cloth_dark)
        c.rect(17, 36 + max(-stride, 0), 4, 8, cloth_dark)
        c.rect(12, 42 + max(stride, 0), 4, 2, (36, 28, 22, 255))
        c.rect(17, 42 + max(-stride, 0), 4, 2, (36, 28, 22, 255))

        # torso
        c.rect(11, 22 + bob, 11, 15, cloth)
        c.rect(11, 22 + bob, 11, 2, cloth_dark)
        c.rect(11, 29 + bob, 11, 1, accent)          # belt
        c.rect(20, 24 + bob, 2, 12, cloth_dark)       # shading down one side

        # arms
        c.rect(9, 24 + bob - stride // 2, 2, 11, fur_dark)
        c.rect(22, 24 + bob + stride // 2, 2, 11, fur_dark)

        # head
        c.ellipse(16, 15 + bob, 6, 6, fur)
        c.ellipse(15, 13 + bob, 4, 4, fur_hi)
        c.rect(13, 18 + bob, 7, 2, fur_dark)          # muzzle shadow
        c.ellipse(16, 19 + bob, 3, 2, fur_hi)         # muzzle

        # ears
        if ears == "round":
            c.ellipse(11, 11 + bob, 2, 2, fur)
            c.ellipse(21, 11 + bob, 2, 2, fur)
        elif ears == "point":
            c.rect(11, 8 + bob, 2, 5, fur)
            c.rect(20, 8 + bob, 2, 5, fur)
        elif ears == "flat":
            c.rect(10, 12 + bob, 3, 2, fur)
            c.rect(20, 12 + bob, 3, 2, fur)

        # eyes (blink on idle frame 2)
        if row == 0 and col == 2:
            c.rect(13, 15 + bob, 2, 1, eye)
            c.rect(18, 15 + bob, 2, 1, eye)
        else:
            c.rect(13, 14 + bob, 2, 2, eye)
            c.rect(18, 14 + bob, 2, 2, eye)

        c.px(16, 19 + bob, (18, 14, 12, 255))         # nose

    return draw


def marten():
    # Grey badger, soot-black pit-captain's coat with brass buttons.
    d = biped(
        fur=(196, 196, 200, 255),
        fur_dark=(120, 120, 126, 255),
        fur_hi=(228, 228, 232, 255),
        cloth=(44, 42, 46, 255),
        cloth_dark=(28, 26, 30, 255),
        accent=(178, 138, 56, 255),
        ears="round",
    )

    def draw(c, row, col):
        d(c, row, col)
        bob = IDLE_BOB[col] if row == 0 else 0
        # Badger face stripes.
        c.rect(13, 11 + bob, 1, 8, (36, 34, 38, 255))
        c.rect(19, 11 + bob, 1, 8, (36, 34, 38, 255))
        # Brass buttons and the whistle chain.
        c.px(16, 25 + bob, (208, 170, 72, 255))
        c.px(16, 27 + bob, (208, 170, 72, 255))
        c.px(19, 26 + bob, (216, 190, 96, 255))
        # Leather cap with candle-bracket.
        c.rect(11, 9 + bob, 11, 2, (58, 40, 26, 255))
        c.px(21, 8 + bob, (200, 196, 180, 255))

    return draw


def wick():
    # Small brown mouse, undyed homespun, candle stub behind the ear.
    d = biped(
        fur=(158, 132, 104, 255),
        fur_dark=(104, 84, 64, 255),
        fur_hi=(190, 166, 138, 255),
        cloth=(196, 186, 162, 255),
        cloth_dark=(150, 140, 118, 255),
        accent=(120, 96, 62, 255),
        ears="round",
    )

    def draw(c, row, col):
        d(c, row, col)
        bob = IDLE_BOB[col] if row == 0 else 0
        # Big mouse ears.
        c.ellipse(10, 11 + bob, 3, 3, (158, 132, 104, 255))
        c.ellipse(22, 11 + bob, 3, 3, (158, 132, 104, 255))
        c.ellipse(10, 11 + bob, 1, 1, (206, 158, 158, 255))
        c.ellipse(22, 11 + bob, 1, 1, (206, 158, 158, 255))
        # Candle stub behind one ear.
        c.rect(23, 9 + bob, 1, 3, (236, 226, 186, 255))
        c.px(23, 8 + bob, (252, 190, 90, 255))

    return draw


def tobin():
    # Young rust-red fox in a scorched leather apron.
    d = biped(
        fur=(186, 92, 46, 255),
        fur_dark=(126, 58, 28, 255),
        fur_hi=(216, 130, 76, 255),
        cloth=(96, 68, 42, 255),
        cloth_dark=(66, 46, 28, 255),
        accent=(140, 106, 58, 255),
        ears="point",
    )

    def draw(c, row, col):
        d(c, row, col)
        bob = IDLE_BOB[col] if row == 0 else 0
        # White muzzle and cheek flash.
        c.ellipse(16, 19 + bob, 3, 2, (238, 232, 220, 255))
        # Soot on the nose.
        c.px(16, 18 + bob, (60, 52, 48, 255))
        # Apron bib + calipers on the belt.
        c.rect(13, 24 + bob, 7, 6, (112, 80, 50, 255))
        c.px(20, 30 + bob, (176, 178, 186, 255))
        c.px(20, 31 + bob, (176, 178, 186, 255))

    return draw


def hettie():
    # Lean otter, patched canvas jacket, rope over one shoulder.
    d = biped(
        fur=(122, 96, 70, 255),
        fur_dark=(80, 62, 46, 255),
        fur_hi=(154, 126, 96, 255),
        cloth=(118, 112, 88, 255),
        cloth_dark=(84, 78, 60, 255),
        accent=(70, 62, 46, 255),
        ears="flat",
    )

    def draw(c, row, col):
        d(c, row, col)
        bob = IDLE_BOB[col] if row == 0 else 0
        # Coil of rope across the chest.
        for i in range(8):
            c.px(12 + i, 25 + bob + (i % 2), (196, 176, 128, 255))
        # Candle-bracket helmet.
        c.rect(11, 9 + bob, 11, 2, (72, 68, 60, 255))
        c.px(16, 7 + bob, (252, 214, 130, 255))
        # Patches.
        c.rect(12, 32 + bob, 2, 2, (92, 96, 76, 255))

    return draw


def grandam():
    # Translucent pale-blue badger ghost. Edges dissolve into bell ripples;
    # she has no feet.
    GHOST = (150, 196, 224, 210)
    GHOST_D = (104, 150, 186, 190)
    GHOST_H = (206, 232, 246, 220)

    def draw(c: Canvas, row: int, col: int):
        drift = IDLE_BOB[col] if row == 0 else STRIDE[col] // 2

        # Bell-shaped ripple hem instead of legs.
        for i, wdt in enumerate((13, 11, 9, 7, 5)):
            alpha = 150 - i * 26
            c.rect(16 - wdt // 2, 36 + i + drift, wdt, 1, (150, 196, 224, max(alpha, 30)))

        c.rect(11, 22 + drift, 11, 15, GHOST)
        c.rect(11, 22 + drift, 11, 2, GHOST_D)
        c.rect(20, 24 + drift, 2, 12, GHOST_D)
        # Founder's apron.
        c.rect(13, 26 + drift, 7, 9, (176, 212, 232, 200))

        # Hands folded.
        c.rect(13, 32 + drift, 6, 2, GHOST_H)

        c.ellipse(16, 15 + drift, 6, 6, GHOST)
        c.ellipse(15, 13 + drift, 4, 4, GHOST_H)
        c.ellipse(11, 11 + drift, 2, 2, GHOST)
        c.ellipse(21, 11 + drift, 2, 2, GHOST)
        # Badger stripes, ghost-pale.
        c.rect(13, 11 + drift, 1, 8, (86, 132, 170, 200))
        c.rect(19, 11 + drift, 1, 8, (86, 132, 170, 200))
        c.rect(13, 14 + drift, 2, 2, (240, 250, 255, 230))
        c.rect(18, 14 + drift, 2, 2, (240, 250, 255, 230))

    return draw


# ── creatures (32x48) ─────────────────────────────────────────────────────────
def tallow_drip():
    WAX = (214, 190, 92, 255)
    WAX_D = (162, 138, 58, 255)
    WAX_H = (238, 222, 148, 255)
    FLAME = (252, 176, 64, 255)
    FLAME_H = (254, 232, 150, 255)

    def draw(c: Canvas, row: int, col: int):
        squash = IDLE_BOB[col] if row == 0 else STRIDE[col] // 2
        c.shadow(16, 45, 5)
        # A humped blob, low to the ground.
        c.ellipse(16, 40 + squash // 2, 7, 5 - squash // 2, WAX)
        c.ellipse(15, 38 + squash // 2, 4, 3, WAX_H)
        c.rect(9, 43, 14, 2, WAX_D)          # the smear it leaves
        # Two dim eye-points.
        c.px(13, 39 + squash // 2, (60, 44, 16, 255))
        c.px(19, 39 + squash // 2, (60, 44, 16, 255))
        # Guttering flame on top.
        flick = col % 2
        c.rect(15, 32 + flick, 2, 4, FLAME)
        c.px(15, 31 + flick, FLAME_H)
        c.px(16, 33 + flick, FLAME_H)

    return draw


def sump_crawler():
    SHELL = (206, 198, 176, 255)
    SHELL_D = (150, 144, 124, 255)
    SHELL_H = (232, 226, 208, 255)
    LEG = (176, 168, 148, 255)

    def draw(c: Canvas, row: int, col: int):
        wave = col
        c.shadow(16, 45, 8)
        # Long segmented body, undulating.
        for seg in range(7):
            y = 40 - seg * 3
            off = (1 if (seg + wave) % 2 else -1) * (1 if row == 1 else 0)
            c.ellipse(16 + off, y, 4 - seg // 4, 2, SHELL)
            c.ellipse(15 + off, y - 1, 2, 1, SHELL_H)
            c.rect(16 + off - 5, y, 2, 1, LEG)
            c.rect(16 + off + 4, y, 2, 1, LEG)
        c.ellipse(16, 20, 4, 3, SHELL_D)     # blind head
        c.rect(13, 19, 2, 1, (90, 84, 70, 255))
        c.rect(18, 19, 2, 1, (90, 84, 70, 255))

    return draw


def wax_wight():
    WAX = (196, 168, 96, 255)
    WAX_D = (140, 118, 62, 255)
    WAX_H = (222, 200, 140, 255)
    GLOW = (248, 150, 60, 255)
    APRON = (86, 62, 38, 255)

    def draw(c: Canvas, row: int, col: int):
        bob = IDLE_BOB[col] if row == 0 else 0
        stride = STRIDE[col] if row == 1 else 0
        c.shadow(16, 45, 6)
        c.rect(12, 36 + max(stride, 0), 4, 8, WAX_D)
        c.rect(17, 36 + max(-stride, 0), 4, 8, WAX_D)
        c.rect(11, 22 + bob, 11, 15, WAX)
        c.rect(20, 24 + bob, 2, 12, WAX_D)
        # The glow through the chest.
        c.ellipse(16, 28 + bob, 3, 3, GLOW)
        c.ellipse(16, 28 + bob, 1, 1, (254, 226, 170, 255))
        # Scorched foundry apron.
        c.rect(13, 25 + bob, 7, 10, APRON)
        c.rect(13, 33 + bob, 7, 2, (52, 38, 24, 255))
        # Arms and a half-melted, featureless head.
        c.rect(9, 24 + bob, 2, 12, WAX_D)
        c.rect(22, 24 + bob, 2, 12, WAX_D)
        c.ellipse(16, 15 + bob, 5, 6, WAX)
        c.ellipse(15, 13 + bob, 3, 3, WAX_H)
        # Wax runnels down the face.
        c.rect(14, 17 + bob, 1, 4, WAX_D)
        c.rect(18, 16 + bob, 1, 5, WAX_D)
        c.px(16, 44, WAX_D)                  # a drip

    return draw


def seam_shrike():
    BODY = (92, 96, 104, 255)
    BODY_D = (60, 64, 72, 255)
    CRYST = (214, 228, 224, 255)
    CRYST_H = (244, 250, 248, 255)
    BEAK = (216, 200, 140, 255)

    def draw(c: Canvas, row: int, col: int):
        flap = [0, -3, -5, -3][col] if row == 1 else IDLE_BOB[col]
        c.shadow(16, 45, 4)
        # Perched/flying bird body.
        c.ellipse(16, 30 + flap, 5, 6, BODY)
        c.ellipse(15, 27 + flap, 3, 3, BODY_D)
        # Crystalline flight feathers.
        for i in range(4):
            c.rect(16 - 6 - i, 28 + flap + i, 3, 1, CRYST)
            c.rect(16 + 4 + i, 28 + flap + i, 3, 1, CRYST)
        c.px(9, 28 + flap, CRYST_H)
        c.px(23, 28 + flap, CRYST_H)
        # Head, hooked beak, bronze-chip eye.
        c.ellipse(16, 22 + flap, 3, 3, BODY)
        c.rect(18, 22 + flap, 3, 1, BEAK)
        c.px(20, 23 + flap, BEAK)
        c.px(17, 21 + flap, (222, 186, 88, 255))
        # Legs when perched.
        if row == 0:
            c.rect(15, 36 + flap, 1, 6, BEAK)
            c.rect(18, 36 + flap, 1, 6, BEAK)

    return draw


def hollow_spawn():
    EARTH = (58, 52, 48, 255)
    EARTH_D = (38, 34, 32, 255)
    EARTH_H = (84, 74, 66, 255)
    ROOT = (96, 82, 58, 255)
    BELLGLOW = (110, 170, 220, 255)

    def draw(c: Canvas, row: int, col: int):
        bob = IDLE_BOB[col] if row == 0 else 0
        stride = STRIDE[col] if row == 1 else 0
        c.shadow(16, 45, 7)
        c.rect(11, 36 + max(stride, 0), 5, 8, EARTH_D)
        c.rect(17, 36 + max(-stride, 0), 5, 8, EARTH_D)
        # Hunched torso.
        c.rect(10, 24 + bob, 13, 14, EARTH)
        c.rect(10, 24 + bob, 13, 2, EARTH_H)
        c.rect(21, 26 + bob, 2, 11, EARTH_D)
        # Roots through the hide, and the bell-glow in the cracks.
        for i in range(5):
            c.px(12 + i * 2, 28 + bob + (i % 3), ROOT)
        c.rect(14, 30 + bob, 4, 1, BELLGLOW)
        c.rect(15, 33 + bob, 2, 1, BELLGLOW)
        # Enormous mole-claws.
        c.rect(7, 28 + bob, 3, 8, EARTH_D)
        c.rect(23, 28 + bob, 3, 8, EARTH_D)
        for i in range(3):
            c.rect(6 - i // 2, 35 + bob + i, 3, 1, (206, 198, 172, 255))
            c.rect(24 + i // 2, 35 + bob + i, 3, 1, (206, 198, 172, 255))
        # Eyeless head, blunt snout.
        c.ellipse(16, 18 + bob, 5, 5, EARTH)
        c.ellipse(16, 21 + bob, 3, 2, EARTH_H)
        c.rect(12, 17 + bob, 8, 1, EARTH_D)

    return draw


# ── bosses (64x80) ────────────────────────────────────────────────────────────
def cinderjack():
    WAX = (232, 196, 72, 255)
    WAX_D = (172, 140, 42, 255)
    WAX_H = (250, 232, 152, 255)
    FLAME = (252, 168, 56, 255)
    FLAME_H = (254, 236, 168, 255)
    IRON = (108, 104, 100, 255)
    IRON_H = (162, 158, 152, 255)

    def draw(c: Canvas, row: int, col: int):
        slump = IDLE_BOB[col] if row == 0 else STRIDE[col] // 2
        c.shadow(32, 76, 18)
        # A cart-sized mound, wider at the base.
        c.ellipse(32, 64 + slump, 22, 12, WAX)
        c.ellipse(32, 52 + slump, 18, 12, WAX)
        c.ellipse(26, 46 + slump, 10, 7, WAX_H)
        # Running wax at the hem.
        for i, x in enumerate((14, 20, 28, 38, 46, 51)):
            c.rect(x, 72 + (i % 3), 2, 4, WAX_D)
        # Dripping arms.
        c.rect(8, 46 + slump, 6, 20, WAX)
        c.rect(50, 46 + slump, 6, 20, WAX)
        c.rect(8, 64 + slump, 6, 4, WAX_D)
        c.rect(50, 64 + slump, 6, 4, WAX_D)
        # The grinning face melted into the front.
        c.rect(22, 44 + slump, 5, 4, (72, 52, 12, 255))
        c.rect(37, 44 + slump, 5, 4, (72, 52, 12, 255))
        for i in range(11):
            c.px(26 + i, 54 + slump + (0 if 2 < i < 8 else -1), (72, 52, 12, 255))
        c.rect(28, 55 + slump, 8, 1, (72, 52, 12, 255))
        # THE CROWN: the bell's iron clapper, worn tilted.
        c.rect(20, 30 + slump, 24, 5, IRON)
        c.rect(20, 30 + slump, 24, 1, IRON_H)
        c.ellipse(42, 36 + slump, 6, 6, IRON)
        c.ellipse(40, 34 + slump, 3, 3, IRON_H)
        # Ring of small flames.
        for i, x in enumerate((12, 24, 40, 52)):
            f = (col + i) % 2
            c.rect(x, 66 + f, 2, 4, FLAME)
            c.px(x, 65 + f, FLAME_H)

    return draw


def knell():
    SHARD = (206, 226, 236, 255)
    SHARD_H = (244, 252, 254, 255)
    SHARD_D = (146, 176, 196, 255)
    GLOW = (96, 156, 200, 160)

    def draw(c: Canvas, row: int, col: int):
        # The whole cluster rotates rather than walks.
        spin = col if row == 0 else col + 2
        c.shadow(32, 76, 10)
        # Cold light in the gaps.
        c.ellipse(32, 40, 20, 24, GLOW)
        # Shards on two counter-rotating rings, no body at the centre.
        ring = [
            (0, -20), (14, -14), (20, 0), (14, 14),
            (0, 20), (-14, 14), (-20, 0), (-14, -14),
        ]
        for i, (dx, dy) in enumerate(ring):
            k = (i + spin) % 8
            sx, sy = 32 + dx, 40 + dy
            size = 3 + (k % 3)
            c.ellipse(sx, sy, size, size + 1, SHARD)
            c.px(sx, sy - size, SHARD_H)
            c.px(sx + 1, sy + 1, SHARD_D)
        inner = [(0, -10), (9, 5), (-9, 5)]
        for i, (dx, dy) in enumerate(inner):
            k = (i + spin) % 3
            c.ellipse(32 + dx, 40 + dy, 2 + k % 2, 3, SHARD_H)
        # A single large shard, always suspended dead centre.
        c.ellipse(32, 40, 3, 6, SHARD)
        c.px(32, 34, SHARD_H)

    return draw


def deeplistener():
    HIDE = (48, 44, 44, 255)
    HIDE_D = (30, 28, 30, 255)
    HIDE_H = (72, 66, 62, 255)
    ROOT = (92, 80, 56, 255)
    CLAW = (200, 194, 172, 255)
    SCAR = (110, 176, 226, 255)

    def draw(c: Canvas, row: int, col: int):
        heave = IDLE_BOB[col] if row == 0 else STRIDE[col] // 2
        c.shadow(32, 77, 26)
        # It fills the frame: a vast mole-back.
        c.ellipse(32, 56 + heave, 30, 20, HIDE)
        c.ellipse(26, 44 + heave, 20, 12, HIDE_H)
        c.ellipse(32, 68, 30, 10, HIDE_D)
        # Blunt questing snout, no eyes anywhere.
        c.ellipse(32, 30 + heave, 13, 10, HIDE)
        c.ellipse(32, 25 + heave, 8, 5, HIDE_H)
        c.rect(28, 22 + heave, 8, 2, (156, 120, 116, 255))
        c.px(30, 23 + heave, HIDE_D)
        c.px(34, 23 + heave, HIDE_D)
        # Roots in the hide.
        for i in range(9):
            c.px(12 + i * 5, 50 + heave + (i % 4), ROOT)
        # Old bell-shaped scars, glowing faintly.
        for i, (sx, sy) in enumerate(((16, 58), (32, 62), (48, 58))):
            c.ellipse(sx, sy + heave, 4, 5, (110, 176, 226, 90))
            c.rect(sx - 3, sy + 4 + heave, 7, 1, SCAR)
        # Digging claws.
        for side in (0, 1):
            bx = 6 if side == 0 else 50
            c.rect(bx, 60 + heave, 8, 10, HIDE_D)
            for i in range(3):
                c.rect(bx - 2 + i * 3, 70 + heave, 2, 6, CLAW)

    return draw


def main() -> None:
    print("hollow_bell character sheets:")
    for obj_id, fn in (
        ("marten_coalbright", marten()),
        ("sister_wick", wick()),
        ("tobin_ashfoot", tobin()),
        ("hettie_marl", hettie()),
        ("grandam_bellow", grandam()),
        ("tallow_drip", tallow_drip()),
        ("sump_crawler", sump_crawler()),
        ("wax_wight", wax_wight()),
        ("seam_shrike", seam_shrike()),
        ("hollow_spawn", hollow_spawn()),
    ):
        build(obj_id, 32, 48, fn)

    for obj_id, fn in (
        ("cinderjack", cinderjack()),
        ("knell", knell()),
        ("deeplistener", deeplistener()),
    ):
        build(obj_id, 64, 80, fn)


if __name__ == "__main__":
    main()
