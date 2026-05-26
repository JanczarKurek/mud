"""
Regenerate the full perspective-consistent wall set (4 directional walls +
4 corners) plus their metadata YAMLs. All geometry is derived from constants
in `wall_perspective.py`, which must mirror `FLOOR_SHIFT_X_TILES` and
`FLOOR_SHIFT_Y_TILES` in `src/world/systems.rs`.

Run from the repo root:

    python3 scripts/gen_wall_set.py

Idempotent: running twice with the same constants produces byte-identical
PNGs and YAMLs.

Outputs (8 directories under assets/overworld_objects/):
    wall_n / wall_s / wall_e / wall_w
    wall_corner_ne / wall_corner_nw / wall_corner_se / wall_corner_sw

World-coordinate naming: Bevy +y = north. `wall_n` sits on the building's
NORTH edge with its slab pushed `WALL_INSET` tiles inward (south) from the
tile's north edge. `wall_corner_ne` sits at the building's NORTH-EAST corner
tile and its two arms reach toward the adjacent `wall_n` (west neighbour)
and `wall_e` (south neighbour) so all three slabs share a flush meeting
point at (fx = 1 - INSET, fy = 1 - INSET).
"""

import math
import os

from PIL import Image

from wall_perspective import (
    TILE_PX,
    WALL_HEIGHT_FLOORS,
    WALL_INSET,
    BG,
    STONE,
    STONE_HI,
    STONE_DARK,
    STONE_VDARK,
    MORTAR,
    CAP_HI,
    CAP_MID,
    CAP_DARK,
    project,
    fill_polygon,
    _line,
)

ASSETS_DIR = "assets/overworld_objects"


# ── Sprite specs ─────────────────────────────────────────────────────────
#
# Each spec lists "arms" — zero-thickness vertical wall slabs. An arm is a
# 2D rectangle in 3D space, either:
#   axis="y": at constant fy (horizontal wall); fx ∈ [t0, t1], fz ∈ [0, H]
#   axis="x": at constant fx (vertical wall);   fy ∈ [t0, t1], fz ∈ [0, H]
# Both axes use the same wall height (`WALL_HEIGHT_FLOORS`) so adjacent
# walls and corners align in z.
#
# Directional walls inset their slab from the outer tile edge by `WALL_INSET`
# so the visible architecture sits inside the tile rather than flush with
# the grid line — gives the player room to stand near the wall on either
# side without overlapping the sprite.
#
# Corners are L-shapes whose two half-tile arms meet at the inset position
# and reach back to the adjacent directional wall slabs.

_N = 1.0 - WALL_INSET   # slab position for north-side walls (high fy)
_S = WALL_INSET         # slab position for south-side walls (low fy)
_E = 1.0 - WALL_INSET   # slab position for east-side walls (high fx)
_W = WALL_INSET         # slab position for west-side walls (low fx)

SPECS = [
    # ── Four directional walls. Each spans the full tile width along its
    # parallel axis and sits inset from the perpendicular outer edge.
    {
        "id": "wall_n",
        "name": "North Wall",
        "description": "Horizontal wall slab on the north edge of its tile (interior is to the south).",
        "arms": [{"axis": "y", "pos": _N, "t0": 0.0, "t1": 1.0}],
        "hide_facing": "south",
    },
    {
        "id": "wall_s",
        "name": "South Wall",
        "description": "Horizontal wall slab on the south edge of its tile (interior is to the north).",
        "arms": [{"axis": "y", "pos": _S, "t0": 0.0, "t1": 1.0}],
        "hide_facing": "south",
    },
    {
        "id": "wall_e",
        "name": "East Wall",
        "description": "Vertical wall slab on the east edge of its tile (interior is to the west).",
        "arms": [{"axis": "x", "pos": _E, "t0": 0.0, "t1": 1.0}],
        "hide_facing": "east",
    },
    {
        "id": "wall_w",
        "name": "West Wall",
        "description": "Vertical wall slab on the west edge of its tile (interior is to the east).",
        "arms": [{"axis": "x", "pos": _W, "t0": 0.0, "t1": 1.0}],
        "hide_facing": "east",
    },
    # ── Four corners (world coords). Each is stamped at the building tile
    # named in its id; its two arms reach back toward the adjacent
    # directional walls so the slabs touch flush at (pos_x, pos_y).
    {
        "id": "wall_corner_ne",
        "name": "Wall Corner NE",
        "description": "North-east building corner; north arm reaches west, east arm reaches south.",
        "arms": [
            # North arm: lives on wall_n's pos line, reaches from the west
            # tile edge inward to the meeting point.
            {"axis": "y", "pos": _N, "t0": 0.0, "t1": _E},
            # East arm: lives on wall_e's pos line, reaches from the south
            # tile edge inward to the meeting point.
            {"axis": "x", "pos": _E, "t0": 0.0, "t1": _N},
        ],
    },
    {
        "id": "wall_corner_nw",
        "name": "Wall Corner NW",
        "description": "North-west building corner; north arm reaches east, west arm reaches south.",
        "arms": [
            {"axis": "y", "pos": _N, "t0": _W, "t1": 1.0},
            {"axis": "x", "pos": _W, "t0": 0.0, "t1": _N},
        ],
    },
    {
        "id": "wall_corner_se",
        "name": "Wall Corner SE",
        "description": "South-east building corner; south arm reaches west, east arm reaches north.",
        "arms": [
            {"axis": "y", "pos": _S, "t0": 0.0, "t1": _E},
            {"axis": "x", "pos": _E, "t0": _S, "t1": 1.0},
        ],
    },
    {
        "id": "wall_corner_sw",
        "name": "Wall Corner SW",
        "description": "South-west building corner; south arm reaches east, west arm reaches north.",
        "arms": [
            {"axis": "y", "pos": _S, "t0": _W, "t1": 1.0},
            {"axis": "x", "pos": _W, "t0": _S, "t1": 1.0},
        ],
    },
]


# ── Canvas sizing ────────────────────────────────────────────────────────
def arm_corners_3d(arm):
    """Return the four 3D corners (bottom-left, bottom-right, top-right, top-left)
    of a wall arm in winding order around the visible face."""
    pos = arm["pos"]
    t0, t1 = arm["t0"], arm["t1"]
    h = WALL_HEIGHT_FLOORS
    if arm["axis"] == "y":
        return [(t0, pos, 0.0), (t1, pos, 0.0), (t1, pos, h), (t0, pos, h)]
    else:
        return [(pos, t0, 0.0), (pos, t1, 0.0), (pos, t1, h), (pos, t0, h)]


def canvas_for_content(corners_3d):
    """Size a canvas to fit `corners_3d`.

    The renderer's bottom-anchor pins the canvas bottom-center pixel to the
    tile's SOUTH edge in world (see `anchor_y_offset = -tile_size * 0.5` in
    `src/world/systems.rs::sync_tile_transforms`). So our reference point is
    the tile-south-center (3D coords `(0.5, 0, 0)`), which must project to
    canvas (cw/2, ch-1). We size the canvas tight to the projected bbox of
    `corners_3d` so the sprite does NOT extend into neighbour tiles — this
    avoids cross-tile alpha occlusion where a tall sprite's transparent
    rows would otherwise hide a wall in the neighbour tile.

    Returns (cw, ch, anchor_px) where `anchor_px` is the PIL pixel of the
    3D origin (0, 0, 0).
    """
    tile_south_raw = project(0.5, 0.0, 0.0, (0, 0))
    offs = []
    for (x, y, z) in corners_3d:
        p = project(x, y, z, (0, 0))
        offs.append((p[0] - tile_south_raw[0], p[1] - tile_south_raw[1]))

    dxs = [o[0] for o in offs]
    dys = [o[1] for o in offs]
    # Width must center the tile-south-center at canvas (cw/2, ch-1) AND fit
    # all content offsets. Take the symmetric envelope.
    cw_left = -2 * min(dxs) if min(dxs) < 0 else 0
    cw_right = 2 * max(dxs) + 1 if max(dxs) >= 0 else 0
    cw = max(int(cw_left), int(cw_right), TILE_PX)
    # Height must reach from canvas bottom up far enough to fit the top of
    # the wall body. Don't round up — keep canvas tight so the sprite stops
    # at the wall's top edge (avoids occluding the neighbour tile above).
    above = -min(dys) if min(dys) < 0 else 0
    ch = max(int(above) + 1, 1)

    anchor_x = cw // 2 - TILE_PX // 2
    anchor_y = ch - 1
    return cw, ch, (anchor_x, anchor_y)


# ── Drawing ──────────────────────────────────────────────────────────────
def _lerp_pt(a, b, t):
    return (round(a[0] + (b[0] - a[0]) * t),
            round(a[1] + (b[1] - a[1]) * t))


def draw_arm(img, arm, anchor):
    """Draw one wall slab: stone body, lit top cap band, edge highlights/shadows."""
    pts3d = arm_corners_3d(arm)
    bl, br, tr, tl = [project(x, y, z, anchor) for (x, y, z) in pts3d]

    # Stone body
    fill_polygon(img, [bl, br, tr, tl], STONE)

    # Top "cap" band — top 18% of face in CAP_HI, next 6% in CAP_MID as a
    # soft shadow line under the lit cap. Fakes a thin lit top surface
    # without adding a real 3D thickness.
    cap_top_frac = 0.18
    cap_shadow_frac = 0.24
    cap_bl = _lerp_pt(tl, bl, cap_top_frac)
    cap_br = _lerp_pt(tr, br, cap_top_frac)
    sh_bl = _lerp_pt(tl, bl, cap_shadow_frac)
    sh_br = _lerp_pt(tr, br, cap_shadow_frac)
    fill_polygon(img, [cap_bl, cap_br, tr, tl], CAP_HI)
    fill_polygon(img, [sh_bl, sh_br, cap_br, cap_bl], CAP_MID)

    # Mortar courses — two horizontal-ish bands across the face below the cap.
    for course_frac in (0.55, 0.80):
        c_bl = _lerp_pt(tl, bl, course_frac)
        c_br = _lerp_pt(tr, br, course_frac)
        c2_bl = _lerp_pt(tl, bl, course_frac + 0.025)
        c2_br = _lerp_pt(tr, br, course_frac + 0.025)
        fill_polygon(img, [c2_bl, c2_br, c_br, c_bl], MORTAR)

    # Edge outlines: bottom seam shadowed, left slanted edge lit, right shadowed.
    _line(img, bl[0], bl[1], br[0], br[1], STONE_VDARK)
    _line(img, bl[0], bl[1], tl[0], tl[1], STONE_HI)
    _line(img, br[0], br[1], tr[0], tr[1], STONE_DARK)
    # Top edge: along (tl, tr) — already covered by CAP_HI fill, no extra stroke.


def _arm_depth(arm):
    """Mean fy of the arm — higher fy is farther from the camera (camera is
    up-left of the player, lower fy = south = closer = drawn later)."""
    if arm["axis"] == "y":
        return arm["pos"]
    return (arm["t0"] + arm["t1"]) / 2.0


def build_sprite(spec):
    # Collect all 3D corners across every arm so canvas fits the union.
    all_corners = []
    for arm in spec["arms"]:
        all_corners.extend(arm_corners_3d(arm))
    cw, ch, anchor = canvas_for_content(all_corners)
    img = Image.new("RGBA", (cw, ch), BG)
    # Painter's algorithm: farther arms first (higher mean fy).
    for arm in sorted(spec["arms"], key=lambda a: -_arm_depth(a)):
        draw_arm(img, arm, anchor)
    return img, cw, ch


# ── Metadata YAML emission ───────────────────────────────────────────────
META_TEMPLATE = """extends: obstacle
name: {name}
description: {description}
render:
  z_index: 0.3
  debug_color: [120, 114, 103]
  debug_size: 1.0
  sprite_path: {sprite_path}
  sprite_width_tiles: {w_tiles}
  sprite_height_tiles: {h_tiles}
  occludes_floor_above: true
  display_height: {display_height}
{hide_line}  stack_order: 50
"""


def _fmt_tiles(px_value):
    """Format a tile count as either an integer-valued float (`2.0`) or a
    decimal with up to 3 dp, trimming trailing zeros while keeping `.0`."""
    v = px_value / TILE_PX
    if v == int(v):
        return f"{int(v)}.0"
    return f"{v:.3f}".rstrip("0").rstrip(".")


def write_metadata(spec, cw, ch):
    dir_path = os.path.join(ASSETS_DIR, spec["id"])
    path = os.path.join(dir_path, "metadata.yaml")
    sprite_path = f"overworld_objects/{spec['id']}/sprite.png"
    hide_line = (
        f"  hide_when_inside_facing: {spec['hide_facing']}\n"
        if spec.get("hide_facing")
        else ""
    )
    content = META_TEMPLATE.format(
        name=spec["name"],
        description=spec["description"],
        sprite_path=sprite_path,
        w_tiles=_fmt_tiles(cw),
        h_tiles=_fmt_tiles(ch),
        display_height=_fmt_tiles(WALL_HEIGHT_FLOORS * TILE_PX),
        hide_line=hide_line,
    )
    with open(path, "w") as f:
        f.write(content)
    print(f"  metadata: {path}")


def main():
    for spec in SPECS:
        dir_path = os.path.join(ASSETS_DIR, spec["id"])
        os.makedirs(dir_path, exist_ok=True)
        img, cw, ch = build_sprite(spec)
        sprite_path = os.path.join(dir_path, "sprite.png")
        img.save(sprite_path)
        print(f"Saved {sprite_path}  ({cw}×{ch})")
        write_metadata(spec, cw, ch)


if __name__ == "__main__":
    main()
