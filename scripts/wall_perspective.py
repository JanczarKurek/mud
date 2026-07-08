"""
Shared projection / perspective constants for the wall-set generator.

Mirror these constants in `src/world/systems.rs` (the renderer's per-floor
shift) so the generated sprites land flush with the renderer's expectations.
After changing them here, also update the Rust side and run

    python3 scripts/gen_wall_set.py

to regenerate every wall + corner sprite + metadata YAML.

Coordinate conventions
----------------------
3D "floor coords" are (fx, fy, fz) where:
    + fx = east  (one tile per unit)
    + fy = north (one tile per unit; matches Bevy +y = up)
    + fz = floors up (one floor unit per FLOOR_SHIFT_{X,Y}_TILES tiles
                      of screen shift)

PIL canvas coords are (px, py) with +py = DOWN. Functions here flip the
sign on the y axis so a 3D point above the floor projects to a smaller PIL
y (visually higher up on the canvas).
"""

import hashlib
import math

# ── Mirror these from src/world/systems.rs ───────────────────────────────
TILE_PX = 48                  # WorldConfig.tile_size
FLOOR_SHIFT_X_TILES = -0.75    # FLOOR_SHIFT_X_TILES
FLOOR_SHIFT_Y_TILES = 0.5     # FLOOR_SHIFT_Y_TILES

# ── Script-only knob: how tall the wall body is in floors (visual only) ─
WALL_HEIGHT_FLOORS = 1.0

# ── Script-only knob: how far the wall slab sits inward from its outer
# tile edge, in tile units. 0.0 = slab flush with the outer edge (visually
# right at the perimeter), 0.5 = slab at the tile midline (no directional
# bias). Tune up to move walls further toward the middle of their tile so
# players can stand closer to the visual edge of a room without overlapping
# the sprite. Applies to wall_s and wall_e directly; wall_n and wall_w are
# clamped (see below) so their visual top doesn't extend past their tile.
WALL_INSET = 0.25

# Visual extent of the wall body in tile units due to the iso floor shift.
# A 1-floor-tall wall projects fz=1 to (FLOOR_SHIFT_X_TILES, -FLOOR_SHIFT_Y_TILES)
# tiles on screen — i.e. the slab TOP sits 0.5 tiles up-left of the slab
# bottom. For wall_n / wall_w, that means a slab placed at the "natural"
# inset position (fy = 1 - INSET / fx = INSET) would visually overshoot
# the tile's north / west boundary by exactly this amount. So we cap them
# to keep the rendered sprite inside its tile cell.
WALL_VIZ_HEIGHT_TILES = WALL_HEIGHT_FLOORS * abs(FLOOR_SHIFT_Y_TILES)
WALL_VIZ_WIDTH_TILES  = WALL_HEIGHT_FLOORS * abs(FLOOR_SHIFT_X_TILES)

# Derived: screen shift per floor in pixels (Bevy world coords: +y up).
SHIFT_X_PX = FLOOR_SHIFT_X_TILES * TILE_PX
SHIFT_Y_PX = FLOOR_SHIFT_Y_TILES * TILE_PX

# ── Slab base positions (shared by walls and directional doors) ──────────
# The fy of an axis="y" arm / the fx of an axis="x" arm. wall_s / wall_e use
# the requested inset directly; wall_n / wall_w clamp toward the centre
# because the iso projection extends the visible slab top up-and-left by
# WALL_VIZ_*_TILES — without the clamp those two would visually overshoot
# their tile's north / west boundary. These feed both the sprite geometry
# and the emitted `floor_mask_rect`s, so they must not change lightly.
SLAB_S = WALL_INSET
SLAB_E = 1.0 - WALL_INSET
SLAB_N = min(1.0 - WALL_INSET, 1.0 - WALL_VIZ_HEIGHT_TILES)
SLAB_W = max(WALL_INSET, WALL_VIZ_WIDTH_TILES)

# ── Hewn-stone wall palette (shared with corners and doors) ──────────────
# Warm sandstone tones (2026-07 art pass; previously cool gray).
BG          = (  0,   0,   0,   0)
STONE       = (168, 140, 105, 255)
STONE_HI    = (198, 172, 134, 255)
STONE_DARK  = (122,  98,  70, 255)
STONE_VDARK = ( 74,  58,  42, 255)
MORTAR      = ( 99,  80,  60, 255)
CAP_HI      = (214, 190, 150, 255)
CAP_MID     = (176, 150, 114, 255)
CAP_DARK    = (120,  98,  70, 255)
CAP_TOP     = (232, 210, 172, 255)

# Door-leaf palette (lifted from scripts/gen_wooden_door_sprites.py so the
# directional doors match the legacy flat door's wood/iron/brass).
WOOD        = (110,  70,  35, 255)
WOOD_HI     = (150,  98,  55, 255)
WOOD_DARK   = ( 70,  42,  18, 255)
WOOD_GRAIN  = ( 90,  55,  22, 255)
IRON        = ( 75,  78,  88, 255)
IRON_HI     = (130, 132, 142, 255)
IRON_DARK   = ( 40,  42,  50, 255)
RING        = (180, 145,  45, 255)
RING_HI     = (240, 200,  85, 255)
DARK_VOID   = ( 18,  14,  10, 255)
THRESHOLD   = ( 75,  60,  40, 255)
THRESH_DARK = ( 45,  38,  30, 255)


# ── Projection ───────────────────────────────────────────────────────────
def project(fx, fy, fz, anchor):
    """Project a 3D floor-coord point to PIL pixel coords.

    `anchor` is the PIL pixel of the 3D origin (0, 0, 0). +fx maps to +px
    (east = right). +fy maps to -py (north = visually up on canvas).
    +fz maps to (SHIFT_X_PX, -SHIFT_Y_PX) in PIL pixels per floor.
    """
    return (
        round(anchor[0] + fx * TILE_PX + fz * SHIFT_X_PX),
        round(anchor[1] - fy * TILE_PX - fz * SHIFT_Y_PX),
    )


def canvas_for_box(fw_tiles, fd_tiles, h_floors=WALL_HEIGHT_FLOORS):
    """Size a canvas to fit the projection of a 3D box.

    Box spans floor coords [0..fw_tiles] × [0..fd_tiles] × [0..h_floors].
    Returns (canvas_w, canvas_h, anchor_px) where canvas dims are rounded
    up to multiples of TILE_PX (keeps `sprite_width_tiles` integral) and
    `anchor_px` places the FOOTPRINT center at the canvas's bottom-center
    (Bevy `Anchor::BOTTOM_CENTER` lands the sprite on the home tile's base).
    """
    corners = [
        (x, y, z)
        for x in (0.0, fw_tiles)
        for y in (0.0, fd_tiles)
        for z in (0.0, h_floors)
    ]
    # Tentative anchor at (0, 0) → find AABB → fit canvas → reposition anchor.
    pts = [project(x, y, z, (0, 0)) for (x, y, z) in corners]
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    min_x, max_x = min(xs), max(xs)
    min_y, max_y = min(ys), max(ys)

    raw_w = max_x - min_x + 1
    raw_h = max_y - min_y + 1
    canvas_w = max(int(math.ceil(raw_w / TILE_PX)) * TILE_PX, TILE_PX)
    canvas_h = max(int(math.ceil(raw_h / TILE_PX)) * TILE_PX, TILE_PX)

    # Place anchor so the footprint CENTER ends up at canvas bottom-center.
    # project(fw/2, fd/2, 0, anchor) must equal (canvas_w/2, canvas_h-1).
    anchor_x = round(canvas_w / 2 - fw_tiles * TILE_PX / 2)
    anchor_y = round(canvas_h - 1 + fd_tiles * TILE_PX / 2)

    # Verify every projected corner is in-bounds; nudge if not.
    nudge_x, nudge_y = 0, 0
    for (x, y, z) in corners:
        px, py = project(x, y, z, (anchor_x, anchor_y))
        if px < 0:
            nudge_x = max(nudge_x, -px)
        if py < 0:
            nudge_y = max(nudge_y, -py)
    anchor_x += nudge_x
    anchor_y += nudge_y

    return canvas_w, canvas_h, (anchor_x, anchor_y)


# ── Drawing primitives ───────────────────────────────────────────────────
def px(img, x, y, color):
    if 0 <= x < img.width and 0 <= y < img.height:
        img.putpixel((x, y), color)


def rect(img, x, y, w, h, color):
    for dy in range(h):
        for dx in range(w):
            px(img, x + dx, y + dy, color)


def fill_polygon(img, pts, color):
    """Fill a convex polygon (list of PIL (x, y) points) with `color`.

    Scanline implementation; clips to canvas bounds. Uses a 1-px overdraw
    on the right edge of each scanline to match the way the existing wall
    scripts fill (no seams at parallelogram edges).
    """
    if not pts:
        return
    ys = [p[1] for p in pts]
    y_min = max(min(ys), 0)
    y_max = min(max(ys), img.height - 1)
    for y in range(y_min, y_max + 1):
        xs = _polygon_intersections_at(pts, y)
        if not xs:
            continue
        x_start = max(int(round(min(xs))), 0)
        x_end = min(int(round(max(xs))), img.width - 1)
        for x in range(x_start, x_end + 1):
            img.putpixel((x, y), color)


def stroke_polygon(img, pts, color):
    """Draw the outline of a polygon as 1-px-thick line segments."""
    n = len(pts)
    for i in range(n):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % n]
        _line(img, x0, y0, x1, y1, color)


def _polygon_intersections_at(pts, y):
    out = []
    n = len(pts)
    for i in range(n):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % n]
        if y0 == y1:
            continue
        if (y0 <= y < y1) or (y1 <= y < y0):
            t = (y - y0) / (y1 - y0)
            out.append(x0 + t * (x1 - x0))
    return out


def _line(img, x0, y0, x1, y1, color):
    dx = x1 - x0
    dy = y1 - y0
    steps = max(abs(dx), abs(dy))
    if steps == 0:
        px(img, x0, y0, color)
        return
    for i in range(steps + 1):
        t = i / steps
        x = int(round(x0 + dx * t))
        y = int(round(y0 + dy * t))
        px(img, x, y, color)


def shade_polygon(img, pts, factor):
    """Multiply the RGB of every already-drawn pixel inside a convex polygon
    by `factor` (alpha untouched, transparent pixels skipped). Used for
    contact shadows so they follow whatever masonry is underneath."""
    if not pts:
        return
    ys = [p[1] for p in pts]
    y_min = max(min(ys), 0)
    y_max = min(max(ys), img.height - 1)
    for y in range(y_min, y_max + 1):
        xs = _polygon_intersections_at(pts, y)
        if not xs:
            continue
        x_start = max(int(round(min(xs))), 0)
        x_end = min(int(round(max(xs))), img.width - 1)
        for x in range(x_start, x_end + 1):
            r, g, b, a = img.getpixel((x, y))
            if a == 0:
                continue
            img.putpixel(
                (x, y),
                (int(r * factor), int(g * factor), int(b * factor), a),
            )


# ── Arm geometry (shared by the wall and door generators) ────────────────
#
# An "arm" is a zero-thickness vertical wall slab: a dict with
#   axis="y": at constant fy (horizontal wall); fx ∈ [t0, t1], fz ∈ [0, H]
#   axis="x": at constant fx (vertical wall);   fy ∈ [t0, t1], fz ∈ [0, H]
# The along-arm coordinate `u` is a WORLD coordinate (fx for axis="y", fy
# for axis="x"), so masonry keyed on u tiles seamlessly across neighbouring
# wall tiles and around corner arms that share the axis.


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


def face_pt(arm, u, v, anchor):
    """Project the face point at along-arm world coord `u`, height fraction
    `v` (0 = slab base, 1 = slab top) to PIL pixel coords."""
    fz = v * WALL_HEIGHT_FLOORS
    if arm["axis"] == "y":
        return project(u, arm["pos"], fz, anchor)
    return project(arm["pos"], u, fz, anchor)


def arm_depth(arm):
    """Mean fy of the arm — higher fy is farther from the camera (camera is
    up-left of the player, lower fy = south = closer = drawn later)."""
    if arm["axis"] == "y":
        return arm["pos"]
    return (arm["t0"] + arm["t1"]) / 2.0


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


# ── Hewn-stone face renderer ─────────────────────────────────────────────

# Face layout in v: coping cap on top, stone-block courses below.
CAP_V0 = 0.80          # cap band spans v ∈ [CAP_V0, 1.0]
COURSES = 3            # stone courses in the body band v ∈ [0, CAP_V0]
BLOCK_W_U = 1.0 / 3.0  # block width along u, in tile units (16 px on screen)
CAP_JOINT_U = 0.5      # coping-stone joint spacing along u, in tile units

# Per-block shade offsets added to STONE, chosen by hash — quantized so
# regeneration is byte-stable and the variation stays subtle.
_SHADE_STEPS = (-14, -6, 4, 12)


def _block_shade(axis, row, col):
    """Deterministic per-block stone shade. Keyed on (axis, row, col) with
    `col` indexed on WORLD u so the same block gets the same shade whichever
    arm (straight wall or corner) paints it."""
    digest = hashlib.md5(f"{axis}:{row}:{col}".encode()).digest()
    delta = _SHADE_STEPS[digest[0] % len(_SHADE_STEPS)]
    r, g, b, a = STONE
    return (
        max(0, min(255, r + delta)),
        max(0, min(255, g + delta)),
        max(0, min(255, b + delta)),
        a,
    )


def _lighten(color, amount):
    r, g, b, a = color
    return (
        min(255, r + amount),
        min(255, g + amount),
        min(255, b + amount),
        a,
    )


def _darken(color, amount):
    r, g, b, a = color
    return (max(0, r - amount), max(0, g - amount), max(0, b - amount), a)


def _split_interval(u0, u1, hole):
    """Clip [u0, u1] against an open hole interval; return the kept pieces."""
    if hole is None:
        return [(u0, u1)]
    h0, h1 = hole
    if u1 <= h0 or u0 >= h1:
        return [(u0, u1)]
    pieces = []
    if u0 < h0:
        pieces.append((u0, h0))
    if u1 > h1:
        pieces.append((h1, u1))
    return pieces


def _face_quad(arm, u0, u1, v0, v1, anchor):
    return [
        face_pt(arm, u0, v0, anchor),
        face_pt(arm, u1, v0, anchor),
        face_pt(arm, u1, v1, anchor),
        face_pt(arm, u0, v1, anchor),
    ]


def draw_stone_arm(img, arm, anchor, skip_uv=None):
    """Draw one wall slab as textured hewn sandstone.

    Staggered block courses topped by a coping cap; per-block shade chosen by
    a deterministic hash keyed on world-u so masonry tiles seamlessly across
    neighbouring wall tiles and corner arms.

    `skip_uv=(u0, u1, v_top)` cuts a doorway: body blocks inside u ∈ [u0, u1]
    below v_top are skipped, blocks straddling the edges are clipped at them.
    The caller is expected to paint a lintel over v ∈ [~v_top, CAP_V0] across
    the hole (the cap band above still renders here).
    """
    axis = arm["axis"]
    t0, t1 = arm["t0"], arm["t1"]
    bl, br, tr, tl = [project(x, y, z, anchor) for (x, y, z) in arm_corners_3d(arm)]

    hole_u = None
    hole_v_top = None
    if skip_uv is not None:
        hole_u = (skip_uv[0], skip_uv[1])
        hole_v_top = skip_uv[2]

    # ── Stone-block courses ──────────────────────────────────────────────
    course_h = CAP_V0 / COURSES
    for row in range(COURSES):
        v0 = row * course_h
        v1 = v0 + course_h
        # Rows whose whole band is below the doorway top get the hole cut;
        # the course straddling v_top is clipped in u for its full band (the
        # caller's lintel covers the leftover strip above the opening).
        row_hole = hole_u if hole_v_top is not None and v0 < hole_v_top else None
        stagger = (row % 2) * BLOCK_W_U / 2.0
        col0 = math.floor((t0 - stagger) / BLOCK_W_U)
        col1 = math.ceil((t1 - stagger) / BLOCK_W_U)
        for col in range(col0, col1 + 1):
            u0 = stagger + col * BLOCK_W_U
            u1 = u0 + BLOCK_W_U
            u0, u1 = max(u0, t0), min(u1, t1)
            if u1 <= u0:
                continue
            for (pu0, pu1) in _split_interval(u0, u1, row_hole):
                shade = _block_shade(axis, row, col)
                quad = _face_quad(arm, pu0, pu1, v0, v1, anchor)
                fill_polygon(img, quad, shade)
                # Bevel: lit top + leading edge, shadowed bottom + trailing.
                q_bl, q_br, q_tr, q_tl = quad
                _line(img, q_tl[0], q_tl[1], q_tr[0], q_tr[1], _lighten(shade, 22))
                _line(img, q_tl[0], q_tl[1], q_bl[0], q_bl[1], _lighten(shade, 12))
                _line(img, q_bl[0], q_bl[1], q_br[0], q_br[1], _darken(shade, 20))
                _line(img, q_br[0], q_br[1], q_tr[0], q_tr[1], _darken(shade, 12))

    # Mortar joints AFTER the block fills — fill_polygon overdraws 1 px on
    # scanline right edges, so joints drawn first would get eaten.
    for row in range(COURSES):
        v0 = row * course_h
        v1 = v0 + course_h
        row_hole = hole_u if hole_v_top is not None and v0 < hole_v_top else None
        # Horizontal course seam (skip the baseline at v=0).
        if row > 0:
            for (su0, su1) in _split_interval(t0, t1, row_hole):
                a = face_pt(arm, su0, v0, anchor)
                b = face_pt(arm, su1, v0, anchor)
                _line(img, a[0], a[1], b[0], b[1], MORTAR)
        # Vertical joints at block boundaries within this course.
        stagger = (row % 2) * BLOCK_W_U / 2.0
        col0 = math.floor((t0 - stagger) / BLOCK_W_U)
        col1 = math.ceil((t1 - stagger) / BLOCK_W_U)
        for col in range(col0, col1 + 1):
            u = stagger + col * BLOCK_W_U
            if u <= t0 or u >= t1:
                continue
            if row_hole is not None and row_hole[0] < u < row_hole[1]:
                continue
            a = face_pt(arm, u, v0, anchor)
            b = face_pt(arm, u, v1, anchor)
            _line(img, a[0], a[1], b[0], b[1], MORTAR)

    # ── Coping cap band (always full span — masonry continues over a
    # doorway's lintel) ──────────────────────────────────────────────────
    cap_bl = face_pt(arm, t0, CAP_V0, anchor)
    cap_br = face_pt(arm, t1, CAP_V0, anchor)
    fill_polygon(img, [cap_bl, cap_br, tr, tl], CAP_HI)
    # Top highlight along the crown, shadow along the underside.
    _line(img, tl[0], tl[1], tr[0], tr[1], CAP_TOP)
    _line(img, cap_bl[0], cap_bl[1], cap_br[0], cap_br[1], CAP_DARK)
    # Coping joints every CAP_JOINT_U of world u.
    joint0 = math.floor(t0 / CAP_JOINT_U)
    joint1 = math.ceil(t1 / CAP_JOINT_U)
    for j in range(joint0, joint1 + 1):
        u = j * CAP_JOINT_U
        if u <= t0 or u >= t1:
            continue
        a = face_pt(arm, u, CAP_V0, anchor)
        b = face_pt(arm, u, 1.0, anchor)
        _line(img, a[0], a[1], b[0], b[1], CAP_MID)

    # ── Base contact shadow (skip the doorway span — no wall meets the
    # ground there) ──────────────────────────────────────────────────────
    for (su0, su1) in _split_interval(t0, t1, hole_u):
        shadow_quad = _face_quad(arm, su0, su1, 0.0, 0.07, anchor)
        shade_polygon(img, shadow_quad, 0.72)

    # ── Edge outlines: bottom seam shadowed, leading slanted edge lit,
    # trailing edge shadowed (matches the old flat-shaded art) ───────────
    for (su0, su1) in _split_interval(t0, t1, hole_u):
        a = face_pt(arm, su0, 0.0, anchor)
        b = face_pt(arm, su1, 0.0, anchor)
        _line(img, a[0], a[1], b[0], b[1], STONE_VDARK)
    _line(img, bl[0], bl[1], tl[0], tl[1], STONE_HI)
    _line(img, br[0], br[1], tr[0], tr[1], STONE_DARK)
