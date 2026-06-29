"""
Generates indoor furniture art.

Directional pieces (4-frame sheets, columns = S, E, N, W) -> sheet.png:
  chair       cell 48×64   (sheet 192×64)
  bed         cell 96×96   (sheet 384×96)
  bookshelf   cell 48×96   (sheet 192×96)
  cabinet     cell 48×96   (sheet 192×96)

Flat pieces (single, engine auto-rotates via rotation_by_facing) -> sprite.png:
  table       96×96   side_table 48×48   bench 96×96   stool 48×48   rug 96×96

Pixel density 48 px = 1 tile. Directional frames render at raw frame px (so the
cell size IS the tile footprint); flat sprites are scaled by sprite_*_tiles in
metadata, but are authored here at 48 px/tile for matching density.
Columns are ordered S, E, N, W to match the idle_s/idle_e/idle_n/idle_w clips.
"""

from PIL import Image
from PIL.Image import Transpose
import os

BG = (0, 0, 0, 0)

# ── Wood palette ─────────────────────────────────────────────────────────────
WOOD_DARK = (70, 44, 26, 255)     # legs / deep shadow
WOOD_DK = (98, 66, 38, 255)
WOOD = (140, 98, 56, 255)
WOOD_HI = (172, 128, 80, 255)
WOOD_GRAIN = (120, 84, 48, 255)

METAL = (206, 202, 186, 255)      # handles / fittings
METAL_DK = (122, 118, 104, 255)

# ── Bed fabrics ──────────────────────────────────────────────────────────────
MATTRESS = (226, 216, 190, 255)
MATTRESS_SH = (198, 186, 158, 255)
PILLOW = (242, 240, 232, 255)
PILLOW_SH = (212, 208, 196, 255)
BLANKET = (152, 60, 60, 255)
BLANKET_HI = (184, 88, 86, 255)
BLANKET_DK = (116, 44, 46, 255)

# ── Book spines ──────────────────────────────────────────────────────────────
BOOKS = [
    (152, 60, 54, 255), (70, 96, 142, 255), (86, 130, 74, 255),
    (178, 142, 62, 255), (120, 72, 128, 255), (58, 122, 122, 255),
    (182, 98, 54, 255), (92, 104, 120, 255),
]

# ── Rug ──────────────────────────────────────────────────────────────────────
RUG_BORDER = (140, 52, 50, 255)
RUG_FIELD = (196, 158, 110, 255)
RUG_FIELD_DK = (170, 132, 88, 255)
RUG_LINE = (96, 64, 60, 255)
RUG_MOTIF = (94, 120, 130, 255)
FRINGE = (224, 210, 180, 255)


def canvas(w, h):
    img = Image.new("RGBA", (w, h), BG)

    def px(x, y, c):
        if 0 <= x < w and 0 <= y < h:
            img.putpixel((int(x), int(y)), c)

    def rect(x, y, rw, rh, c):
        for dy in range(int(rh)):
            for dx in range(int(rw)):
                px(x + dx, y + dy, c)

    def ellipse(cx, cy, rx, ry, c):
        for dy in range(-int(ry), int(ry) + 1):
            for dx in range(-int(rx), int(rx) + 1):
                if (dx / rx) ** 2 + (dy / ry) ** 2 <= 1.0:
                    px(cx + dx, cy + dy, c)

    return img, px, rect, ellipse


def panel(rect, x, y, w, h, base=WOOD, dk=WOOD_DK, hi=WOOD_HI):
    rect(x, y, w, h, base)
    rect(x, y, w, 1, hi)
    rect(x, y, 1, h, hi)
    rect(x, y + h - 1, w, 1, dk)
    rect(x + w - 1, y, 1, h, dk)


def save_sheet(frames, object_id):
    fw, fh = frames[0].size
    sheet = Image.new("RGBA", (fw * len(frames), fh), BG)
    for i, f in enumerate(frames):
        sheet.paste(f, (i * fw, 0))
    out = f"assets/overworld_objects/{object_id}/sheet.png"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    sheet.save(out)
    print(f"Saved {out}  ({sheet.width}×{sheet.height})")


def save_sprite(img, object_id):
    out = f"assets/overworld_objects/{object_id}/sprite.png"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img.save(out)
    print(f"Saved {out}  ({img.width}×{img.height})")


def mirror(img):
    return img.transpose(Transpose.FLIP_LEFT_RIGHT)


# ══ CHAIR (48×64) ════════════════════════════════════════════════════════════
# Faces the way the sitter looks; the backrest sits on the OPPOSITE edge.
def chair(d):
    if d == "w":
        return mirror(chair("e"))
    img, px, rect, ellipse = canvas(48, 64)

    def legs(front_only=False):
        rect(14, 46, 3, 9, WOOD_DARK)
        rect(31, 46, 3, 9, WOOD_DARK)
        if not front_only:
            rect(17, 45, 2, 7, WOOD_DK)
            rect(29, 45, 2, 7, WOOD_DK)

    def seat():
        panel(rect, 13, 34, 22, 12)
        rect(13, 44, 22, 2, WOOD_DK)        # front lip

    def backrest_panel(x, y, w, h):
        panel(rect, x, y, w, h)
        # vertical slats
        rect(x + w // 3, y + 2, 1, h - 4, WOOD_DK)
        rect(x + 2 * w // 3, y + 2, 1, h - 4, WOOD_DK)

    if d == "s":            # backrest at far (top) edge
        backrest_panel(14, 18, 20, 17)
        legs()
        seat()
    elif d == "n":          # backrest toward viewer (front/bottom)
        legs()
        seat()
        backrest_panel(14, 41, 20, 16)
    elif d == "e":          # backrest on left edge, sitter faces right
        backrest_panel(11, 22, 6, 26)
        legs()
        seat()
        rect(33, 36, 3, 8, WOOD_HI)         # hint of seat front toward east
    return img


# ══ BED (96×96) ══════════════════════════════════════════════════════════════
# Headboard sits on the edge OPPOSITE the facing direction; pillow beside it.
def bed(d):
    if d == "w":
        return mirror(bed("e"))
    img, px, rect, ellipse = canvas(96, 96)

    def vertical(head_top):
        # frame
        panel(rect, 26, 8, 44, 84, WOOD, WOOD_DK, WOOD_HI)
        rect(29, 11, 38, 78, MATTRESS)
        rect(29, 11, 38, 78, MATTRESS) if False else None
        rect(64, 11, 3, 78, MATTRESS_SH)            # right shading
        if head_top:
            panel(rect, 24, 4, 48, 12, WOOD, WOOD_DARK, WOOD_HI)   # headboard
            rect(33, 16, 30, 14, PILLOW); rect(33, 27, 30, 3, PILLOW_SH)
            rect(30, 34, 36, 54, BLANKET)
            rect(30, 34, 36, 3, BLANKET_HI)
            rect(30, 60, 36, 2, BLANKET_DK)         # fold
        else:
            panel(rect, 24, 84, 48, 12, WOOD, WOOD_DARK, WOOD_HI)  # headboard bottom
            rect(33, 70, 30, 14, PILLOW); rect(33, 70, 30, 3, PILLOW_SH)
            rect(30, 12, 36, 54, BLANKET)
            rect(30, 63, 36, 3, BLANKET_DK)
            rect(30, 38, 36, 2, BLANKET_DK)         # fold

    def horizontal(head_left):
        panel(rect, 6, 36, 84, 48, WOOD, WOOD_DK, WOOD_HI)
        rect(9, 39, 78, 42, MATTRESS)
        rect(9, 78, 78, 3, MATTRESS_SH)
        if head_left:
            panel(rect, 4, 32, 12, 56, WOOD, WOOD_DARK, WOOD_HI)
            rect(18, 44, 14, 32, PILLOW); rect(18, 44, 3, 32, PILLOW_SH)
            rect(36, 40, 50, 40, BLANKET)
            rect(36, 40, 3, 40, BLANKET_HI)
            rect(60, 40, 2, 40, BLANKET_DK)
        # head_right handled by mirror

    if d == "s":
        vertical(head_top=True)
    elif d == "n":
        vertical(head_top=False)
    elif d == "e":
        horizontal(head_left=True)
    return img


# ══ BOOKSHELF (48×96) ════════════════════════════════════════════════════════
def bookshelf(d):
    if d == "w":
        return mirror(bookshelf("e"))
    img, px, rect, ellipse = canvas(48, 96)

    if d == "s":            # open front, full of books
        panel(rect, 5, 8, 38, 84, WOOD, WOOD_DARK, WOOD_HI)
        rect(8, 11, 32, 78, WOOD_DK)            # recessed interior
        shelf_ys = [14, 33, 52, 71]
        for sy in shelf_ys:
            rect(8, sy + 17, 32, 2, WOOD)       # shelf board
            x = 9
            bi = sy
            while x < 39:
                bw = 2 + ((x + sy) % 3)
                bh = 13 + ((x * 7 + sy) % 4)
                col = BOOKS[(x + sy) % len(BOOKS)]
                rect(x, sy + 17 - bh, bw, bh, col)
                rect(x, sy + 17 - bh, bw, 1, tuple(min(255, c + 28) for c in col[:3]) + (255,))
                x += bw + 1
    elif d == "n":          # plain wooden back
        panel(rect, 5, 8, 38, 84, WOOD, WOOD_DARK, WOOD_HI)
        for px_x in range(10, 40, 9):
            rect(px_x, 11, 1, 78, WOOD_GRAIN)
        rect(8, 30, 32, 2, WOOD_DK)             # cross braces
        rect(8, 64, 32, 2, WOOD_DK)
    elif d == "e":          # thin side, shelf edges + sliver of books on front edge
        panel(rect, 17, 8, 14, 84, WOOD, WOOD_DARK, WOOD_HI)
        for sy in (31, 50, 69, 88):
            rect(17, sy, 14, 1, WOOD_DK)
        # book sliver peeking at the front (east) edge
        for sy in (16, 35, 54, 73):
            rect(29, sy, 2, 16, BOOKS[sy % len(BOOKS)])
    return img


# ══ CABINET / wardrobe (48×96) ═══════════════════════════════════════════════
def cabinet(d):
    if d == "w":
        return mirror(cabinet("e"))
    img, px, rect, ellipse = canvas(48, 96)

    if d == "s":            # two doors + handles
        panel(rect, 6, 10, 36, 82, WOOD, WOOD_DARK, WOOD_HI)
        rect(6, 6, 36, 5, WOOD_HI)              # cornice
        rect(6, 88, 36, 4, WOOD_DK)            # base
        rect(23, 12, 2, 78, WOOD_DARK)        # door split
        for dx in (9, 26):                     # door panels
            panel(rect, dx, 16, 13, 64, WOOD, WOOD_DK, WOOD_HI)
            rect(dx + 2, 18, 9, 60, WOOD_GRAIN)
        rect(20, 44, 2, 8, METAL); rect(26, 44, 2, 8, METAL)   # handles
    elif d == "n":          # plain back
        panel(rect, 6, 8, 36, 84, WOOD, WOOD_DARK, WOOD_HI)
        for px_x in range(11, 40, 8):
            rect(px_x, 11, 1, 78, WOOD_GRAIN)
    elif d == "e":          # slim side panel
        panel(rect, 16, 8, 16, 84, WOOD, WOOD_DARK, WOOD_HI)
        rect(19, 14, 10, 72, WOOD_GRAIN)
        rect(18, 12, 12, 2, WOOD_HI)
    return img


# ══ Flat (auto-rotated) pieces ═══════════════════════════════════════════════
def table():
    img, px, rect, ellipse = canvas(96, 96)
    # legs first (corners), then top over them
    for (lx, ly) in [(12, 58), (78, 58), (16, 34), (74, 34)]:
        rect(lx, ly, 6, 18, WOOD_DARK)
        rect(lx, ly, 2, 18, WOOD_DK)
    panel(rect, 8, 34, 80, 28, WOOD, WOOD_DK, WOOD_HI)
    for gy in range(38, 60, 4):                # grain
        rect(11, gy, 74, 1, WOOD_GRAIN)
    rect(8, 60, 80, 2, WOOD_DARK)              # apron shadow
    save_sprite(img, "table")


def side_table():
    img, px, rect, ellipse = canvas(48, 48)
    for (lx, ly) in [(10, 34), (34, 34), (12, 22), (32, 22)]:
        rect(lx, ly, 4, 12, WOOD_DARK)
    panel(rect, 8, 12, 32, 22, WOOD, WOOD_DK, WOOD_HI)
    rect(11, 22, 26, 1, WOOD_DARK)            # drawer seam
    rect(22, 25, 4, 3, METAL)                 # knob
    save_sprite(img, "side_table")


def bench():
    img, px, rect, ellipse = canvas(96, 96)
    for lx in (12, 44, 78):
        rect(lx, 52, 6, 14, WOOD_DARK)
    panel(rect, 8, 40, 80, 13, WOOD, WOOD_DK, WOOD_HI)
    for gy in (43, 47):
        rect(11, gy, 74, 1, WOOD_GRAIN)
    rect(8, 51, 80, 2, WOOD_DARK)
    save_sprite(img, "bench")


def stool():
    img, px, rect, ellipse = canvas(48, 48)
    # three splayed legs
    for (x0, x1) in [(14, 11), (24, 24), (34, 37)]:
        for t in range(14):
            rect(x1 + (x0 - x1) * (14 - t) // 14, 22 + t, 2, 1, WOOD_DARK)
    ellipse(24, 20, 14, 9, WOOD_DK)
    ellipse(24, 19, 13, 8, WOOD)
    ellipse(24, 17, 9, 5, WOOD_HI)
    save_sprite(img, "stool")


def rug():
    img, px, rect, ellipse = canvas(96, 96)
    x0, y0, w, h = 6, 18, 84, 60
    rect(x0, y0, w, h, RUG_BORDER)                       # border base
    rect(x0 + 4, y0 + 4, w - 8, h - 8, RUG_FIELD)        # field
    # inner accent line
    for (lx, ly, lw, lh) in [(x0 + 8, y0 + 8, w - 16, 1), (x0 + 8, y0 + h - 9, w - 16, 1),
                             (x0 + 8, y0 + 8, 1, h - 16), (x0 + w - 9, y0 + 8, 1, h - 16)]:
        rect(lx, ly, lw, lh, RUG_LINE)
    # woven shading stripes
    for sy in range(y0 + 12, y0 + h - 12, 6):
        rect(x0 + 10, sy, w - 20, 1, RUG_FIELD_DK)
    # central diamond motif
    cx, cy = x0 + w // 2, y0 + h // 2
    for i in range(10):
        rect(cx - i, cy - (10 - i), 2 * i + 1, 1, RUG_MOTIF if i % 2 else RUG_LINE)
        rect(cx - i, cy + (10 - i), 2 * i + 1, 1, RUG_MOTIF if i % 2 else RUG_LINE)
    # fringe on the short (top & bottom) ends
    for fx in range(x0, x0 + w, 4):
        rect(fx, y0 - 3, 2, 3, FRINGE)
        rect(fx, y0 + h, 2, 3, FRINGE)
    save_sprite(img, "rug")


def directional(builder, object_id):
    order = ["s", "e", "n", "w"]
    frames = [builder(d) for d in order]
    save_sheet(frames, object_id)
    save_sprite(frames[0], object_id)   # south frame = static fallback (asset viewer / debug)


if __name__ == "__main__":
    directional(chair, "chair")
    directional(bed, "bed")
    directional(bookshelf, "bookshelf")
    directional(cabinet, "cabinet")
    table()
    side_table()
    bench()
    stool()
    rug()
