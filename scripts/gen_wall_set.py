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

import os

from PIL import Image

from wall_perspective import (
    TILE_PX,
    SLAB_N,
    SLAB_S,
    SLAB_E,
    SLAB_W,
    BG,
    arm_corners_3d,
    arm_depth,
    canvas_for_content,
    draw_stone_arm,
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

# Slab base positions live in wall_perspective.py (shared with the door
# generator); aliased so the SPECS below keep their original spelling.
_S = SLAB_S
_E = SLAB_E
_N = SLAB_N
_W = SLAB_W

SPECS = [
    # ── Four directional walls. Each spans the full tile width along its
    # parallel axis and sits inset from the perpendicular outer edge.
    {
        "id": "wall_n",
        "name": "North Wall",
        "description": "Horizontal wall slab on the north edge of its tile (interior is to the south).",
        "arms": [{"axis": "y", "pos": _N, "t0": 0.0, "t1": 1.0}],
        # Floor is kept only on the interior (south) side of the slab.
        "floor_mask": [0.0, 0.0, 1.0, _N],
        "hide_facing": "south",
    },
    {
        "id": "wall_s",
        "name": "South Wall",
        "description": "Horizontal wall slab on the south edge of its tile (interior is to the north).",
        "arms": [{"axis": "y", "pos": _S, "t0": 0.0, "t1": 1.0}],
        # Floor is kept only on the interior (north) side of the slab.
        "floor_mask": [0.0, _S, 1.0, 1.0],
        "hide_facing": "south",
    },
    {
        "id": "wall_e",
        "name": "East Wall",
        "description": "Vertical wall slab on the east edge of its tile (interior is to the west).",
        "arms": [{"axis": "x", "pos": _E, "t0": 0.0, "t1": 1.0}],
        # Floor is kept only on the interior (west) side of the slab.
        "floor_mask": [0.0, 0.0, _E, 1.0],
        "hide_facing": "east",
    },
    {
        "id": "wall_w",
        "name": "West Wall",
        "description": "Vertical wall slab on the west edge of its tile (interior is to the east).",
        "arms": [{"axis": "x", "pos": _W, "t0": 0.0, "t1": 1.0}],
        # Floor is kept only on the interior (east) side of the slab.
        "floor_mask": [_W, 0.0, 1.0, 1.0],
        "hide_facing": "east",
    },
    # ── Four corners (world coords). Each is stamped at the building tile
    # named in its id; its two arms reach back toward the adjacent
    # directional walls so the slabs touch flush at (pos_x, pos_y).
    {
        "id": "wall_corner_ne",
        "wall_corner": "ne",
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
        # Interior is south-west: floor kept on fy<_N and fx<_E.
        "floor_mask": [0.0, 0.0, _E, _N],
    },
    {
        "id": "wall_corner_nw",
        "wall_corner": "nw",
        "name": "Wall Corner NW",
        "description": "North-west building corner; north arm reaches east, west arm reaches south.",
        "arms": [
            {"axis": "y", "pos": _N, "t0": _W, "t1": 1.0},
            {"axis": "x", "pos": _W, "t0": 0.0, "t1": _N},
        ],
        # Interior is south-east: floor kept on fy<_N and fx>_W.
        "floor_mask": [_W, 0.0, 1.0, _N],
    },
    {
        "id": "wall_corner_se",
        "wall_corner": "se",
        "name": "Wall Corner SE",
        "description": "South-east building corner; south arm reaches west, east arm reaches north.",
        "arms": [
            {"axis": "y", "pos": _S, "t0": 0.0, "t1": _E},
            {"axis": "x", "pos": _E, "t0": _S, "t1": 1.0},
        ],
        # Interior is north-west: floor kept on fy>_S and fx<_E.
        "floor_mask": [0.0, _S, _E, 1.0],
    },
    {
        "id": "wall_corner_sw",
        "wall_corner": "sw",
        "name": "Wall Corner SW",
        "description": "South-west building corner; south arm reaches east, west arm reaches north.",
        "arms": [
            {"axis": "y", "pos": _S, "t0": _W, "t1": 1.0},
            {"axis": "x", "pos": _W, "t0": _S, "t1": 1.0},
        ],
        # Interior is north-east: floor kept on fy>_S and fx>_W.
        "floor_mask": [_W, _S, 1.0, 1.0],
    },
]


# ── Drawing ──────────────────────────────────────────────────────────────
# Geometry (arm_corners_3d / canvas_for_content) and the hewn-stone face
# renderer live in wall_perspective.py, shared with gen_door_set.py.
def build_sprite(spec):
    # Collect all 3D corners across every arm so canvas fits the union.
    all_corners = []
    for arm in spec["arms"]:
        all_corners.extend(arm_corners_3d(arm))
    cw, ch, anchor = canvas_for_content(all_corners)
    img = Image.new("RGBA", (cw, ch), BG)
    # Painter's algorithm: farther arms first (higher mean fy).
    for arm in sorted(spec["arms"], key=lambda a: -arm_depth(a)):
        draw_stone_arm(img, arm, anchor)
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
  block_size: 2
  walkable_surface: true
{mask_line}{hide_line}{corner_line}  stack_order: 50
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
    mask_line = ""
    if spec.get("floor_mask"):
        m = spec["floor_mask"]
        nums = ", ".join(f"{round(v, 3)}" for v in m)
        mask_line = f"  floor_mask_rect: [{nums}]\n"
    # Corners carry `wall_corner:` (drives the renderer's fade/tint choice);
    # it used to be hand-added post-generation and got clobbered on
    # regeneration — emit it here so that can't happen again.
    corner_line = (
        f"  wall_corner: {spec['wall_corner']}\n" if spec.get("wall_corner") else ""
    )
    content = META_TEMPLATE.format(
        name=spec["name"],
        description=spec["description"],
        sprite_path=sprite_path,
        w_tiles=_fmt_tiles(cw),
        h_tiles=_fmt_tiles(ch),
        mask_line=mask_line,
        hide_line=hide_line,
        corner_line=corner_line,
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
