"""
Generates assets/overworld_objects/player/sheet.png plus three recolor-layer
sheets under assets/overworld_objects/player/layers/, using the same oblique
3D projection as the wall set (see `scripts/wall_perspective.py`). The character
is composed of stacked 3D boxes whose top caps slant at the same angle as a
wall's lit cap band, so the player visually belongs to the same iso world.

Frame size is **96×96 px** (NOT the originally-discussed 64×96). The reason:
at the synced floor shift (`FLOOR_SHIFT_X_TILES = -0.75`), an iso-projected
character with a footprint centred on the tile and a head ~1.0 floor up needs
a canvas wide enough to absorb the head's up-LEFT lean (36 px / floor). 96 wide
just barely fits. Body footprints are designed symmetric about the tile centre
`(0.5, 0.5)` so 90° rotations leave the character standing on the same spot.

Sheet layout: 4 columns × 8 rows = 384 × 768 px.
  Row 0: idle_s  (south-facing idle)
  Row 1: walk_s  (south-facing walk)
  Row 2: idle_n
  Row 3: walk_n
  Row 4: idle_e
  Row 5: walk_e
  Row 6: idle_w
  Row 7: walk_w

Outputs:
  sheet.png            full-color base sheet (skin + tunic + pants + hair)
  layers/hair.png      hair-region pixels, white-tone palette (tintable)
  layers/torso.png     tunic-region pixels, white-tone palette (tintable)
  layers/trousers.png  pants-region pixels, white-tone palette (tintable)
  sprite_large.png     single south-facing idle frame (no animation), for
                       inventory icons / static fallback.

The four layer sheets share the base sheet's frame grid exactly — they are
multiplicatively tinted by `Sprite::color` at runtime, with the chosen
per-character RGB defining the visible hue.
"""

import os

from PIL import Image

from wall_perspective import (
    TILE_PX,
    BG,
    project,
    fill_polygon,
    _line,
)

# ── Frame & sheet geometry ────────────────────────────────────────────────────
FRAME_W = 96
FRAME_H = 96
COLS = 4
ROWS = 8

OUT_BASE = "assets/overworld_objects/player/sheet.png"
OUT_LARGE = "assets/overworld_objects/player/sprite_large.png"
OUT_LAYER_DIR = "assets/overworld_objects/player/layers"

# Per-frame anchor: bottom-center of the frame pins to the tile's south edge,
# matching `Anchor::BOTTOM_CENTER` + `anchor_y_offset = -tile_size * 0.5` in
# `src/world/systems.rs::sync_tile_transforms`. So `project(0.5, 0, 0)` must
# land at (FRAME_W/2, FRAME_H-1).
ANCHOR = (FRAME_W // 2 - TILE_PX // 2, FRAME_H - 1)


# ── Palette ───────────────────────────────────────────────────────────────────
# Each region uses three tones — front (base), side (shadow on east face),
# top (highlight on the lit cap), matching the wall stone palette pattern.

SKIN_BASE = (220, 170, 120, 255)
SKIN_SIDE = (180, 130,  85, 255)
SKIN_TOP  = (240, 195, 150, 255)

HAIR_BASE = (220, 180,  35, 255)
HAIR_SIDE = (170, 130,  20, 255)
HAIR_TOP  = (250, 215,  80, 255)

TUNIC_BASE = (145,  55, 165, 255)
TUNIC_SIDE = (100,  30, 120, 255)
TUNIC_TOP  = (175,  90, 200, 255)

PANTS_BASE = ( 55,  70, 105, 255)
PANTS_SIDE = ( 35,  48,  75, 255)
PANTS_TOP  = ( 80,  98, 140, 255)

BOOT_BASE = ( 72,  44,  18, 255)
BOOT_SIDE = ( 45,  28,  10, 255)
BOOT_TOP  = ( 95,  62,  28, 255)

BELT_BASE = (130,  85,  25, 255)
BELT_SIDE = ( 90,  55,  15, 255)
BELT_TOP  = (170, 120,  45, 255)

EYE_WHITE = (240, 240, 240, 255)
EYE_PUPIL = ( 20,  20,  30, 255)
MOUTH     = (110,  60,  45, 255)

# White-tone palette used by tintable layer sheets. Brightness ratios match
# the original tonal spread so the tinted result reads close to the base art.
LAYER_BASE = (220, 220, 220, 255)  # primary face
LAYER_SIDE = (160, 160, 160, 255)  # shadowed face
LAYER_TOP  = (255, 255, 255, 255)  # lit cap


# ── 3D box drawing ────────────────────────────────────────────────────────────
def draw_box(img, fx0, fx1, fy0, fy1, fz0, fz1, c_front, c_side, c_top):
    """Draw a 3D box: south face (front), east face (side), top face (cap).

    Edge strokes match the wall convention so character outlines read the same
    way as wall outlines: bottom seams shadowed, left vertical lit, right
    verticals shadowed.
    """
    # 8 corners in (PIL pixel) space.
    bsw = project(fx0, fy0, fz0, ANCHOR)
    bse = project(fx1, fy0, fz0, ANCHOR)
    bne = project(fx1, fy1, fz0, ANCHOR)
    bnw = project(fx0, fy1, fz0, ANCHOR)
    tsw = project(fx0, fy0, fz1, ANCHOR)
    tse = project(fx1, fy0, fz1, ANCHOR)
    tne = project(fx1, fy1, fz1, ANCHOR)
    tnw = project(fx0, fy1, fz1, ANCHOR)

    # Front face (fy = fy0) — camera-side.
    fill_polygon(img, [bsw, bse, tse, tsw], c_front)
    # East face (fx = fx1) — shadow side.
    fill_polygon(img, [bse, bne, tne, tse], c_side)
    # Top face (fz = fz1) — lit cap, same tilt as wall caps.
    fill_polygon(img, [tsw, tse, tne, tnw], c_top)

    # Edge strokes. Use a derived dark tone from the front color so each region
    # has its own coherent outline rather than a hard black.
    dark = _scale(c_front, 0.55)
    light = _scale(c_front, 1.18)
    # Bottom seam (south edge), shadowed.
    _line(img, bsw[0], bsw[1], bse[0], bse[1], dark)
    # Right-bottom edge (east at fz0), shadowed.
    _line(img, bse[0], bse[1], bne[0], bne[1], dark)
    # Left vertical (lit edge facing the light source).
    _line(img, bsw[0], bsw[1], tsw[0], tsw[1], light)
    # Right verticals (shadowed).
    _line(img, bse[0], bse[1], tse[0], tse[1], dark)
    _line(img, bne[0], bne[1], tne[0], tne[1], dark)


def _scale(rgba, k):
    r, g, b, a = rgba
    return (
        max(0, min(255, int(r * k))),
        max(0, min(255, int(g * k))),
        max(0, min(255, int(b * k))),
        a,
    )


# ── Body model ────────────────────────────────────────────────────────────────
# Footprint coords are in tile units (fx east, fy north). Canonical pose is
# SOUTH-facing.
#
# EVERY footprint here is symmetric about the tile centre (0.5, 0.5). That
# matters for two reasons:
#   1) Rotation about (0.5, 0.5) leaves the body centred on the tile — the
#      character doesn't drift toward an edge as it turns.
#   2) The east-facing case in particular needs its head not to lean off the
#      left of the canvas; a centred footprint stays well inside the frame.
#
# fz_max is kept at ~1.1: at FLOOR_SHIFT_X_TILES = -0.75 the head leans 36 px
# per floor. With fx_min = 0.36 (head's west edge) and anchor_x = 24, the head
# top corner is at px = 24 + 0.36*48 - fz*36 = 41.3 - 36*fz. For fz ≤ 1.1
# this stays positive — i.e. the head fits in the 96-wide canvas.

# Stacked fz bands.
BOOT_TOP_FZ      = 0.08
PANTS_TOP_FZ     = 0.46
BELT_TOP_FZ      = 0.52
TORSO_TOP_FZ     = 0.86
NECK_TOP_FZ      = 0.91
HEAD_TOP_FZ      = 1.08
HAIR_TOP_FZ      = 1.10
# Arm spans roughly the upper torso: forearm (skin) below, sleeve (tunic) above.
ARM_BOTTOM_FZ    = 0.52
SLEEVE_BOTTOM_FZ = 0.70

# Canonical xy footprints (fx0, fx1, fy0, fy1). All symmetric about (0.5, 0.5).
# Legs sit side-by-side along fx (perpendicular to the south-facing direction);
# 90° rotation flips that to north-south spread for east/west facings.
LEFT_LEG   = (0.34, 0.45, 0.40, 0.60)
RIGHT_LEG  = (0.55, 0.66, 0.40, 0.60)
TORSO_FP   = (0.32, 0.68, 0.38, 0.62)
NECK_FP    = (0.46, 0.54, 0.43, 0.57)
HEAD_FP    = (0.36, 0.64, 0.36, 0.64)
LEFT_ARM   = (0.28, 0.34, 0.42, 0.58)
RIGHT_ARM  = (0.66, 0.72, 0.42, 0.58)


def rotate_xy(box, facing):
    """Rotate an xy footprint (fx0, fx1, fy0, fy1) about (0.5, 0.5) for facing."""
    fx0, fx1, fy0, fy1 = box
    if facing == "s":
        return (fx0, fx1, fy0, fy1)
    if facing == "n":
        return (1.0 - fx1, 1.0 - fx0, 1.0 - fy1, 1.0 - fy0)
    if facing == "e":
        # 90° CCW about (0.5, 0.5): (x, y) → (y, 1 - x)
        return (fy0, fy1, 1.0 - fx1, 1.0 - fx0)
    if facing == "w":
        # 90° CW about (0.5, 0.5): (x, y) → (1 - y, x)
        return (1.0 - fy1, 1.0 - fy0, fx0, fx1)
    raise ValueError(f"bad facing: {facing}")


def swing_xy(box, facing, swing):
    """Slide an xy footprint by `swing` (tile units) along the facing direction.

    Forward direction is the direction the character is *facing*: south-facing
    walks toward -fy, north toward +fy, east toward +fx, west toward -fx.
    """
    fx0, fx1, fy0, fy1 = box
    if facing == "s":
        return (fx0, fx1, fy0 - swing, fy1 - swing)
    if facing == "n":
        return (fx0, fx1, fy0 + swing, fy1 + swing)
    if facing == "e":
        return (fx0 + swing, fx1 + swing, fy0, fy1)
    if facing == "w":
        return (fx0 - swing, fx1 - swing, fy0, fy1)
    raise ValueError(f"bad facing: {facing}")


def _depth_key(fy_avg, fz_min):
    """Painter's-algorithm key. Bigger fy → farther from camera (drawn first).
    Bigger fz_min → higher (drawn later) so heads sit on top of torsos."""
    return (-fy_avg, fz_min)


# ── Region painters ───────────────────────────────────────────────────────────
# Each painter draws ONE region's boxes into `img`, parametrised by the colour
# triple so the base sheet and the white-tone layer sheets share geometry.
# Mirrors the per-region split from the original generator.

def paint_pants(img, *, facing, frame, c_base, c_side, c_top):
    """Pant legs (fz BOOT_TOP..PANTS_TOP) — RECOLORED region."""
    legs = [
        (LEFT_LEG, frame["l_foot_swing"], frame["l_foot_dz"]),
        (RIGHT_LEG, frame["r_foot_swing"], frame["r_foot_dz"]),
    ]
    # Back leg first (painter's algorithm — but they only overlap at swing
    # extremes; sort by depth post-rotation+swing).
    rendered = []
    for box, swing, dz in legs:
        xy = swing_xy(rotate_xy(box, facing), facing, swing)
        rendered.append((xy, dz))
    rendered.sort(key=lambda r: _depth_key((r[0][2] + r[0][3]) / 2, BOOT_TOP_FZ + r[1]))
    for xy, dz in rendered:
        fx0, fx1, fy0, fy1 = xy
        draw_box(
            img, fx0, fx1, fy0, fy1,
            BOOT_TOP_FZ + dz, PANTS_TOP_FZ + dz,
            c_base, c_side, c_top,
        )


def paint_tunic(img, *, facing, frame, c_base, c_side, c_top):
    """Torso, belt-position riser, and upper-arm sleeves — RECOLORED region."""
    dz = frame["body_dz"]
    fx0, fx1, fy0, fy1 = rotate_xy(TORSO_FP, facing)
    # Torso main box.
    draw_box(img, fx0, fx1, fy0, fy1,
             BELT_TOP_FZ + dz, TORSO_TOP_FZ + dz,
             c_base, c_side, c_top)
    # Sleeves: top portion of arm boxes (lower portion = forearm = skin).
    arms = [
        (LEFT_ARM, frame["l_arm_swing"]),
        (RIGHT_ARM, frame["r_arm_swing"]),
    ]
    rendered = []
    for box, swing in arms:
        xy = swing_xy(rotate_xy(box, facing), facing, swing)
        rendered.append(xy)
    rendered.sort(key=lambda xy: _depth_key((xy[2] + xy[3]) / 2, SLEEVE_BOTTOM_FZ))
    for fx0, fx1, fy0, fy1 in rendered:
        draw_box(img, fx0, fx1, fy0, fy1,
                 SLEEVE_BOTTOM_FZ + dz, TORSO_TOP_FZ + dz,
                 c_base, c_side, c_top)


def paint_hair(img, *, facing, frame, c_base, c_side, c_top):
    """Hair cap on top of head — RECOLORED region."""
    dz = frame["body_dz"]
    fx0, fx1, fy0, fy1 = rotate_xy(HEAD_FP, facing)
    # Slight northward sway: shift hair cap by a few tenths of a pixel in fy
    # via `hair_dy`. Subtle, only the top cap reads it.
    fy_shift = frame.get("hair_dy", 0.0)
    fy0 += fy_shift
    fy1 += fy_shift
    draw_box(img, fx0, fx1, fy0, fy1,
             HEAD_TOP_FZ + dz, HAIR_TOP_FZ + dz,
             c_base, c_side, c_top)


def paint_skin_and_accessories(img, *, facing, frame):
    """Boots, belt, forearms, neck, head, face features. NOT recolored."""
    dz = frame["body_dz"]

    # ── Boots ────────────────────────────────────────────────────────────────
    boots = [
        (LEFT_LEG, frame["l_foot_swing"], frame["l_foot_dz"]),
        (RIGHT_LEG, frame["r_foot_swing"], frame["r_foot_dz"]),
    ]
    rendered = []
    for box, swing, leg_dz in boots:
        xy = swing_xy(rotate_xy(box, facing), facing, swing)
        rendered.append((xy, leg_dz))
    rendered.sort(key=lambda r: _depth_key((r[0][2] + r[0][3]) / 2, 0.0 + r[1]))
    for xy, leg_dz in rendered:
        fx0, fx1, fy0, fy1 = xy
        draw_box(img, fx0, fx1, fy0, fy1,
                 0.0 + leg_dz, BOOT_TOP_FZ + leg_dz,
                 BOOT_BASE, BOOT_SIDE, BOOT_TOP)

    # ── Belt band ────────────────────────────────────────────────────────────
    fx0, fx1, fy0, fy1 = rotate_xy(TORSO_FP, facing)
    draw_box(img, fx0, fx1, fy0, fy1,
             PANTS_TOP_FZ + dz, BELT_TOP_FZ + dz,
             BELT_BASE, BELT_SIDE, BELT_TOP)

    # ── Forearms (lower part of arm boxes) ───────────────────────────────────
    arms = [
        (LEFT_ARM, frame["l_arm_swing"]),
        (RIGHT_ARM, frame["r_arm_swing"]),
    ]
    rendered = []
    for box, swing in arms:
        xy = swing_xy(rotate_xy(box, facing), facing, swing)
        rendered.append(xy)
    rendered.sort(key=lambda xy: _depth_key((xy[2] + xy[3]) / 2, ARM_BOTTOM_FZ))
    for fx0, fx1, fy0, fy1 in rendered:
        draw_box(img, fx0, fx1, fy0, fy1,
                 ARM_BOTTOM_FZ + dz, SLEEVE_BOTTOM_FZ + dz,
                 SKIN_BASE, SKIN_SIDE, SKIN_TOP)

    # ── Neck ─────────────────────────────────────────────────────────────────
    fx0, fx1, fy0, fy1 = rotate_xy(NECK_FP, facing)
    draw_box(img, fx0, fx1, fy0, fy1,
             TORSO_TOP_FZ + dz, NECK_TOP_FZ + dz,
             SKIN_BASE, SKIN_SIDE, SKIN_TOP)

    # ── Head ─────────────────────────────────────────────────────────────────
    fx0, fx1, fy0, fy1 = rotate_xy(HEAD_FP, facing)
    draw_box(img, fx0, fx1, fy0, fy1,
             NECK_TOP_FZ + dz, HEAD_TOP_FZ + dz,
             SKIN_BASE, SKIN_SIDE, SKIN_TOP)

    # ── Face features ────────────────────────────────────────────────────────
    paint_face_features(img, facing=facing, head_xy=(fx0, fx1, fy0, fy1),
                        head_fz=(NECK_TOP_FZ + dz, HEAD_TOP_FZ + dz),
                        blink=frame["blink"])


def paint_face_features(img, *, facing, head_xy, head_fz, blink):
    """Eyes and mouth, painted onto the camera-facing head face.

    For south-facing the face features go on the head's south face (fy=fy0);
    for east-facing they go on the east face (fx=fx1). North/west show the
    back of the head — no features drawn.
    """
    if facing not in ("s", "e"):
        return

    fx0, fx1, fy0, fy1 = head_xy
    fz0, fz1 = head_fz
    head_h = fz1 - fz0
    eye_fz = fz0 + 0.55 * head_h
    mouth_fz = fz0 + 0.25 * head_h

    if facing == "s":
        face_fy = fy0
        left_fx = fx0 + 0.28 * (fx1 - fx0)
        right_fx = fx0 + 0.72 * (fx1 - fx0)
        eye_left  = project(left_fx,  face_fy, eye_fz, ANCHOR)
        eye_right = project(right_fx, face_fy, eye_fz, ANCHOR)
        mouth_l   = project(left_fx,  face_fy, mouth_fz, ANCHOR)
        mouth_r   = project(right_fx, face_fy, mouth_fz, ANCHOR)
    else:  # "e"
        face_fx = fx1
        # Two eyes on the east face — pick fy values that span the face.
        back_fy  = fy0 + 0.30 * (fy1 - fy0)
        front_fy = fy0 + 0.70 * (fy1 - fy0)
        eye_left  = project(face_fx, back_fy,  eye_fz, ANCHOR)
        eye_right = project(face_fx, front_fy, eye_fz, ANCHOR)
        mouth_l   = project(face_fx, back_fy,  mouth_fz, ANCHOR)
        mouth_r   = project(face_fx, front_fy, mouth_fz, ANCHOR)

    _draw_eye(img, eye_left, blink)
    _draw_eye(img, eye_right, blink)
    _line(img, mouth_l[0], mouth_l[1], mouth_r[0], mouth_r[1], MOUTH)


def _draw_eye(img, p, blink):
    x, y = p
    if blink:
        for dx in (-1, 0, 1):
            _px(img, x + dx, y, _scale(SKIN_BASE, 0.6))
        return
    # 2×2 white pupil region with a 1-px dark pupil dot.
    for dy in (-1, 0):
        for dx in (-1, 0):
            _px(img, x + dx, y + dy, EYE_WHITE)
    _px(img, x - 1, y, EYE_PUPIL)


def _px(img, x, y, color):
    if 0 <= x < img.width and 0 <= y < img.height:
        img.putpixel((x, y), color)


# ── Frame builders ────────────────────────────────────────────────────────────

FRAME_DEFAULTS = dict(
    body_dz=0.0,
    l_foot_dz=0.0, r_foot_dz=0.0,
    l_foot_swing=0.0, r_foot_swing=0.0,
    l_arm_swing=0.0, r_arm_swing=0.0,
    hair_dy=0.0,
    blink=False,
)


def _frame(**overrides):
    out = dict(FRAME_DEFAULTS)
    out.update(overrides)
    return out


# Idle: subtle breathing + hair sway + blink on frame 3.
IDLE_FRAMES = [
    _frame(body_dz=0.0,    hair_dy=0.0,  blink=False),
    _frame(body_dz=-0.018, hair_dy=0.0,  blink=False),
    _frame(body_dz=-0.018, hair_dy=0.01, blink=False),
    _frame(body_dz=0.0,    hair_dy=0.0,  blink=True),
]

# Walk: 4-frame stride. Frames 1/3 are neutral with body lift; frames 0/2 are
# opposite swings.
WALK_FRAMES = [
    _frame(body_dz=-0.01,
           l_foot_swing= 0.045, r_foot_swing=-0.045,
           l_foot_dz=    0.020, r_foot_dz=    0.000,
           l_arm_swing= -0.040, r_arm_swing= 0.040),
    _frame(body_dz=0.025),
    _frame(body_dz=-0.01,
           l_foot_swing=-0.045, r_foot_swing= 0.045,
           l_foot_dz=    0.000, r_foot_dz=    0.020,
           l_arm_swing= 0.040,  r_arm_swing=-0.040),
    _frame(body_dz=0.025),
]


def make_base_frame(*, facing, frame):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    # Painter order: skin/accessories first (boots, neck, head, etc.), then
    # pants legs above boots, tunic above belt, hair on top. Each region's
    # boxes occupy disjoint fz bands so the order is well-defined.
    paint_skin_and_accessories(img, facing=facing, frame=frame)
    paint_pants(img, facing=facing, frame=frame,
                c_base=PANTS_BASE, c_side=PANTS_SIDE, c_top=PANTS_TOP)
    paint_tunic(img, facing=facing, frame=frame,
                c_base=TUNIC_BASE, c_side=TUNIC_SIDE, c_top=TUNIC_TOP)
    paint_hair(img, facing=facing, frame=frame,
               c_base=HAIR_BASE, c_side=HAIR_SIDE, c_top=HAIR_TOP)
    return img


def make_hair_layer_frame(*, facing, frame):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    paint_hair(img, facing=facing, frame=frame,
               c_base=LAYER_BASE, c_side=LAYER_SIDE, c_top=LAYER_TOP)
    return img


def make_torso_layer_frame(*, facing, frame):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    paint_tunic(img, facing=facing, frame=frame,
                c_base=LAYER_BASE, c_side=LAYER_SIDE, c_top=LAYER_TOP)
    return img


def make_trousers_layer_frame(*, facing, frame):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    paint_pants(img, facing=facing, frame=frame,
                c_base=LAYER_BASE, c_side=LAYER_SIDE, c_top=LAYER_TOP)
    return img


# ── Sheet assembly ────────────────────────────────────────────────────────────

# Row → (facing, frames) pairs, in the order that the metadata declares them.
SHEET_ROWS = [
    ("s", IDLE_FRAMES),
    ("s", WALK_FRAMES),
    ("n", IDLE_FRAMES),
    ("n", WALK_FRAMES),
    ("e", IDLE_FRAMES),
    ("e", WALK_FRAMES),
    ("w", IDLE_FRAMES),
    ("w", WALK_FRAMES),
]


def assemble(make_fn):
    sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
    for row_idx, (facing, frames) in enumerate(SHEET_ROWS):
        for col_idx, frame in enumerate(frames):
            img = make_fn(facing=facing, frame=frame)
            sheet.paste(img, (col_idx * FRAME_W, row_idx * FRAME_H))
    return sheet


def save(image, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    image.save(path)
    print(f"Saved {path}  ({image.width}×{image.height})")


def main():
    save(assemble(make_base_frame), OUT_BASE)
    save(assemble(make_hair_layer_frame),     os.path.join(OUT_LAYER_DIR, "hair.png"))
    save(assemble(make_torso_layer_frame),    os.path.join(OUT_LAYER_DIR, "torso.png"))
    save(assemble(make_trousers_layer_frame), os.path.join(OUT_LAYER_DIR, "trousers.png"))
    # Static fallback / inventory icon: single south-facing idle frame.
    save(make_base_frame(facing="s", frame=IDLE_FRAMES[0]), OUT_LARGE)


if __name__ == "__main__":
    main()
