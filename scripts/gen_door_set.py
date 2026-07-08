"""
Generate the four directional wall-slab doors (wooden_door_n / _s / _e / _w)
plus their metadata YAMLs. Each door is the SAME slab arm as its matching
wall (`gen_wall_set.py`) with a doorway cut into the masonry, so a door
drops into a wall run seamlessly: identical canvas, identical
`floor_mask_rect`, identical `hide_when_inside_facing`.

Run from the repo root:

    python3 scripts/gen_door_set.py

Idempotent: running twice produces byte-identical PNGs and YAMLs.

Outputs (4 directories under assets/overworld_objects/), two states each:
    closed.png  — planked leaf + iron straps + brass ring set in the opening
    open.png    — dark passable opening, leaf swung back flat against the
                  hinge jamb (the `locked` state reuses closed.png)

The legacy flat `wooden_door` object is unrelated to this script and stays
as-is (existing saves reference it); see scripts/gen_wooden_door_sprites.py.
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
    CAP_MID,
    CAP_DARK,
    MORTAR,
    STONE_VDARK,
    WOOD,
    WOOD_HI,
    WOOD_DARK,
    WOOD_GRAIN,
    IRON,
    IRON_HI,
    IRON_DARK,
    RING,
    RING_HI,
    DARK_VOID,
    THRESHOLD,
    THRESH_DARK,
    arm_corners_3d,
    canvas_for_content,
    draw_stone_arm,
    face_pt,
    fill_polygon,
    _line,
)

ASSETS_DIR = "assets/overworld_objects"

# Doorway cut, in face coords: u ∈ [DOOR_U0, DOOR_U1] (world tile units along
# the arm), v ∈ [0, DOOR_V_TOP] (slab-height fraction). The lintel band fills
# v ∈ [DOOR_V_TOP, 0.80] and the coping cap continues over it.
DOOR_U0 = 0.22
DOOR_U1 = 0.78
DOOR_V_TOP = 0.72
LINTEL_V_TOP = 0.80  # = wall_perspective.CAP_V0

DOOR_SPECS = [
    {
        "id": "wooden_door_n",
        "name": "Wooden Door (North)",
        "description": "Wooden door set into a north-edge wall slab (interior is to the south).",
        "arm": {"axis": "y", "pos": SLAB_N, "t0": 0.0, "t1": 1.0},
        "floor_mask": [0.0, 0.0, 1.0, SLAB_N],
        "hide_facing": "south",
    },
    {
        "id": "wooden_door_s",
        "name": "Wooden Door (South)",
        "description": "Wooden door set into a south-edge wall slab (interior is to the north).",
        "arm": {"axis": "y", "pos": SLAB_S, "t0": 0.0, "t1": 1.0},
        "floor_mask": [0.0, SLAB_S, 1.0, 1.0],
        "hide_facing": "south",
    },
    {
        "id": "wooden_door_e",
        "name": "Wooden Door (East)",
        "description": "Wooden door set into an east-edge wall slab (interior is to the west).",
        "arm": {"axis": "x", "pos": SLAB_E, "t0": 0.0, "t1": 1.0},
        "floor_mask": [0.0, 0.0, SLAB_E, 1.0],
        "hide_facing": "east",
    },
    {
        "id": "wooden_door_w",
        "name": "Wooden Door (West)",
        "description": "Wooden door set into a west-edge wall slab (interior is to the east).",
        "arm": {"axis": "x", "pos": SLAB_W, "t0": 0.0, "t1": 1.0},
        "floor_mask": [SLAB_W, 0.0, 1.0, 1.0],
        "hide_facing": "east",
    },
]


# ── Face-space drawing helpers ───────────────────────────────────────────
def face_rect(img, arm, anchor, u0, u1, v0, v1, color):
    quad = [
        face_pt(arm, u0, v0, anchor),
        face_pt(arm, u1, v0, anchor),
        face_pt(arm, u1, v1, anchor),
        face_pt(arm, u0, v1, anchor),
    ]
    fill_polygon(img, quad, color)


def face_line(img, arm, anchor, u0, v0, u1, v1, color):
    a = face_pt(arm, u0, v0, anchor)
    b = face_pt(arm, u1, v1, anchor)
    _line(img, a[0], a[1], b[0], b[1], color)


def draw_lintel(img, arm, anchor):
    """Single long lintel stone spanning the opening, under the coping cap."""
    u0, u1 = DOOR_U0 - 0.02, DOOR_U1 + 0.02
    face_rect(img, arm, anchor, u0, u1, DOOR_V_TOP, LINTEL_V_TOP, CAP_MID)
    # Underside shadow + end joints against the flanking masonry.
    face_line(img, arm, anchor, u0, DOOR_V_TOP, u1, DOOR_V_TOP, CAP_DARK)
    face_line(img, arm, anchor, u0, DOOR_V_TOP, u0, LINTEL_V_TOP, MORTAR)
    face_line(img, arm, anchor, u1, DOOR_V_TOP, u1, LINTEL_V_TOP, MORTAR)


def draw_jamb_edges(img, arm, anchor):
    """Dark inner edges where the masonry meets the opening."""
    face_line(img, arm, anchor, DOOR_U0, 0.0, DOOR_U0, DOOR_V_TOP, STONE_VDARK)
    face_line(img, arm, anchor, DOOR_U1, 0.0, DOOR_U1, DOOR_V_TOP, STONE_VDARK)


def draw_strap(img, arm, anchor, u0, u1, v0, v1):
    """One horizontal iron strap band across the leaf."""
    face_rect(img, arm, anchor, u0, u1, v0, v1, IRON)
    face_line(img, arm, anchor, u0, v1, u1, v1, IRON_HI)
    face_line(img, arm, anchor, u0, v0, u1, v0, IRON_DARK)


def draw_ring(img, arm, anchor, u, v):
    """Brass ring handle — drawn in screen space around the projected point
    (a dangling ring doesn't foreshorten with the wall face)."""
    cx, cy = face_pt(arm, u, v, anchor)
    ring_offsets = [
        (-1, -2), (0, -2), (1, -2),
        (-2, -1), (2, -1),
        (-2, 0), (2, 0),
        (-1, 1), (0, 1), (1, 1),
    ]
    for (dx, dy) in ring_offsets:
        x, y = cx + dx, cy + dy
        if 0 <= x < img.width and 0 <= y < img.height:
            img.putpixel((x, y), RING)
    if 0 <= cx - 1 < img.width and 0 <= cy - 2 < img.height:
        img.putpixel((cx - 1, cy - 2), RING_HI)
    # Mounting plate above the ring.
    if 0 <= cx < img.width and 0 <= cy - 3 < img.height:
        img.putpixel((cx, cy - 3), IRON_DARK)


def draw_hinge(img, arm, anchor, v0, v1, empty=False):
    """Hinge plate on the hinge jamb (u0 side). `empty=True` draws just the
    dark plate left behind when the leaf has swung away."""
    face_rect(
        img, arm, anchor, DOOR_U0 + 0.005, DOOR_U0 + 0.06, v0, v1,
        IRON_DARK if empty else IRON,
    )
    if not empty:
        face_line(img, arm, anchor, DOOR_U0 + 0.005, v1, DOOR_U0 + 0.06, v1, IRON_HI)


STRAP_BANDS = [(0.10, 0.18), (0.52, 0.60)]
HINGE_BANDS = [(0.12, 0.20), (0.55, 0.63)]


def make_closed(spec, cw, ch, anchor):
    img = Image.new("RGBA", (cw, ch), BG)
    arm = spec["arm"]
    draw_stone_arm(img, arm, anchor, skip_uv=(DOOR_U0, DOOR_U1, DOOR_V_TOP))
    draw_lintel(img, arm, anchor)

    # Door leaf filling the opening, flush with the wall plane.
    face_rect(img, arm, anchor, DOOR_U0, DOOR_U1, 0.0, DOOR_V_TOP, WOOD)
    # Vertical plank seams (4 planks). "Vertical" = along the slab height,
    # so they foreshorten exactly like the masonry joints do.
    plank_w = (DOOR_U1 - DOOR_U0) / 4.0
    for k in range(1, 4):
        u = DOOR_U0 + k * plank_w
        face_line(img, arm, anchor, u, 0.03, u, DOOR_V_TOP - 0.04, WOOD_DARK)
        face_line(img, arm, anchor, u + 0.012, 0.03, u + 0.012, DOOR_V_TOP - 0.04, WOOD_GRAIN)
    # Leaf bevel: lit top + hinge-side edge, shadowed bottom + handle side.
    face_line(img, arm, anchor, DOOR_U0, DOOR_V_TOP - 0.02, DOOR_U1, DOOR_V_TOP - 0.02, WOOD_HI)
    face_line(img, arm, anchor, DOOR_U0 + 0.01, 0.0, DOOR_U0 + 0.01, DOOR_V_TOP, WOOD_HI)
    face_line(img, arm, anchor, DOOR_U0, 0.02, DOOR_U1, 0.02, WOOD_DARK)
    face_line(img, arm, anchor, DOOR_U1 - 0.01, 0.0, DOOR_U1 - 0.01, DOOR_V_TOP, WOOD_DARK)

    for (v0, v1) in STRAP_BANDS:
        draw_strap(img, arm, anchor, DOOR_U0 + 0.02, DOOR_U1 - 0.02, v0, v1)
    for (v0, v1) in HINGE_BANDS:
        draw_hinge(img, arm, anchor, v0, v1)
    draw_ring(img, arm, anchor, DOOR_U1 - 0.12, 0.34)

    draw_jamb_edges(img, arm, anchor)
    return img


def make_open(spec, cw, ch, anchor):
    img = Image.new("RGBA", (cw, ch), BG)
    arm = spec["arm"]
    draw_stone_arm(img, arm, anchor, skip_uv=(DOOR_U0, DOOR_U1, DOOR_V_TOP))
    draw_lintel(img, arm, anchor)

    # Dark passable opening with a lit threshold strip at the base.
    face_rect(img, arm, anchor, DOOR_U0, DOOR_U1, 0.0, DOOR_V_TOP, DARK_VOID)
    face_rect(img, arm, anchor, DOOR_U0, DOOR_U1, 0.0, 0.07, THRESH_DARK)
    face_line(img, arm, anchor, DOOR_U0, 0.07, DOOR_U1, 0.07, THRESHOLD)
    # Depth shadow along the lintel underside inside the opening.
    face_line(img, arm, anchor, DOOR_U0, DOOR_V_TOP - 0.03, DOOR_U1, DOOR_V_TOP - 0.03, (35, 30, 25, 255))

    # Leaf swung back flat against the inner face of the hinge jamb — a thin
    # in-plane sliver so the open state stays inside the wall canvas.
    leaf_u1 = DOOR_U0 + 0.10
    face_rect(img, arm, anchor, DOOR_U0 + 0.01, leaf_u1, 0.0, DOOR_V_TOP - 0.02, WOOD)
    face_line(img, arm, anchor, leaf_u1, 0.0, leaf_u1, DOOR_V_TOP - 0.02, WOOD_HI)
    face_line(img, arm, anchor, DOOR_U0 + 0.05, 0.02, DOOR_U0 + 0.05, DOOR_V_TOP - 0.05, WOOD_DARK)
    face_line(img, arm, anchor, DOOR_U0 + 0.062, 0.02, DOOR_U0 + 0.062, DOOR_V_TOP - 0.05, WOOD_GRAIN)
    for (v0, v1) in STRAP_BANDS:
        face_rect(img, arm, anchor, DOOR_U0 + 0.01, leaf_u1, v0, v0 + 0.03, IRON)
    # Empty hinge plates left on the jamb.
    for (v0, v1) in HINGE_BANDS:
        draw_hinge(img, arm, anchor, v0, v1, empty=True)

    draw_jamb_edges(img, arm, anchor)
    return img


# ── Metadata YAML emission ───────────────────────────────────────────────
META_TEMPLATE = """extends: obstacle
name: {name}
description: {description}
render:
  z_index: 0.3
  debug_color: [110, 70, 40]
  debug_size: 1.0
  sprite_path: overworld_objects/{id}/closed.png
  sprite_width_tiles: {w_tiles}
  sprite_height_tiles: {h_tiles}
  occludes_floor_above: true
  block_size: 2
  walkable_surface: true
  floor_mask_rect: [{mask}]
  hide_when_inside_facing: {hide_facing}
  stack_order: 50
states:
  locked:
    sprite_path: overworld_objects/{id}/closed.png
    colliding: true
  closed:
    sprite_path: overworld_objects/{id}/closed.png
    colliding: true
  open:
    sprite_path: overworld_objects/{id}/open.png
    colliding: false
initial_state: closed
lock:
  lock_id: 7
  pick_dc: 15
  force_dc: 18
interactions:
  - verb: pick_lock
    label: Pick Lock
    from: [locked]
    to: closed
    skill_gate:
      skill: Thievery
      dc: from_lock_pick
  - verb: force_lock
    label: Force Lock
    from: [locked]
    to: closed
    skill_gate:
      skill: Athletics
      dc: from_lock_force
  - verb: use_key
    label: Use Key
    from: [locked]
    to: closed
    key_gate:
      source: from_lock
  - verb: open
    label: Open
    from: [closed]
    to: open
  - verb: close
    label: Close
    from: [open]
    to: closed
"""


def _fmt_tiles(px_value):
    """Format a tile count as either an integer-valued float (`2.0`) or a
    decimal with up to 3 dp, trimming trailing zeros while keeping `.0`."""
    v = px_value / TILE_PX
    if v == int(v):
        return f"{int(v)}.0"
    return f"{v:.3f}".rstrip("0").rstrip(".")


def write_metadata(spec, cw, ch):
    path = os.path.join(ASSETS_DIR, spec["id"], "metadata.yaml")
    mask = ", ".join(f"{round(v, 3)}" for v in spec["floor_mask"])
    content = META_TEMPLATE.format(
        id=spec["id"],
        name=spec["name"],
        description=spec["description"],
        w_tiles=_fmt_tiles(cw),
        h_tiles=_fmt_tiles(ch),
        mask=mask,
        hide_facing=spec["hide_facing"],
    )
    with open(path, "w") as f:
        f.write(content)
    print(f"  metadata: {path}")


def main():
    for spec in DOOR_SPECS:
        dir_path = os.path.join(ASSETS_DIR, spec["id"])
        os.makedirs(dir_path, exist_ok=True)
        # Same canvas as the matching wall — the client sizes every state's
        # sprite from the base render dims, so closed/open must share it.
        cw, ch, anchor = canvas_for_content(arm_corners_3d(spec["arm"]))
        closed = make_closed(spec, cw, ch, anchor)
        closed.save(os.path.join(dir_path, "closed.png"))
        opened = make_open(spec, cw, ch, anchor)
        opened.save(os.path.join(dir_path, "open.png"))
        print(f"Saved {dir_path}/closed.png + open.png  ({cw}×{ch})")
        write_metadata(spec, cw, ch)


if __name__ == "__main__":
    main()
