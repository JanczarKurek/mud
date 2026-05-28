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

# ── Hewn-stone wall palette (shared with corners) ────────────────────────
BG          = (  0,   0,   0,   0)
STONE       = (120, 114, 103, 255)
STONE_HI    = (160, 150, 134, 255)
STONE_DARK  = ( 80,  74,  66, 255)
STONE_VDARK = ( 50,  46,  40, 255)
MORTAR      = ( 55,  50,  44, 255)
CAP_HI      = (180, 170, 154, 255)
CAP_MID     = (140, 132, 118, 255)
CAP_DARK    = ( 88,  82,  72, 255)


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
