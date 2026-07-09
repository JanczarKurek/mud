"""
Generate the perspective container sprites + metadata:

    iron_chest            south-facing chest (regenerated in place), closed/open
    iron_chest_e/_n/_w    facing variants (new), closed/open
    barrel                sheared-cylinder barrel (regenerated in place)
    crate                 planked pine cube (new)

All geometry uses the shared oblique projection from wall_perspective.py so
containers read as 3D boxes/cylinders consistent with the wall/door set:
visible faces are TOP, SOUTH and EAST; height shears up-left by
(SHIFT_X_PX, -SHIFT_Y_PX) per floor.

Stacking contract (src/world/systems.rs): block_size 2 art tops out at
fz = 1.0, block_size 1 art at fz = 0.5, so stacked objects sit flush.

Run from the repo root:

    python3 scripts/gen_container_set.py

Idempotent: running twice produces byte-identical PNGs and YAMLs.
Supersedes scripts/gen_iron_chest_sprites.py and scripts/gen_barrel_sprite.py
(both deleted when this script landed).
"""

import hashlib
import math
import os

from PIL import Image

from wall_perspective import (
    TILE_PX,
    SHIFT_X_PX,
    SHIFT_Y_PX,
    BG,
    WOOD,
    WOOD_HI,
    WOOD_DARK,
    IRON,
    IRON_HI,
    IRON_DARK,
    RING,
    RING_HI,
    canvas_for_content,
    fill_polygon,
    project,
    _line,
    _lighten,
    _darken,
)

ASSETS_DIR = "assets/overworld_objects"

# Crate palette: pale planked pine (the wall set has no light wood).
PINE       = (168, 128,  76, 255)
PINE_HI    = (200, 160, 104, 255)
PINE_DARK  = (118,  86,  48, 255)
PINE_GRAIN = (142, 104,  60, 255)

INTERIOR   = ( 52,  38,  22, 255)
SHADOW_A   = 70  # ground-shadow alpha


# ── Small helpers ────────────────────────────────────────────────────────
def hash_shade(key, base, steps=(-12, -5, 4, 10)):
    """Deterministic per-part shade offset (byte-stable regeneration)."""
    digest = hashlib.md5(key.encode()).digest()
    delta = steps[digest[0] % len(steps)]
    r, g, b, a = base
    return (
        max(0, min(255, r + delta)),
        max(0, min(255, g + delta)),
        max(0, min(255, b + delta)),
        a,
    )


def proj_f(fx, fy, fz, anchor):
    """Unrounded projection (for smooth circle sweeps)."""
    return (
        anchor[0] + fx * TILE_PX + fz * SHIFT_X_PX,
        anchor[1] - fy * TILE_PX - fz * SHIFT_Y_PX,
    )


def quad3(pts3, anchor):
    return [project(x, y, z, anchor) for (x, y, z) in pts3]


def fill3(img, pts3, anchor, color):
    fill_polygon(img, quad3(pts3, anchor), color)


def line3(img, a3, b3, anchor, color):
    a = project(*a3, anchor)
    b = project(*b3, anchor)
    _line(img, a[0], a[1], b[0], b[1], color)


def south_face(x0, x1, y, z0, z1):
    return [(x0, y, z0), (x1, y, z0), (x1, y, z1), (x0, y, z1)]


def east_face(y0, y1, x, z0, z1):
    return [(x, y0, z0), (x, y1, z0), (x, y1, z1), (x, y0, z1)]


def top_face(x0, y0, x1, y1, z):
    return [(x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z)]


def ground_shadow(img, anchor, cx, cy, rx, ry):
    """Soft translucent ellipse on the floor plane. Drawn FIRST (the canvas
    is transparent, so shade_polygon can't do contact shadows here); the
    body overdraws it with opaque pixels."""
    cpx, cpy = proj_f(cx, cy, 0.0, anchor)
    rxp, ryp = rx * TILE_PX, ry * TILE_PX
    x_lo = max(int(cpx - rxp) - 1, 0)
    x_hi = min(int(cpx + rxp) + 1, img.width - 1)
    y_lo = max(int(cpy - ryp) - 1, 0)
    y_hi = min(int(cpy + ryp) + 1, img.height - 1)
    for y in range(y_lo, y_hi + 1):
        for x in range(x_lo, x_hi + 1):
            dx = (x - cpx) / rxp
            dy = (y - cpy) / ryp
            d = dx * dx + dy * dy
            if d <= 1.0:
                a = SHADOW_A if d <= 0.72 else SHADOW_A // 2
                img.putpixel((x, y), (0, 0, 0, a))


def outline_silhouette(img, factor=0.72):
    """Darken opaque pixels that border the outside (alpha < 200, i.e.
    transparency or the translucent ground shadow) — crisp 1px rim."""
    w, h = img.width, img.height
    src = img.load()
    edges = []
    for y in range(h):
        for x in range(w):
            r, g, b, a = src[x, y]
            if a < 200:
                continue
            for (nx, ny) in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                if nx < 0 or ny < 0 or nx >= w or ny >= h or src[nx, ny][3] < 200:
                    edges.append((x, y, r, g, b, a))
                    break
    for (x, y, r, g, b, a) in edges:
        src[x, y] = (int(r * factor), int(g * factor), int(b * factor), a)


# ═════════════════════════════════ CHEST ════════════════════════════════
CH_X0, CH_X1 = 0.10, 0.90
CH_Y0, CH_Y1 = 0.10, 0.90
BODY_TOP = 0.36          # body/lid seam height (floors)
LID_TOP = 0.50           # closed-lid top — exactly block_size 1 (half block)
LID_LIP = 0.02           # lid overhang past the body
OPEN_LID_TOP = 0.85      # top of the raised lid slab in the open state

# facing → the visible face carrying the lock plate ('s'/'e') or, when the
# lock faces away ('n'/'w'), which visible face shows the hinge straps.
CHEST_SPECS = [
    {
        "id": "iron_chest",
        "name": "Iron Chest",
        "description": "A heavy iron-banded chest. Its lock faces south.",
        "facing": "s",
    },
    {
        "id": "iron_chest_e",
        "name": "Iron Chest (East)",
        "description": "A heavy iron-banded chest. Its lock faces east.",
        "facing": "e",
    },
    {
        "id": "iron_chest_n",
        "name": "Iron Chest (North)",
        "description": "A heavy iron-banded chest. Its lock faces north, away from view.",
        "facing": "n",
    },
    {
        "id": "iron_chest_w",
        "name": "Iron Chest (West)",
        "description": "A heavy iron-banded chest. Its lock faces west, away from view.",
        "facing": "w",
    },
]

HINGE_FOR_FACING = {"s": "n", "e": "w", "n": "s", "w": "e"}


def chest_canvas():
    """One canvas shared by all 4 facings × both states (the client sizes
    every state's sprite from the base render dims)."""
    corners = []
    for x in (CH_X0 - LID_LIP, CH_X1 + LID_LIP):
        for y in (CH_Y0 - LID_LIP, CH_Y1 + LID_LIP):
            corners.append((x, y, 0.0))
            corners.append((x, y, LID_TOP))
            corners.append((x, y, OPEN_LID_TOP))  # any hinge side's raised lid
    return canvas_for_content(corners)


def draw_face_planks(img, anchor, face_kind, pos, a0, a1, z0, z1, n, key, base=WOOD):
    """Horizontal planks on a vertical face. face_kind 's' = face at fy=pos
    spanning fx∈[a0,a1]; 'e' = face at fx=pos spanning fy∈[a0,a1]."""
    dz = (z1 - z0) / n
    for k in range(n):
        pz0 = z0 + k * dz
        pz1 = pz0 + dz
        shade = hash_shade(f"{key}:{face_kind}:{k}", base)
        if face_kind == "s":
            pts = south_face(a0, a1, pos, pz0, pz1)
        else:
            pts = east_face(a0, a1, pos, pz0, pz1)
        fill3(img, pts, anchor, shade)
        # plank seam (bottom of each plank except the lowest) + lit top edge
        if face_kind == "s":
            seam_a, seam_b = (a0, pos, pz0), (a1, pos, pz0)
            top_a, top_b = (a0, pos, pz1), (a1, pos, pz1)
        else:
            seam_a, seam_b = (pos, a0, pz0), (pos, a1, pz0)
            top_a, top_b = (pos, a0, pz1), (pos, a1, pz1)
        if k > 0:
            line3(img, seam_a, seam_b, anchor, _darken(shade, 26))
        if k == n - 1:
            line3(img, top_a, top_b, anchor, _lighten(shade, 18))


def draw_iron_band_on_face(img, anchor, face_kind, pos, a0, a1, z0, z1):
    if face_kind == "s":
        pts = south_face(a0, a1, pos, z0, z1)
        hi_a, hi_b = (a0, pos, z1), (a1, pos, z1)
        lo_a, lo_b = (a0, pos, z0), (a1, pos, z0)
    else:
        pts = east_face(a0, a1, pos, z0, z1)
        hi_a, hi_b = (pos, a0, z1), (pos, a1, z1)
        lo_a, lo_b = (pos, a0, z0), (pos, a1, z0)
    fill3(img, pts, anchor, IRON)
    line3(img, hi_a, hi_b, anchor, IRON_HI)
    line3(img, lo_a, lo_b, anchor, IRON_DARK)


def draw_corner_straps(img, anchor, z1):
    """Vertical iron straps on the visible corners of both faces."""
    w = 0.07
    for a in (CH_X0 + 0.02, CH_X1 - 0.02 - w):
        draw_vert_strap(img, anchor, "s", CH_Y0, a, a + w, 0.0, z1)
    for a in (CH_Y0 + 0.02, CH_Y1 - 0.02 - w):
        draw_vert_strap(img, anchor, "e", CH_X1, a, a + w, 0.0, z1)


def draw_vert_strap(img, anchor, face_kind, pos, a0, a1, z0, z1):
    if face_kind == "s":
        pts = south_face(a0, a1, pos, z0, z1)
        left_a, left_b = (a0, pos, z0), (a0, pos, z1)
    else:
        pts = east_face(a0, a1, pos, z0, z1)
        left_a, left_b = (pos, a0, z0), (pos, a0, z1)
    fill3(img, pts, anchor, IRON)
    line3(img, left_a, left_b, anchor, IRON_HI)


def draw_lock_plate(img, anchor, face_kind):
    """Brass lock plate with keyhole, centered on the given visible face."""
    mid = 0.5
    a0, a1 = mid - 0.09, mid + 0.09
    z0, z1 = 0.14, 0.34
    pos = CH_Y0 if face_kind == "s" else CH_X1
    if face_kind == "s":
        pts = south_face(a0, a1, pos, z0, z1)
        hole = project(mid, pos, (z0 + z1) / 2, anchor)
        hi_a, hi_b = (a0, pos, z1), (a1, pos, z1)
    else:
        pts = east_face(a0, a1, pos, z0, z1)
        hole = project(pos, mid, (z0 + z1) / 2, anchor)
        hi_a, hi_b = (pos, a0, z1), (pos, a1, z1)
    fill3(img, pts, anchor, RING)
    line3(img, hi_a, hi_b, anchor, RING_HI)
    for (dx, dy) in ((0, -1), (0, 0), (-1, 1), (0, 1), (1, 1)):
        x, y = hole[0] + dx, hole[1] + dy
        if 0 <= x < img.width and 0 <= y < img.height:
            img.putpixel((x, y), IRON_DARK)


def draw_hinge_plates(img, anchor, face_kind):
    """Two small hinge plates straddling the lid seam (shown when the chest
    faces away and we see its hinge side)."""
    pos = CH_Y0 if face_kind == "s" else CH_X1
    for mid in (0.30, 0.70):
        a0, a1 = mid - 0.05, mid + 0.05
        if face_kind == "s":
            pts = south_face(a0, a1, pos, BODY_TOP - 0.05, BODY_TOP + 0.09)
            hi_a, hi_b = (a0, pos, BODY_TOP + 0.09), (a1, pos, BODY_TOP + 0.09)
        else:
            pts = east_face(a0, a1, pos, BODY_TOP - 0.05, BODY_TOP + 0.09)
            hi_a, hi_b = (pos, a0, BODY_TOP + 0.09), (pos, a1, BODY_TOP + 0.09)
        fill3(img, pts, anchor, IRON)
        line3(img, hi_a, hi_b, anchor, IRON_HI)


def draw_ring_handle(img, anchor, face_kind):
    """Small brass ring, drawn in screen space (a dangling ring doesn't
    foreshorten with the face)."""
    if face_kind == "s":
        cx, cy = project(0.5, CH_Y0, 0.22, anchor)
    else:
        cx, cy = project(CH_X1, 0.5, 0.22, anchor)
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


def draw_chest_body(img, anchor, chest_id, facing, lid=True):
    """Body (and closed lid when `lid`) shared by both states."""
    ground_shadow(img, anchor, 0.5, 0.5, 0.48, 0.30)

    # Body faces up to the seam.
    draw_face_planks(img, anchor, "e", CH_X1, CH_Y0, CH_Y1, 0.0, BODY_TOP, 2,
                     f"{chest_id}:body")
    draw_face_planks(img, anchor, "s", CH_Y0, CH_X0, CH_X1, 0.0, BODY_TOP, 2,
                     f"{chest_id}:body")

    if lid:
        # Lid band with a small overhang lip.
        lx0, lx1 = CH_X0 - LID_LIP, CH_X1 + LID_LIP
        ly0, ly1 = CH_Y0 - LID_LIP, CH_Y1 + LID_LIP
        draw_face_planks(img, anchor, "e", lx1, ly0, ly1, BODY_TOP, LID_TOP, 1,
                         f"{chest_id}:lid")
        draw_face_planks(img, anchor, "s", ly0, lx0, lx1, BODY_TOP, LID_TOP, 1,
                         f"{chest_id}:lid")
        # Lid top: planks running east-west.
        n_strips = 4
        dy = (ly1 - ly0) / n_strips
        for k in range(n_strips):
            y0 = ly0 + k * dy
            shade = hash_shade(f"{chest_id}:top:{k}", WOOD_HI, steps=(-16, -8, 0, 8))
            fill3(img, top_face(lx0, y0, lx1, y0 + dy, LID_TOP), anchor, shade)
            if k > 0:
                line3(img, (lx0, y0, LID_TOP), (lx1, y0, LID_TOP), anchor,
                      _darken(shade, 24))
        # Iron banding around the lid-top perimeter.
        b = 0.045
        for pts in (
            top_face(lx0, ly0, lx1, ly0 + b, LID_TOP),
            top_face(lx0, ly1 - b, lx1, ly1, LID_TOP),
            top_face(lx0, ly0, lx0 + b, ly1, LID_TOP),
            top_face(lx1 - b, ly0, lx1, ly1, LID_TOP),
        ):
            fill3(img, pts, anchor, IRON)
        # Hasp strap across the top, running toward the lock side.
        if facing in ("s", "n"):
            fill3(img, top_face(0.46, ly0, 0.54, ly1, LID_TOP), anchor, IRON)
            line3(img, (0.46, ly0, LID_TOP), (0.46, ly1, LID_TOP), anchor, IRON_HI)
        else:
            fill3(img, top_face(lx0, 0.46, lx1, 0.54, LID_TOP), anchor, IRON)
            line3(img, (lx0, 0.54, LID_TOP), (lx1, 0.54, LID_TOP), anchor, IRON_HI)
        # Brass hasp plate on the lid top at the lock edge — the top face is
        # the chest's dominant face in this projection, so the lock must
        # read there, not just on the short front face.
        hasp = {
            "s": top_face(0.42, ly0, 0.58, ly0 + 0.14, LID_TOP),
            "n": top_face(0.42, ly1 - 0.14, 0.58, ly1, LID_TOP),
            "e": top_face(lx1 - 0.14, 0.42, lx1, 0.58, LID_TOP),
            "w": top_face(lx0, 0.42, lx0 + 0.14, 0.58, LID_TOP),
        }[facing]
        fill3(img, hasp, anchor, RING)
        hx = sum(p[0] for p in hasp) / 4
        hy = sum(p[1] for p in hasp) / 4
        hpx = project(hx, hy, LID_TOP, anchor)
        if 0 <= hpx[0] < img.width and 0 <= hpx[1] < img.height:
            img.putpixel(hpx, IRON_DARK)
            if hpx[1] + 1 < img.height:
                img.putpixel((hpx[0], hpx[1] + 1), IRON_DARK)
        # Seam band where lid meets body.
        draw_iron_band_on_face(img, anchor, "s", CH_Y0, CH_X0, CH_X1,
                               BODY_TOP - 0.03, BODY_TOP + 0.01)
        draw_iron_band_on_face(img, anchor, "e", CH_X1, CH_Y0, CH_Y1,
                               BODY_TOP - 0.03, BODY_TOP + 0.01)

    draw_corner_straps(img, anchor, LID_TOP if lid else BODY_TOP)

    # Facing hardware on the visible faces.
    if facing == "s":
        draw_lock_plate(img, anchor, "s")
    elif facing == "e":
        draw_lock_plate(img, anchor, "e")
    elif facing == "n":
        draw_hinge_plates(img, anchor, "s")
        draw_ring_handle(img, anchor, "e")
    else:  # w
        draw_hinge_plates(img, anchor, "e")
        draw_ring_handle(img, anchor, "s")


def draw_open_lid(img, anchor, chest_id, facing):
    """Raised lid: a vertical slab standing on the hinge edge."""
    hinge = HINGE_FOR_FACING[facing]
    z0, z1 = BODY_TOP, OPEN_LID_TOP
    interior = hinge in ("n", "w")  # lid interior faces the camera
    base = _lighten(WOOD, 8) if interior else WOOD
    if hinge == "n":
        pts = south_face(CH_X0, CH_X1, CH_Y1, z0, z1)
        top_a, top_b = (CH_X0, CH_Y1, z1), (CH_X1, CH_Y1, z1)
    elif hinge == "w":
        pts = east_face(CH_Y0, CH_Y1, CH_X0, z0, z1)
        top_a, top_b = (CH_X0, CH_Y0, z1), (CH_X0, CH_Y1, z1)
    elif hinge == "s":
        pts = south_face(CH_X0, CH_X1, CH_Y0, z0, z1)
        top_a, top_b = (CH_X0, CH_Y0, z1), (CH_X1, CH_Y0, z1)
    else:  # 'e'
        pts = east_face(CH_Y0, CH_Y1, CH_X1, z0, z1)
        top_a, top_b = (CH_X1, CH_Y0, z1), (CH_X1, CH_Y1, z1)
    fill3(img, pts, anchor, base)

    # Plank seams + edge frame on the raised lid.
    if hinge in ("n", "s"):
        y = CH_Y1 if hinge == "n" else CH_Y0
        for k in range(1, 4):
            u = CH_X0 + k * (CH_X1 - CH_X0) / 4
            line3(img, (u, y, z0 + 0.03), (u, y, z1 - 0.03), anchor,
                  _darken(base, 22))
        line3(img, (CH_X0, y, z0), (CH_X0, y, z1), anchor, _darken(base, 30))
        line3(img, (CH_X1, y, z0), (CH_X1, y, z1), anchor, _darken(base, 30))
    else:
        x = CH_X0 if hinge == "w" else CH_X1
        for k in range(1, 4):
            u = CH_Y0 + k * (CH_Y1 - CH_Y0) / 4
            line3(img, (x, u, z0 + 0.03), (x, u, z1 - 0.03), anchor,
                  _darken(base, 22))
        line3(img, (x, CH_Y0, z0), (x, CH_Y0, z1), anchor, _darken(base, 30))
        line3(img, (x, CH_Y1, z0), (x, CH_Y1, z1), anchor, _darken(base, 30))
    line3(img, top_a, top_b, anchor, _lighten(base, 26))

    if not interior:
        # We see the lid's outer shell — keep its iron straps visible.
        if hinge == "s":
            draw_iron_band_on_face(img, anchor, "s", CH_Y0, CH_X0, CH_X1,
                                   z0 + 0.28, z0 + 0.34)
        else:
            draw_iron_band_on_face(img, anchor, "e", CH_X1, CH_Y0, CH_Y1,
                                   z0 + 0.28, z0 + 0.34)


def make_chest(chest_id, facing, state, cw, ch, anchor):
    img = Image.new("RGBA", (cw, ch), BG)
    if state == "closed":
        draw_chest_body(img, anchor, chest_id, facing, lid=True)
        outline_silhouette(img)
        return img

    hinge = HINGE_FOR_FACING[facing]
    if hinge in ("n", "w"):
        # Lid stands at the back — draw it first, body in front.
        draw_open_lid(img, anchor, chest_id, facing)
        draw_chest_body(img, anchor, chest_id, facing, lid=False)
        draw_open_interior(img, anchor)
    else:
        draw_chest_body(img, anchor, chest_id, facing, lid=False)
        draw_open_interior(img, anchor)
        draw_open_lid(img, anchor, chest_id, facing)  # in front of the opening
    outline_silhouette(img)
    return img


def draw_open_interior(img, anchor):
    """Top opening: wood rim around a dark interior."""
    rim = 0.05
    fill3(img, top_face(CH_X0, CH_Y0, CH_X1, CH_Y1, BODY_TOP), anchor, WOOD_DARK)
    fill3(
        img,
        top_face(CH_X0 + rim, CH_Y0 + rim, CH_X1 - rim, CH_Y1 - rim, BODY_TOP),
        anchor,
        INTERIOR,
    )
    # A couple of glinting coins so the open chest reads as a container.
    for (gx, gy, c) in ((0.42, 0.40, RING), (0.55, 0.52, RING_HI), (0.60, 0.36, RING)):
        p = project(gx, gy, BODY_TOP, anchor)
        if 0 <= p[0] < img.width and 0 <= p[1] < img.height:
            img.putpixel(p, c)
            if p[0] + 1 < img.width:
                img.putpixel((p[0] + 1, p[1]), c)


# ═════════════════════════════════ BARREL ═══════════════════════════════
# The barrel deliberately SOFTENS the strict projection. A faithful
# 1-floor-tall cylinder under the (-36, -24)px/floor shear reads as a log
# lying on the ground — circles carry no orientation cues the way a box's
# straight edges do (we tried: strict circles, slimmer, tapered, faceted;
# all read as lying). So the barrel is drawn as a classic standing barrel:
# elliptical rims (0.62 vertical squash = a side-ish viewing angle) and a
# gentle up-left lean toward the wall shear. Trade-off: an object stacked
# on the barrel (renderer offset = full floor shear) sits slightly up-left
# of the drawn lid instead of flush — much less wrong than a lying barrel.
BARREL_R_END = 0.36    # rim radius at base/top (tile units)
BARREL_R_MID = 0.41    # bulge radius at half height
BARREL_ELL = 0.62      # vertical squash of the rim ellipses
BARREL_LEAN = (-12.0, -27.0)  # screen offset of the top rim vs the base rim
BARREL_STAVES = 10
HOOP_BANDS = ((0.14, 0.24), (0.72, 0.82))


def barrel_radius(t):
    return BARREL_R_END + (BARREL_R_MID - BARREL_R_END) * math.sin(math.pi * t)


def barrel_canvas():
    """Custom canvas math (the barrel is drawn in screen space, not through
    `project`). Mirrors canvas_for_content's convention: tile south-center
    at canvas (cw/2, ch-1); the base rim sits on the tile center, 24px up."""
    r_max = max(barrel_radius(t) for t in (0.0, 0.5, 1.0)) * TILE_PX
    lean_x, lean_y = BARREL_LEAN
    # Horizontal content extent relative to canvas center.
    dx_min = min(-r_max, lean_x - r_max) - 2
    dx_max = max(r_max, lean_x + r_max) + 2
    cw = max(int(-2 * dx_min), int(2 * dx_max + 1), TILE_PX)
    # Vertical: base center is 24px above the canvas bottom (tile center).
    base_cy_off = TILE_PX // 2
    top_edge = base_cy_off - lean_y + r_max * BARREL_ELL + 2
    ch = int(top_edge) + 1
    base_center = (cw // 2, ch - 1 - base_cy_off)
    return cw, ch, base_center


def make_barrel(cw, ch, base_center):
    img = Image.new("RGBA", (cw, ch), BG)
    bx, by = base_center
    lean_x, lean_y = BARREL_LEAN

    def center(t):
        return (bx + lean_x * t, by + lean_y * t)

    # Ground shadow hugging the base rim.
    rs = barrel_radius(0.0) * TILE_PX + 3
    for y in range(ch):
        for x in range(cw):
            dx = (x - bx) / rs
            dy = (y - by) / (rs * BARREL_ELL)
            d = dx * dx + dy * dy
            if d <= 1.0:
                img.putpixel((x, y), (0, 0, 0, SHADOW_A if d <= 0.72 else SHADOW_A // 2))

    # Sweep of squashed ellipses; every pixel keeps the highest slice t.
    steps = 200
    tmap = {}
    for i in range(steps + 1):
        t = i / steps
        cx, cy = center(t)
        rx = barrel_radius(t) * TILE_PX
        ry = rx * BARREL_ELL
        for y in range(max(int(cy - ry) - 1, 0), min(int(cy + ry) + 1, ch - 1) + 1):
            for x in range(max(int(cx - rx) - 1, 0), min(int(cx + rx) + 1, cw - 1) + 1):
                ddx = (x - cx) / rx
                ddy = (y - cy) / ry
                if ddx * ddx + ddy * ddy <= 1.0:
                    tmap[(x, y)] = t

    tcx, tcy = center(1.0)
    trx = barrel_radius(1.0) * TILE_PX
    try_ = trx * BARREL_ELL
    phi_light = math.radians(115)
    stw = 2 * math.pi / BARREL_STAVES

    for (xy, t) in tmap.items():
        x, y = xy
        ddx = (x - tcx) / trx
        ddy = (y - tcy) / try_
        if ddx * ddx + ddy * ddy <= 1.0:
            continue  # lid pixel — drawn below
        cx, cy = center(t)
        phi = math.atan2((y - cy) / BARREL_ELL, x - cx)
        stave = int((phi + math.pi) / stw)
        if any(b0 <= t <= b1 for (b0, b1) in HOOP_BANDS):
            hoop_mid = any(abs(t - (b0 + b1) / 2) < 0.035 for (b0, b1) in HOOP_BANDS)
            color = IRON_HI if hoop_mid else IRON
        else:
            color = hash_shade(f"barrel:stave:{stave}", WOOD, steps=(-14, -6, 4, 12))
            frac = ((phi + math.pi) % stw) / stw
            if frac < 0.10:
                color = _darken(color, 26)  # stave seam along the axis
        lit = math.cos(phi - phi_light)
        f = 1.0 + 0.20 * lit
        r, g, b, a = color
        img.putpixel((x, y), (
            max(0, min(255, int(r * f))),
            max(0, min(255, int(g * f))),
            max(0, min(255, int(b * f))),
            255,
        ))

    # Lid: straight boards, lit up-left rim, iron bung.
    for y in range(max(int(tcy - try_) - 1, 0), min(int(tcy + try_) + 1, ch - 1) + 1):
        for x in range(max(int(tcx - trx) - 1, 0), min(int(tcx + trx) + 1, cw - 1) + 1):
            ddx = (x - tcx) / trx
            ddy = (y - tcy) / try_
            d2 = ddx * ddx + ddy * ddy
            if d2 > 1.0:
                continue
            if d2 > 0.86:
                phi = math.atan2(ddy, ddx)
                lit = math.cos(phi - math.radians(205)) > 0.15
                img.putpixel((x, y), _lighten(WOOD_HI, 18) if lit else WOOD_DARK)
                continue
            row = int((ddy + 1.0) / 2.0 * 4)
            shade = hash_shade(f"barrel:lid:{row}", WOOD_HI, steps=(-12, -5, 2, 9))
            if ((ddy + 1.0) / 2.0 * 4) % 1.0 < 0.14:
                shade = _darken(shade, 20)
            img.putpixel((x, y), shade)
    for (ox, oy) in ((0, 0), (1, 0), (0, 1), (1, 1)):
        x, y = int(tcx + 4) + ox, int(tcy - 2) + oy
        if 0 <= x < cw and 0 <= y < ch:
            img.putpixel((x, y), IRON_DARK)

    outline_silhouette(img)
    return img


BARREL_META = """extends: movable_obstacle
name: Barrel
description: A heavy wooden barrel that can be opened as a simple container.
container_capacity: 8
rotatable: true
# Heavy enough that shoving it (push/pull) requires an Athletics check vs this
# weight — see docs/utility_systems.md §4. As a full-block, walkable collider it
# also doubles as cover, a barricade, and a stack-to-climb step once pushed.
weight: 12.0
render:
  z_index: 0.25
  debug_color: [134, 83, 42]
  debug_size: 0.62
  sprite_path: overworld_objects/barrel/sprite.png
  sprite_width_tiles: {w_tiles}
  sprite_height_tiles: {h_tiles}
  block_size: 2
  walkable_surface: true
  stack_order: 10
"""


# ═════════════════════════════════ CRATE ════════════════════════════════
CR_X0, CR_X1 = 0.08, 0.92
CR_Y0, CR_Y1 = 0.08, 0.92
CR_TOP = 1.0  # full block


def crate_canvas():
    corners = [
        (x, y, z)
        for x in (CR_X0, CR_X1)
        for y in (CR_Y0, CR_Y1)
        for z in (0.0, CR_TOP)
    ]
    return canvas_for_content(corners)


def draw_crate_panel(img, anchor, face_kind, pos, a0, a1, key):
    """One crate side: frame posts + horizontal planks + diagonal X-brace."""
    post = 0.09
    # Planks behind everything.
    draw_face_planks(img, anchor, face_kind, pos, a0, a1, 0.0, CR_TOP, 4,
                     key, base=PINE)
    # Frame posts on both ends and rails top/bottom.
    for (p0, p1) in ((a0, a0 + post), (a1 - post, a1)):
        if face_kind == "s":
            pts = south_face(p0, p1, pos, 0.0, CR_TOP)
        else:
            pts = east_face(p0, p1, pos, 0.0, CR_TOP)
        fill3(img, pts, anchor, hash_shade(f"{key}:post:{p0:.2f}", PINE_DARK,
                                           steps=(-8, -3, 3, 8)))
    for (z0, z1) in ((0.0, 0.07), (CR_TOP - 0.07, CR_TOP)):
        if face_kind == "s":
            pts = south_face(a0, a1, pos, z0, z1)
        else:
            pts = east_face(a0, a1, pos, z0, z1)
        fill3(img, pts, anchor, PINE_DARK)
    # X-brace across the inner panel.
    i0, i1 = a0 + post, a1 - post
    if face_kind == "s":
        c0, c1 = (i0, pos, 0.08), (i1, pos, CR_TOP - 0.08)
        c2, c3 = (i0, pos, CR_TOP - 0.08), (i1, pos, 0.08)
    else:
        c0, c1 = (pos, i0, 0.08), (pos, i1, CR_TOP - 0.08)
        c2, c3 = (pos, i0, CR_TOP - 0.08), (pos, i1, 0.08)
    for (pa, pb) in ((c0, c1), (c2, c3)):
        line3(img, pa, pb, anchor, PINE_GRAIN)
        # 2px-thick brace: offset copy one pixel up.
        a = project(*pa, anchor)
        b = project(*pb, anchor)
        _line(img, a[0], a[1] - 1, b[0], b[1] - 1, PINE_GRAIN)
    # Nails at the brace corners.
    for p3 in (c0, c1, c2, c3):
        x, y = project(*p3, anchor)
        if 0 <= x < img.width and 0 <= y < img.height:
            img.putpixel((x, y), IRON_DARK)


def make_crate(cw, ch, anchor):
    img = Image.new("RGBA", (cw, ch), BG)
    ground_shadow(img, anchor, 0.5, 0.5, 0.50, 0.32)

    draw_crate_panel(img, anchor, "e", CR_X1, CR_Y0, CR_Y1, "crate:e")
    draw_crate_panel(img, anchor, "s", CR_Y0, CR_X0, CR_X1, "crate:s")

    # Top: edge frame + planks running east-west.
    frame = 0.07
    n_strips = 4
    dy = (CR_Y1 - CR_Y0) / n_strips
    for k in range(n_strips):
        y0 = CR_Y0 + k * dy
        shade = hash_shade(f"crate:top:{k}", PINE_HI, steps=(-14, -7, 0, 7))
        fill3(img, top_face(CR_X0, y0, CR_X1, y0 + dy, CR_TOP), anchor, shade)
        if k > 0:
            line3(img, (CR_X0, y0, CR_TOP), (CR_X1, y0, CR_TOP), anchor,
                  _darken(shade, 24))
    for pts in (
        top_face(CR_X0, CR_Y0, CR_X1, CR_Y0 + frame, CR_TOP),
        top_face(CR_X0, CR_Y1 - frame, CR_X1, CR_Y1, CR_TOP),
        top_face(CR_X0, CR_Y0, CR_X0 + frame, CR_Y1, CR_TOP),
        top_face(CR_X1 - frame, CR_Y0, CR_X1, CR_Y1, CR_TOP),
    ):
        fill3(img, pts, anchor, PINE_DARK)
    # Lit west edge of the top (light from up-left, like the walls).
    line3(img, (CR_X0, CR_Y0, CR_TOP), (CR_X0, CR_Y1, CR_TOP), anchor, PINE_HI)
    line3(img, (CR_X0, CR_Y1, CR_TOP), (CR_X1, CR_Y1, CR_TOP), anchor, PINE_HI)

    outline_silhouette(img)
    return img


CRATE_META = """extends: movable_obstacle
name: Crate
description: A sturdy wooden shipping crate. Can be opened, shoved, and climbed.
container_capacity: 10
rotatable: true
weight: 10.0
render:
  z_index: 0.25
  debug_color: [168, 128, 76]
  debug_size: 0.7
  sprite_path: overworld_objects/crate/sprite.png
  sprite_width_tiles: {w_tiles}
  sprite_height_tiles: {h_tiles}
  block_size: 2
  walkable_surface: true
  stack_order: 15
"""


# ── Metadata emission ────────────────────────────────────────────────────
CHEST_META = """extends: movable_obstacle
name: {name}
description: {description}
container_capacity: 12
colliding: true
render:
  z_index: 0.25
  debug_color: [120, 120, 130]
  debug_size: 0.78
  sprite_path: overworld_objects/{id}/closed.png
  sprite_width_tiles: {w_tiles}
  sprite_height_tiles: {h_tiles}
  block_size: 1
  walkable_surface: true
  stack_order: 20
states:
  locked:
    sprite_path: overworld_objects/{id}/closed.png
  closed:
    sprite_path: overworld_objects/{id}/closed.png
  open:
    sprite_path: overworld_objects/{id}/open.png
initial_state: closed
lock:
  lock_id: 7
  pick_dc: 12
  force_dc: 20
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
"""


def _fmt_tiles(px_value):
    """Format a tile count as either an integer-valued float (`2.0`) or a
    decimal with up to 3 dp, trimming trailing zeros while keeping `.0`."""
    v = px_value / TILE_PX
    if v == int(v):
        return f"{int(v)}.0"
    return f"{v:.3f}".rstrip("0").rstrip(".")


def write_file(path, content):
    with open(path, "w") as f:
        f.write(content)
    print(f"  metadata: {path}")


def main():
    # Chests: one shared canvas for all facings × states.
    cw, ch, anchor = chest_canvas()
    for spec in CHEST_SPECS:
        dir_path = os.path.join(ASSETS_DIR, spec["id"])
        os.makedirs(dir_path, exist_ok=True)
        for state in ("closed", "open"):
            img = make_chest(spec["id"], spec["facing"], state, cw, ch, anchor)
            img.save(os.path.join(dir_path, f"{state}.png"))
        print(f"Saved {dir_path}/closed.png + open.png  ({cw}×{ch})")
        write_file(
            os.path.join(dir_path, "metadata.yaml"),
            CHEST_META.format(
                id=spec["id"],
                name=spec["name"],
                description=spec["description"],
                w_tiles=_fmt_tiles(cw),
                h_tiles=_fmt_tiles(ch),
            ),
        )

    # Barrel.
    cw, ch, anchor = barrel_canvas()
    dir_path = os.path.join(ASSETS_DIR, "barrel")
    os.makedirs(dir_path, exist_ok=True)
    make_barrel(cw, ch, anchor).save(os.path.join(dir_path, "sprite.png"))
    print(f"Saved {dir_path}/sprite.png  ({cw}×{ch})")
    write_file(
        os.path.join(dir_path, "metadata.yaml"),
        BARREL_META.format(w_tiles=_fmt_tiles(cw), h_tiles=_fmt_tiles(ch)),
    )

    # Crate.
    cw, ch, anchor = crate_canvas()
    dir_path = os.path.join(ASSETS_DIR, "crate")
    os.makedirs(dir_path, exist_ok=True)
    make_crate(cw, ch, anchor).save(os.path.join(dir_path, "sprite.png"))
    print(f"Saved {dir_path}/sprite.png  ({cw}×{ch})")
    write_file(
        os.path.join(dir_path, "metadata.yaml"),
        CRATE_META.format(w_tiles=_fmt_tiles(cw), h_tiles=_fmt_tiles(ch)),
    )


if __name__ == "__main__":
    main()
