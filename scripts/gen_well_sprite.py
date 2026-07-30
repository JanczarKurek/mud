"""
Generates assets/overworld_objects/well/sprite.png

Oblique-projected stone well per docs/sprite_style.md, built on the shared
projection in wall_perspective.py: a sandstone ring (swept cylinder, lit
top annulus, dark water in the hole), two wooden posts carrying a crossbar
windlass, and a rope-hung bucket. Footprint is centred on the tile
(0.5, 0.5); the canvas is sized tight with canvas_for_content and the
bottom-center pixel anchors to the tile's south edge. Deterministic output.

Run from the repo root:

    python3 scripts/gen_well_sprite.py
"""

import math
import os

from PIL import Image

from wall_perspective import (
    TILE_PX,
    BG,
    STONE,
    STONE_HI,
    STONE_DARK,
    STONE_VDARK,
    MORTAR,
    CAP_TOP,
    CAP_MID,
    CAP_DARK,
    WOOD,
    WOOD_HI,
    WOOD_DARK,
    DARK_VOID,
    project,
    canvas_for_content,
    fill_polygon,
    shade_polygon,
    _line,
)

OUT_PATH = "assets/overworld_objects/well/sprite.png"

ROPE     = (180, 140,  60, 255)
WATER_HI = ( 80, 130, 210, 255)

# ── Geometry (3D floor coords, tile-centred) ─────────────────────────────
CX, CY = 0.5, 0.5          # footprint centre
R_OUT = 0.36               # ring outer radius (tiles)
R_IN = 0.22                # ring inner radius (hole)
RING_H = 0.32              # ring top height (floors)
POST_HW = 0.05             # post half-width
POST_X_W, POST_X_E = 0.16, 0.84   # bases tucked behind the drum's flanks
POST_Y = 0.62              # north of centre → drum occludes the bases
POST_H = 0.74
BAR_Z0, BAR_Z1 = 0.68, 0.78
BUCKET_Z0, BUCKET_Z1 = 0.30, 0.40

CONTENT_CORNERS = [
    (x, y, z)
    for x in (POST_X_W - POST_HW, POST_X_E + POST_HW)
    for y in (CY - R_OUT - 0.04, POST_Y + 0.03)
    for z in (0.0, BAR_Z1)
]

CW, CH, ANCHOR = canvas_for_content(CONTENT_CORNERS)
img = Image.new("RGBA", (CW, CH), BG)

# Screen direction the shear leaves visible (down-right = opposite of the
# up-left height shear); used to pick lit vs shadowed arcs.
_shear = project(0, 0, 1.0, (0, 0))
_norm = math.hypot(_shear[0], _shear[1])
VIS_DIR = (-_shear[0] / _norm, -_shear[1] / _norm)


def lerp_color(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(4))


def disc(center, r, color):
    x0, y0 = int(center[0] - r) - 1, int(center[1] - r) - 1
    x1, y1 = int(center[0] + r) + 1, int(center[1] + r) + 1
    for y in range(max(y0, 0), min(y1, img.height - 1) + 1):
        for x in range(max(x0, 0), min(x1, img.width - 1) + 1):
            if (x - center[0]) ** 2 + (y - center[1]) ** 2 <= r * r:
                img.putpixel((x, y), color)


def box(x0, x1, y0, y1, z0, z1, c_front, c_side, c_top):
    """Axis-aligned 3D box: lit top, south front, shadowed east side."""
    top = [project(x0, y0, z1, ANCHOR), project(x1, y0, z1, ANCHOR),
           project(x1, y1, z1, ANCHOR), project(x0, y1, z1, ANCHOR)]
    east = [project(x1, y0, z0, ANCHOR), project(x1, y1, z0, ANCHOR),
            project(x1, y1, z1, ANCHOR), project(x1, y0, z1, ANCHOR)]
    front = [project(x0, y0, z0, ANCHOR), project(x1, y0, z0, ANCHOR),
             project(x1, y0, z1, ANCHOR), project(x0, y0, z1, ANCHOR)]
    fill_polygon(img, top, c_top)
    fill_polygon(img, east, c_side)
    fill_polygon(img, front, c_front)


# ── Posts + crossbar (drawn first — north of centre, behind the drum) ────
for px_c in (POST_X_W, POST_X_E):
    box(px_c - POST_HW, px_c + POST_HW, POST_Y - POST_HW, POST_Y + POST_HW,
        0.0, POST_H, WOOD, WOOD_DARK, WOOD_HI)
box(POST_X_W - POST_HW, POST_X_E + POST_HW, POST_Y - 0.03, POST_Y + 0.03,
    BAR_Z0, BAR_Z1, WOOD, WOOD_DARK, WOOD_HI)

# ── Stone ring: swept cylinder side ──────────────────────────────────────
STEPS = 26
r_out_px = R_OUT * TILE_PX
r_in_px = R_IN * TILE_PX
for i in range(STEPS + 1):
    t = i / STEPS
    center = project(CX, CY, RING_H * t, ANCHOR)
    disc(center, r_out_px, lerp_color(STONE_VDARK, STONE, 0.3 + 0.7 * t))

top_center = project(CX, CY, RING_H, ANCHOR)
base_center = project(CX, CY, 0.0, ANCHOR)


def _arc_outline(center, r, color, min_dot):
    """Outline the circle arc whose outward direction faces the viewer."""
    steps = int(2 * math.pi * r * 2)
    for i in range(steps):
        th = 2 * math.pi * i / steps
        d = (math.cos(th), math.sin(th))
        if d[0] * VIS_DIR[0] + d[1] * VIS_DIR[1] < min_dot:
            continue
        x = int(round(center[0] + d[0] * r))
        y = int(round(center[1] + d[1] * r))
        if 0 <= x < img.width and 0 <= y < img.height:
            img.putpixel((x, y), color)


# Two stone courses on the drum side: horizontal ring seams plus staggered
# vertical joints between them (visible down-right arc only).
COURSE_FZ = [0.0, RING_H / 3.0, 2.0 * RING_H / 3.0, RING_H]
for fz in COURSE_FZ[1:3]:
    _arc_outline(project(CX, CY, fz, ANCHOR), r_out_px - 0.5, MORTAR, 0.1)
for course in range(3):
    z0, z1 = COURSE_FZ[course], COURSE_FZ[course + 1]
    c0 = project(CX, CY, z0, ANCHOR)
    c1 = project(CX, CY, z1, ANCHOR)
    offset = 15 if course % 2 else 0
    for deg in range(-180 + offset, 180 + offset, 30):
        th = math.radians(deg)
        d = (math.cos(th), math.sin(th))
        if d[0] * VIS_DIR[0] + d[1] * VIS_DIR[1] < 0.25:
            continue
        _line(img,
              int(round(c0[0] + d[0] * (r_out_px - 0.5))),
              int(round(c0[1] + d[1] * (r_out_px - 0.5))),
              int(round(c1[0] + d[0] * (r_out_px - 0.5))),
              int(round(c1[1] + d[1] * (r_out_px - 0.5))),
              MORTAR)

# Ground the drum: dark outline along the base's visible arc.
_arc_outline(base_center, r_out_px - 0.5, STONE_VDARK, -0.05)

# ── Ring top: annulus cap, hole, water ───────────────────────────────────
x0, y0 = int(top_center[0] - r_out_px) - 1, int(top_center[1] - r_out_px) - 1
x1, y1 = int(top_center[0] + r_out_px) + 1, int(top_center[1] + r_out_px) + 1
for y in range(max(y0, 0), min(y1, img.height - 1) + 1):
    for x in range(max(x0, 0), min(x1, img.width - 1) + 1):
        dx, dy = x - top_center[0], y - top_center[1]
        dist = math.hypot(dx, dy)
        if dist > r_out_px:
            continue
        if dist < r_in_px:
            img.putpixel((x, y), DARK_VOID)
            continue
        d = (dx / dist, dy / dist) if dist > 0 else (0.0, 0.0)
        toward_light = -(d[0] * VIS_DIR[0] + d[1] * VIS_DIR[1])
        if dist >= r_out_px - 1.6 and toward_light > 0.2:
            img.putpixel((x, y), CAP_TOP)        # lit up-left outer rim
        elif dist <= r_in_px + 1.6 and toward_light < -0.2:
            img.putpixel((x, y), CAP_DARK)       # shadowed inner rim
        else:
            img.putpixel((x, y), CAP_MID)
# Radial coping joints every 45°.
for deg in range(0, 360, 45):
    th = math.radians(deg + 22)
    d = (math.cos(th), math.sin(th))
    ax = top_center[0] + d[0] * (r_in_px + 1)
    ay = top_center[1] + d[1] * (r_in_px + 1)
    bx = top_center[0] + d[0] * (r_out_px - 1)
    by = top_center[1] + d[1] * (r_out_px - 1)
    _line(img, int(round(ax)), int(round(ay)), int(round(bx)), int(round(by)), CAP_DARK)
# Water glints deep in the hole.
for gx, gy in ((0, 1), (1, 2), (-2, 2)):
    x, y = top_center[0] + gx, top_center[1] + 3 + gy
    if math.hypot(x - top_center[0], y - top_center[1]) < r_in_px - 1:
        img.putpixel((int(x), int(y)), WATER_HI)

# ── Rope + bucket (after the drum — they hang above the hole) ────────────
rope_top = project(CX, POST_Y, BAR_Z0, ANCHOR)
rope_bot = project(CX, 0.52, BUCKET_Z1, ANCHOR)
_line(img, rope_top[0], rope_top[1], rope_bot[0], rope_bot[1], ROPE)
box(CX - 0.08, CX + 0.08, 0.46, 0.58,
    BUCKET_Z0, BUCKET_Z1, WOOD, WOOD_DARK, WOOD_HI)

# ── Contact shadow around the south base of the drum ─────────────────────
shadow_quad = [
    project(CX - R_OUT, CY - R_OUT - 0.04, 0.0, ANCHOR),
    project(CX + R_OUT, CY - R_OUT - 0.04, 0.0, ANCHOR),
    project(CX + R_OUT, CY - R_OUT + 0.06, 0.0, ANCHOR),
    project(CX - R_OUT, CY - R_OUT + 0.06, 0.0, ANCHOR),
]
shade_polygon(img, shadow_quad, 0.75)

# Catch-light on the drum's south-west base edge.
_line(
    img,
    int(base_center[0] - r_out_px * 0.7), int(base_center[1] + r_out_px * 0.6),
    int(base_center[0] - r_out_px * 0.2), int(base_center[1] + r_out_px * 0.95),
    STONE_HI,
)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
img.save(OUT_PATH)
print(f"wrote {OUT_PATH} ({CW}x{CH}) -> sprite_width_tiles: {CW/48:.3f}, sprite_height_tiles: {CH/48:.3f}")
