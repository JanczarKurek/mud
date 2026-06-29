"""
Generates static vegetation sprites for the overworld:

  oak_tree        96×144   (2 × 3 tiles)
  pine_tree       48×144   (1 × 3 tiles)
  bush            48×64    (1 × 1.33 tiles)
  flowering_bush  48×64
  berry_bush      48×64
  hedge           48×48    (1 × 1 tile, tiles edge-to-edge)

Each is written to assets/overworld_objects/<id>/sprite.png.
Pixel density is 48 px = 1 tile (matches gen_campfire_sheet.py conventions).
Style: blocky, 2-3 shading levels, no anti-aliasing, transparent background,
no pure-black outlines (darkened base colour instead).
"""

from PIL import Image
import os

BG = (0, 0, 0, 0)

# ── Foliage palette ──────────────────────────────────────────────────────────
LEAF_SHADOW = (20, 56, 30, 255)
LEAF_DK = (30, 84, 42, 255)
LEAF = (46, 120, 56, 255)
LEAF_MD = (62, 146, 72, 255)
LEAF_HI = (104, 182, 100, 255)

PINE_SHADOW = (18, 58, 44, 255)
PINE_DK = (26, 78, 56, 255)
PINE = (36, 100, 68, 255)
PINE_HI = (74, 142, 98, 255)

# ── Bark / wood palette ──────────────────────────────────────────────────────
BARK_DK = (58, 38, 22, 255)
BARK = (92, 62, 36, 255)
BARK_HI = (122, 88, 54, 255)

# ── Accent palettes ──────────────────────────────────────────────────────────
FLOWER_PINK = (230, 138, 176, 255)
FLOWER_WHT = (238, 234, 224, 255)
FLOWER_YEL = (242, 210, 92, 255)
FLOWER_CTR = (236, 188, 70, 255)

BERRY = (188, 44, 44, 255)
BERRY_HI = (226, 98, 84, 255)
BERRY_DK = (140, 28, 30, 255)

SHADOW = (18, 38, 24, 95)   # soft ground shadow (semi-transparent)


def make_canvas(w, h):
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

    def disc(cx, cy, r, c):
        ellipse(cx, cy, r, r, c)

    return img, px, rect, ellipse, disc


def save(img, object_id):
    save_as(img, object_id, "sprite.png")


def save_as(img, object_id, filename):
    out = f"assets/overworld_objects/{object_id}/{filename}"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img.save(out)
    print(f"Saved {out}  ({img.width}×{img.height})")


# ── Oak tree ─────────────────────────────────────────────────────────────────
def gen_oak_tree():
    img, px, rect, ellipse, disc = make_canvas(96, 144)

    # Ground shadow
    ellipse(48, 139, 26, 6, SHADOW)

    # Trunk (bottom-centre, rises into the canopy)
    rect(40, 96, 16, 44, BARK_DK)
    rect(42, 96, 11, 44, BARK)
    rect(43, 96, 3, 44, BARK_HI)
    # Root flare
    rect(36, 134, 6, 6, BARK_DK)
    rect(54, 134, 6, 6, BARK_DK)

    # Canopy — stacked discs, dark to light, biased up-left for a light source
    disc(48, 58, 42, LEAF_SHADOW)
    disc(48, 54, 40, LEAF_DK)
    disc(44, 50, 33, LEAF)
    disc(40, 44, 24, LEAF_MD)
    disc(36, 38, 13, LEAF_HI)
    # Clumps for a bumpy silhouette
    for (cx, cy, r) in [(20, 60, 12), (74, 58, 13), (66, 78, 11), (28, 80, 10), (50, 84, 12)]:
        disc(cx, cy, r, LEAF_DK)
    for (cx, cy, r) in [(22, 56, 7), (72, 54, 8), (60, 40, 9)]:
        disc(cx, cy, r, LEAF_MD)

    save(img, "oak_tree")


# ── Pine tree ────────────────────────────────────────────────────────────────
def gen_pine_tree():
    img, px, rect, ellipse, disc = make_canvas(48, 144)

    ellipse(24, 139, 15, 5, SHADOW)

    # Trunk
    rect(20, 118, 8, 22, BARK_DK)
    rect(22, 118, 4, 22, BARK)

    def tier(y_top, y_bot, half_top, half_bot, base, hi):
        for y in range(y_top, y_bot):
            t = (y - y_top) / max(1, (y_bot - y_top))
            half = half_top + (half_bot - half_top) * t
            rect(int(24 - half), y, int(half * 2), 1, base)
        # left-edge highlight, darker right shoulder
        for y in range(y_top + 2, y_bot, 2):
            t = (y - y_top) / max(1, (y_bot - y_top))
            half = half_top + (half_bot - half_top) * t
            px(int(24 - half) + 1, y, hi)
            px(int(24 + half) - 1, y, PINE_SHADOW)

    # Four overlapping tiers, widest at the base
    tier(8, 46, 2, 13, PINE, PINE_HI)
    tier(36, 80, 4, 17, PINE_DK, PINE)
    tier(70, 110, 6, 21, PINE, PINE_HI)
    tier(100, 124, 8, 22, PINE_DK, PINE)
    # Bright tip
    rect(23, 8, 2, 4, PINE_HI)

    save(img, "pine_tree")


# ── Bush family ──────────────────────────────────────────────────────────────
def bush_mound(img, px, rect, ellipse, disc, base=LEAF, dark=LEAF_DK, mid=LEAF_MD, hi=LEAF_HI):
    ellipse(24, 60, 18, 5, SHADOW)
    disc(24, 44, 18, dark)
    disc(16, 42, 12, base)
    disc(32, 43, 12, base)
    disc(24, 38, 14, base)
    disc(20, 35, 9, mid)
    disc(30, 36, 8, mid)
    disc(19, 31, 5, hi)
    # bumpy lower edge
    for cx in (10, 24, 38):
        disc(cx, 50, 6, dark)


def gen_bush():
    img, px, rect, ellipse, disc = make_canvas(48, 64)
    bush_mound(img, px, rect, ellipse, disc)
    save(img, "bush")


def gen_flowering_bush():
    img, px, rect, ellipse, disc = make_canvas(48, 64)
    bush_mound(img, px, rect, ellipse, disc)
    flowers = [
        (14, 34, FLOWER_PINK), (21, 30, FLOWER_WHT), (29, 33, FLOWER_YEL),
        (34, 40, FLOWER_PINK), (12, 44, FLOWER_WHT), (24, 42, FLOWER_YEL),
        (33, 46, FLOWER_PINK), (18, 47, FLOWER_WHT),
    ]
    for (x, y, c) in flowers:
        # 5px flower: petals + centre
        px(x, y - 1, c); px(x - 1, y, c); px(x + 1, y, c); px(x, y + 1, c)
        px(x, y, FLOWER_CTR)
    save(img, "flowering_bush")


def gen_berry_bush():
    # The bush has two visual states driven by its `states:` block:
    #   sprite.png  → "full"  (mound + berries)
    #   empty.png   → "empty" (the same mound, picked clean)
    berry_base, berry_mid = (40, 108, 50, 255), (54, 132, 64, 255)
    berries = [(15, 36), (22, 33), (30, 37), (35, 43), (13, 46), (25, 45), (33, 48), (19, 49)]

    # ── Full ──
    full, px, rect, ellipse, disc = make_canvas(48, 64)
    bush_mound(full, px, rect, ellipse, disc, base=berry_base, mid=berry_mid)
    for (x, y) in berries:
        px(x, y, BERRY); px(x + 1, y, BERRY); px(x, y + 1, BERRY); px(x + 1, y + 1, BERRY)
        px(x, y, BERRY_HI)
    save(full, "berry_bush")

    # ── Empty: identical foliage, no berries ──
    empty, px, rect, ellipse, disc = make_canvas(48, 64)
    bush_mound(empty, px, rect, ellipse, disc, base=berry_base, mid=berry_mid)
    save_as(empty, "berry_bush", "empty.png")


# ── Berries (item icon) ───────────────────────────────────────────────────────
def gen_berries():
    # Small 32×32 inventory/ground icon: a clustered handful of red berries on a
    # short leafy sprig. Scaled down to ~0.4 tile by `debug_size` at render time.
    img, px, rect, ellipse, disc = make_canvas(32, 32)
    ellipse(16, 28, 9, 3, SHADOW)
    # Leafy sprig behind the cluster
    disc(20, 9, 4, LEAF_DK)
    disc(22, 8, 3, LEAF_MD)
    rect(15, 10, 2, 9, LEAF_DK)            # stem
    # Berry cluster — dark rim, body, single highlight pip each
    cluster = [(11, 16), (18, 14), (15, 20), (22, 19), (10, 22), (17, 25)]
    for (x, y) in cluster:
        disc(x, y, 4, BERRY_DK)
        disc(x, y, 3, BERRY)
        px(x - 1, y - 1, BERRY_HI); px(x, y - 1, BERRY_HI)
    save(img, "berries")


# ── Hedge (tileable) ─────────────────────────────────────────────────────────
def gen_hedge():
    img, px, rect, ellipse, disc = make_canvas(48, 48)
    # Solid leafy block spanning the full width so segments butt seamlessly.
    rect(0, 6, 48, 42, LEAF)
    rect(0, 6, 48, 6, LEAF_MD)       # lit top band
    rect(0, 6, 48, 2, LEAF_HI)       # bright crown
    rect(0, 42, 48, 6, LEAF_DK)      # shaded base
    # Leaf texture — a staggered dot grid using darks and lights
    for y in range(10, 44, 4):
        for x in range((y // 4) % 2 * 2, 48, 4):
            px(x, y, LEAF_DK)
            px(x + 1, y + 1, LEAF_HI)
    save(img, "hedge")


if __name__ == "__main__":
    gen_oak_tree()
    gen_pine_tree()
    gen_bush()
    gen_flowering_bush()
    gen_berry_bush()
    gen_berries()
    gen_hedge()
