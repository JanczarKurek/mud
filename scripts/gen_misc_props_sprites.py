"""
Regenerates the legacy 16×16 hand-made prop sprites as upright-elevation
pixel art per docs/sprite_style.md (bottom-anchored, visible height, lit
tops, shadowed east sides). Deterministic output.

  crystal_cluster   48×64  (1.0 × 1.333 tiles)  colliding obstacle
  stone             32×26  (0.667 × 0.542)      movable (pushable) rock
  portal_arch       64×96  (1.333 × 2.0)        standing stone arch, glowing
  cave_mushroom     24×28  (0.5 × 0.583)        small glowing fungus

Run from the repo root:

    python3 scripts/gen_misc_props_sprites.py
"""

from PIL import Image
import os


BG = (0, 0, 0, 0)


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

    def tri(p0, p1, p2, c):
        ys = sorted([p0, p1, p2], key=lambda p: p[1])
        y_min, y_max = int(ys[0][1]), int(ys[2][1])
        for y in range(y_min, y_max + 1):
            xs = []
            for (a, b) in ((p0, p1), (p1, p2), (p2, p0)):
                if a[1] == b[1]:
                    continue
                if (a[1] <= y < b[1]) or (b[1] <= y < a[1]):
                    t = (y - a[1]) / (b[1] - a[1])
                    xs.append(a[0] + t * (b[0] - a[0]))
            if len(xs) >= 2:
                for x in range(int(round(min(xs))), int(round(max(xs))) + 1):
                    px(x, y, c)

    return img, px, rect, ellipse, tri


def save(img, object_id):
    out = f"assets/overworld_objects/{object_id}/sprite.png"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img.save(out)
    print(f"wrote {out} ({img.width}x{img.height}) -> "
          f"sprite_width_tiles: {img.width/48:.3f}, sprite_height_tiles: {img.height/48:.3f}")


# ── Crystal cluster ──────────────────────────────────────────────────────
CRYS_DK   = ( 16,  86,  96, 255)
CRYS_MID  = ( 54, 160, 172, 255)
CRYS_LIT  = (110, 220, 228, 255)
CRYS_GLNT = (200, 250, 252, 255)
ROCK      = ( 96,  92,  88, 255)
ROCK_DK   = ( 64,  60,  58, 255)
ROCK_HI   = (128, 124, 118, 255)
SHADOW    = (  0,   0,   0,  70)


def gen_crystal_cluster():
    img, px, rect, ellipse, tri = make_canvas(48, 64)
    ellipse(24, 58, 16, 4, SHADOW)
    # Rocky base mound
    ellipse(24, 55, 15, 6, ROCK_DK)
    ellipse(23, 54, 13, 5, ROCK)
    ellipse(19, 52, 6, 3, ROCK_HI)
    # Shards: (tip, base_left, base_right, lean) — west facet lit, east dark.
    shards = [
        (( 12, 22), ( 6, 54), (18, 54)),   # left, medium
        (( 37, 18), (30, 54), (43, 54)),   # right, medium
        (( 23,  4), (14, 56), (33, 56)),   # centre, tall (drawn over)
        (( 30, 34), (25, 57), (36, 57)),   # small front-right
    ]
    for (tip, bl, br) in shards:
        mid = ((bl[0] + br[0]) // 2, bl[1])
        tri(tip, bl, mid, CRYS_MID)        # west facet
        tri(tip, mid, br, CRYS_DK)         # east facet (shadow)
        # facet ridge + glint near the tip
        for i in range(3):
            px(tip[0], tip[1] + i, CRYS_LIT)
        px(tip[0] - 1, tip[1] + 3, CRYS_LIT)
        px(tip[0], tip[1] + 1, CRYS_GLNT)
    # Lit rim on the centre shard's west edge
    for t in range(0, 44, 2):
        x = 23 + (14 - 23) * t // 52
        px(x, 4 + t, CRYS_LIT)
    save(img, "crystal_cluster")


# ── Stone (pushable boulder) ─────────────────────────────────────────────
STONE_C  = (118, 118, 124, 255)
STONE_DK = ( 82,  82,  90, 255)
STONE_VD = ( 58,  58,  66, 255)
STONE_HI = (152, 152, 158, 255)


def gen_stone():
    img, px, rect, ellipse, tri = make_canvas(32, 26)
    ellipse(16, 22, 12, 3, SHADOW)
    # Boulder body: squashed dome with a flat-ish base
    ellipse(16, 13, 13, 9, STONE_VD)          # dark silhouette
    ellipse(15, 12, 12, 8, STONE_C)           # body
    ellipse(11, 9, 6, 4, STONE_HI)            # lit top-west patch
    # East + lower shading
    ellipse(22, 15, 5, 4, STONE_DK)
    rect(6, 18, 20, 3, STONE_DK)
    # Cracks (deterministic)
    for (cx, cy) in ((14, 13), (15, 14), (16, 15), (21, 10), (22, 11)):
        px(cx, cy, STONE_VD)
    save(img, "stone")


# ── Portal arch ──────────────────────────────────────────────────────────
ARCH      = (168, 140, 105, 255)   # sandstone, matches the wall palette
ARCH_HI   = (198, 172, 134, 255)
ARCH_DK   = (122,  98,  70, 255)
ARCH_VD   = ( 74,  58,  42, 255)
GLOW_DK   = ( 34,  96, 118, 255)
GLOW      = ( 66, 160, 180, 255)
GLOW_LT   = (109, 196, 205, 255)
GLOW_HI   = (188, 240, 244, 255)


def gen_portal_arch():
    img, px, rect, ellipse, tri = make_canvas(64, 96)
    ellipse(32, 92, 24, 4, SHADOW)

    # Portal glow inside the opening (drawn first, pillars/arch overlap it):
    # vertical bands, brightest in the middle.
    for (x0, w, c) in ((20, 24, GLOW_DK), (23, 18, GLOW), (27, 10, GLOW_LT)):
        rect(x0, 26, w, 64, c)
    # Shimmer sparks (deterministic)
    for (sx, sy) in ((26, 38), (35, 46), (30, 58), (38, 66), (25, 72), (33, 82)):
        px(sx, sy, GLOW_HI)
        px(sx, sy - 1, GLOW_LT)

    # Pillars: front face + east-side shadow + plinth
    for x0 in (6, 44):
        rect(x0, 20, 14, 68, ARCH)
        rect(x0 + 11, 20, 3, 68, ARCH_DK)     # east shadow side
        rect(x0, 20, 2, 68, ARCH_HI)          # west catch-light
        # Block joints
        for y in range(32, 88, 12):
            rect(x0, y, 14, 1, ARCH_VD)
        # Plinth
        rect(x0 - 2, 86, 18, 6, ARCH_DK)
        rect(x0 - 2, 86, 18, 1, ARCH)

    # Arch span: stepped curve over the top
    rect(6, 12, 52, 10, ARCH)
    rect(10, 6, 44, 8, ARCH)
    rect(18, 2, 28, 6, ARCH)
    rect(18, 2, 28, 1, ARCH_HI)               # lit crown
    rect(10, 6, 44, 1, ARCH_HI)
    rect(6, 12, 4, 1, ARCH_HI)
    rect(6, 20, 52, 2, ARCH_VD)               # underside shadow
    # Keystone
    rect(28, 2, 8, 12, ARCH_HI)
    rect(34, 2, 2, 12, ARCH_DK)
    # Arch east-end shading
    rect(54, 12, 4, 10, ARCH_DK)
    rect(50, 6, 4, 8, ARCH_DK)
    rect(43, 2, 3, 6, ARCH_DK)

    save(img, "portal_arch")


# ── Cave mushroom ────────────────────────────────────────────────────────
CAP      = (205, 170, 112, 255)
CAP_HI   = (232, 205, 150, 255)
CAP_DK   = (150, 115,  70, 255)
STEM     = (222, 205, 170, 255)
STEM_DK  = (176, 158, 122, 255)
SPORE    = (255, 235, 180, 140)


def gen_cave_mushroom():
    img, px, rect, ellipse, tri = make_canvas(24, 28)
    ellipse(12, 25, 8, 2, SHADOW)
    # Stem
    rect(10, 14, 4, 12, STEM)
    rect(13, 14, 1, 12, STEM_DK)
    # Cap dome
    ellipse(12, 10, 10, 7, CAP_DK)            # rim silhouette
    ellipse(12, 9, 9, 6, CAP)
    ellipse(9, 7, 5, 3, CAP_HI)               # lit top-west
    rect(4, 13, 17, 2, CAP_DK)                # under-rim gill shadow
    # Spots
    for (sx, sy) in ((8, 10), (14, 6), (17, 10)):
        px(sx, sy, CAP_HI)
        px(sx + 1, sy, CAP_HI)
    # Faint spore glow around the cap (semi-transparent)
    for (gx, gy) in ((2, 8), (22, 7), (12, 1), (5, 3), (19, 2)):
        px(gx, gy, SPORE)
    save(img, "cave_mushroom")


if __name__ == "__main__":
    gen_crystal_cluster()
    gen_stone()
    gen_portal_arch()
    gen_cave_mushroom()
