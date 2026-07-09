"""
Generate the dual-grid floor tilesets for the crafted interior floors:

    assets/floors/flagstone/tileset.png          (3 variants, 64x192)
    assets/floors/checkered_marble/tileset.png   (2 variants, 64x128)

Atlas contract (mirrors src/world/floor_render.rs — keep in sync):
  - 64px wide = 4 columns of 16px tiles; one variant block = 4 rows (16 tiles);
    variant blocks stack vertically.
  - A tile depicts a 4-corner occupancy mask; the tile for mask `m` sits at
    row-major index MASK_TO_AUTHORING_INDEX[m] within its block.
  - Mask bit -> opaque 8x8 quadrant (PIL coords): 1 -> bottom-left,
    2 -> bottom-right, 4 -> top-left, 8 -> top-right. Unset quadrants are
    fully transparent (hard edges — these are crafted interior floors; the
    hand-drawn terrain tilesets feather instead).
  - Variants are hash-picked PER 16px render cell, so every variant must
    keep structural joints at the same positions (here: x/y in {0, 8}) and
    identical base shading; only interior detail may differ.

The metadata.yaml files are hand-authored (weights stay tunable without
regenerating art).

Run from the repo root:

    python3 scripts/gen_floor_tilesets.py

Idempotent: running twice produces byte-identical PNGs.
"""

import hashlib
import os

from PIL import Image

# Mirror of src/world/floor_render.rs::MASK_TO_AUTHORING_INDEX.
MASK_TO_AUTHORING_INDEX = [12, 0, 13, 3, 15, 11, 4, 2, 8, 14, 1, 5, 9, 7, 10, 6]
# Mask bit -> top-left pixel of the 8x8 quadrant it keeps opaque.
BIT_TO_QUAD = {1: (0, 8), 2: (8, 8), 4: (0, 0), 8: (8, 0)}
T = 16

FLOORS_DIR = "assets/floors"

# Flagstone: the wall sandstone (wall_perspective.STONE) pulled toward warm
# gray so slabs sit naturally beside the hewn walls.
FLAG_BASE   = (150, 134, 110, 255)
FLAG_HI     = (172, 156, 132, 255)
FLAG_DARK   = (120, 104,  84, 255)
FLAG_MORTAR = ( 92,  78,  62, 255)

MARBLE_LIGHT      = (232, 228, 220, 255)
MARBLE_LIGHT_VEIN = (210, 204, 194, 255)
MARBLE_LIGHT_SPEC = (246, 243, 238, 255)
MARBLE_DARK       = ( 66,  64,  70, 255)
MARBLE_DARK_VEIN  = ( 98,  96, 104, 255)


def hash_bytes(key):
    return hashlib.md5(key.encode()).digest()


# ── Flagstone ────────────────────────────────────────────────────────────
def flagstone_interior(variant):
    """One 16x16 slab cell. Variant 0: full slab; 1: split at y=8;
    2: split at x=8. Mortar always at x=0 / y=0 so joints align across
    hash-picked cells (render cells are 16px apart)."""
    img = Image.new("RGBA", (T, T), FLAG_BASE)
    p = img.load()

    def mortar_row(y, x0=0, x1=T):
        for x in range(x0, x1):
            p[x, y] = FLAG_MORTAR

    def mortar_col(x, y0=0, y1=T):
        for y in range(y0, y1):
            p[x, y] = FLAG_MORTAR

    def bevel(x0, y0, x1, y1):
        """Lit top/left inner edge, dark bottom/right inner edge of a slab
        spanning [x0, x1) x [y0, y1)."""
        for x in range(x0, x1):
            p[x, y0] = FLAG_HI
            p[x, y1 - 1] = FLAG_DARK
        for y in range(y0, y1):
            p[x0, y] = FLAG_HI
            p[x1 - 1, y] = FLAG_DARK

    mortar_row(0)
    mortar_col(0)
    slabs = {
        0: [(1, 1, T, T)],
        1: [(1, 1, T, 8), (1, 9, T, T)],
        2: [(1, 1, 8, T), (9, 1, T, T)],
    }[variant]
    if variant == 1:
        mortar_row(8, 1, T)
    if variant == 2:
        mortar_col(8, 1, T)
    for si, (x0, y0, x1, y1) in enumerate(slabs):
        bevel(x0, y0, x1, y1)
        # 2-3 hash-placed pits/cracks per slab, inset from the slab edges.
        digest = hash_bytes(f"flag:{variant}:{si}")
        n = 2 + digest[0] % 2
        for k in range(n):
            dx = digest[1 + 2 * k] % max(x1 - x0 - 4, 1)
            dy = digest[2 + 2 * k] % max(y1 - y0 - 4, 1)
            px_, py_ = x0 + 2 + dx, y0 + 2 + dy
            p[px_, py_] = FLAG_DARK
            if digest[3 + 2 * k] % 2 and px_ + 1 < x1 - 1:
                p[px_ + 1, py_] = FLAG_DARK
        # A little tonal patching inside the slab (kept subtle, inset 1px).
        for k in range(3):
            dx = digest[8 + 2 * k] % max(x1 - x0 - 2, 1)
            dy = digest[9 + 2 * k] % max(y1 - y0 - 2, 1)
            p[x0 + 1 + dx, y0 + 1 + dy] = FLAG_HI if k % 2 else (
                158, 142, 118, 255)
    return img


# ── Checkered marble ─────────────────────────────────────────────────────
def marble_interior(variant):
    """8px checker, fixed phase: light at (0,0) and (8,8). One render-cell
    offset (half a world tile = 24 screen px = 12 atlas px... period is 8px,
    and cells repeat every 16px = exactly two periods) keeps the pattern
    globally aligned. Variants differ only in vein layout."""
    img = Image.new("RGBA", (T, T), MARBLE_LIGHT)
    p = img.load()
    for qy in range(2):
        for qx in range(2):
            light = (qx + qy) % 2 == 0
            base = MARBLE_LIGHT if light else MARBLE_DARK
            vein = MARBLE_LIGHT_VEIN if light else MARBLE_DARK_VEIN
            x0, y0 = qx * 8, qy * 8
            for y in range(8):
                for x in range(8):
                    p[x0 + x, y0 + y] = base
            # Hash-keyed diagonal vein, inset 1px so cells never bleed.
            digest = hash_bytes(f"marble:{variant}:{qx}:{qy}")
            vx = 1 + digest[0] % 4
            vy = 1 + digest[1] % 4
            length = 3 + digest[2] % 3
            down = digest[3] % 2 == 0
            for k in range(length):
                x = vx + k
                y = vy + k if down else vy + (length - 1 - k)
                if 1 <= x <= 6 and 1 <= y <= 6:
                    p[x0 + x, y0 + y] = vein
            if light:
                p[x0 + 1, y0 + 1] = MARBLE_LIGHT_SPEC
    return img


# ── Atlas assembly ───────────────────────────────────────────────────────
def cut_tile(interior, mask):
    """Alpha-clear the quadrants whose bit is NOT in `mask`, then darken a
    1px rim on interior pixels bordering a cleared quadrant so the floor
    edge reads as a cut slab edge."""
    tile = interior.copy()
    p = tile.load()
    cleared = set()
    for bit, (qx, qy) in BIT_TO_QUAD.items():
        if mask & bit:
            continue
        for y in range(qy, qy + 8):
            for x in range(qx, qx + 8):
                p[x, y] = (0, 0, 0, 0)
                cleared.add((x, y))
    if not cleared or mask == 0:
        return tile
    for y in range(T):
        for x in range(T):
            if (x, y) in cleared:
                continue
            if any(
                (nx, ny) in cleared
                for (nx, ny) in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1))
            ):
                r, g, b, a = p[x, y]
                p[x, y] = (max(0, r - 22), max(0, g - 22), max(0, b - 22), a)
    return tile


def build_atlas(interior_fn, n_variants):
    atlas = Image.new("RGBA", (4 * T, 4 * T * n_variants), (0, 0, 0, 0))
    for v in range(n_variants):
        interior = interior_fn(v)
        for mask in range(16):
            row, col = divmod(MASK_TO_AUTHORING_INDEX[mask], 4)
            atlas.paste(cut_tile(interior, mask), (col * T, (4 * v + row) * T))
    return atlas


def main():
    for (floor_id, fn, n) in (
        ("flagstone", flagstone_interior, 3),
        ("checkered_marble", marble_interior, 2),
    ):
        dir_path = os.path.join(FLOORS_DIR, floor_id)
        os.makedirs(dir_path, exist_ok=True)
        atlas = build_atlas(fn, n)
        path = os.path.join(dir_path, "tileset.png")
        atlas.save(path)
        print(f"Saved {path}  ({atlas.width}×{atlas.height}, {n} variants)")


if __name__ == "__main__":
    main()
