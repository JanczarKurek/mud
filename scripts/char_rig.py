"""
Shared 4-facing oblique character rig (see docs/sprite_style.md).

Characters are stacks of 3D boxes in the shared wall_perspective projection:
south front face, shadowed east face, lit top cap — the same 3-tone-per-region
treatment as the wall set and gen_player_sheet.py. This module generalises that
script's approach into a data-driven rig:

  * a character is a list of PARTS: {fp, fz, colors, swing_key, dz_key,
    fy_shift_key} where fp is the canonical SOUTH-facing xy footprint
    (tile units, whole body symmetric about (0.5, 0.5)) and fz the height band
    in floors;
  * per-frame animation params (foot swings, body bob, blink…) are a plain
    dict; each part picks its params via *_key names;
  * facings are true rotations about the tile centre — south → east maps
    (x, y) → (1-y, x) so a quadruped's head really points east (note:
    gen_player_sheet.py's e/w rotations are mirrored, which its symmetric
    body hides);
  * all parts are painted in one global painter's pass sorted by
    (-fy, fx, fz_min): farther north first, then west, then lower boxes.

Builders `humanoid_parts()` and `quadruped_parts()` cover the two body plans;
`assemble()` renders the standard 4×8 sheet (idle_s, walk_s, idle_n, walk_n,
idle_e, walk_e, idle_w, walk_w — matches the metadata clip convention).

Frame sizes: 96×96 humanoids, 96×64 quadrupeds, ~128×112 for boss-scale
bodies (the frame must absorb the head's 36 px/floor up-LEFT lean; anchor is
always `(W//2 - TILE_PX//2, H-1)` so `project(0.5, 0, 0)` hits the frame's
bottom-center).
"""

import os

from PIL import Image

from wall_perspective import TILE_PX, BG, project, fill_polygon, _line


# ── Basics ───────────────────────────────────────────────────────────────────

def anchor_for(frame_w, frame_h):
    """PIL pixel of the 3D origin so the tile-south-center hits (W/2, H-1)."""
    return (frame_w // 2 - TILE_PX // 2, frame_h - 1)


def scale_color(rgba, k):
    r, g, b, a = rgba
    return (
        max(0, min(255, int(r * k))),
        max(0, min(255, int(g * k))),
        max(0, min(255, int(b * k))),
        a,
    )


def triple(base):
    """Derive the (front, east-side, top-cap) triple from one base colour."""
    return (base, scale_color(base, 0.70), scale_color(base, 1.24))


def _px(img, x, y, color):
    if 0 <= x < img.width and 0 <= y < img.height:
        img.putpixel((x, y), color)


def draw_box(img, anchor, fx0, fx1, fy0, fy1, fz0, fz1, colors):
    """One 3D box: south front, east side, top cap, wall-style edge strokes."""
    c_front, c_side, c_top = colors
    bsw = project(fx0, fy0, fz0, anchor)
    bse = project(fx1, fy0, fz0, anchor)
    bne = project(fx1, fy1, fz0, anchor)
    tsw = project(fx0, fy0, fz1, anchor)
    tse = project(fx1, fy0, fz1, anchor)
    tne = project(fx1, fy1, fz1, anchor)
    tnw = project(fx0, fy1, fz1, anchor)

    fill_polygon(img, [bsw, bse, tse, tsw], c_front)
    fill_polygon(img, [bse, bne, tne, tse], c_side)
    fill_polygon(img, [tsw, tse, tne, tnw], c_top)

    dark = scale_color(c_front, 0.55)
    light = scale_color(c_front, 1.18)
    _line(img, bsw[0], bsw[1], bse[0], bse[1], dark)
    _line(img, bse[0], bse[1], bne[0], bne[1], dark)
    _line(img, bsw[0], bsw[1], tsw[0], tsw[1], light)
    _line(img, bse[0], bse[1], tse[0], tse[1], dark)
    _line(img, bne[0], bne[1], tne[0], tne[1], dark)


# ── Facing transforms (about the tile centre (0.5, 0.5)) ─────────────────────

def rotate_xy(box, facing):
    fx0, fx1, fy0, fy1 = box
    if facing == "s":
        return (fx0, fx1, fy0, fy1)
    if facing == "n":                       # 180°
        return (1.0 - fx1, 1.0 - fx0, 1.0 - fy1, 1.0 - fy0)
    if facing == "e":                       # south → east: (x, y) → (1-y, x)
        return (1.0 - fy1, 1.0 - fy0, fx0, fx1)
    if facing == "w":                       # south → west: (x, y) → (y, 1-x)
        return (fy0, fy1, 1.0 - fx1, 1.0 - fx0)
    raise ValueError(f"bad facing: {facing}")


def swing_xy(box, facing, swing):
    """Slide a footprint by `swing` tiles along the facing (forward) axis."""
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


# ── Frame rendering ──────────────────────────────────────────────────────────

def render_frame(cfg, facing, frame):
    """Render one frame of `cfg` at `facing` with animation params `frame`.

    cfg keys:
      frame_w, frame_h    canvas size
      parts               list of part dicts (see module docstring)
      face                optional face spec (see paint_face)
      post_paint          optional fn(img, facing, frame, anchor) for props
    """
    img = Image.new("RGBA", (cfg["frame_w"], cfg["frame_h"]), BG)
    anchor = anchor_for(cfg["frame_w"], cfg["frame_h"])

    placed = []
    for part in cfg["parts"]:
        xy = rotate_xy(part["fp"], facing)
        swing = frame.get(part.get("swing_key") or "", 0.0)
        if swing:
            xy = swing_xy(xy, facing, swing)
        dz = frame.get(part.get("dz_key") or "", 0.0)
        shift = frame.get(part.get("fy_shift_key") or "", 0.0)
        if shift:
            xy = (xy[0], xy[1], xy[2] + shift, xy[3] + shift)
        fz0, fz1 = part["fz"]
        placed.append((xy, fz0 + dz, fz1 + dz, part["colors"]))

    # Painter's algorithm: north-most first, then west-most, then lowest.
    # Depth is quantized to 0.1-tile buckets so sub-pixel animation shifts
    # (hair sway, breathing) can't flip the order of stacked parts — within a
    # bucket the lower box paints first (head before hair), and the sort is
    # stable so list order breaks exact ties.
    def _bucket(v):
        return int(v * 10.0 + 0.5)

    placed.sort(key=lambda p: (-_bucket((p[0][2] + p[0][3]) / 2),
                               _bucket((p[0][0] + p[0][1]) / 2),
                               p[1]))
    for (fx0, fx1, fy0, fy1), fz0, fz1, colors in placed:
        draw_box(img, anchor, fx0, fx1, fy0, fy1, fz0, fz1, colors)

    if cfg.get("face"):
        paint_face(img, anchor, cfg["face"], facing, frame)
    if cfg.get("post_paint"):
        cfg["post_paint"](img, facing, frame, anchor)
    return img


def paint_face(img, anchor, face, facing, frame):
    """Eyes (and optional mouth) on the visible head face.

    face keys: fp, fz (head box, canonical south), dz_key, eye_white,
    eye_pupil, mouth (colour or None), skin (for the blink line),
    style: "human" (2×2 eyes + mouth), "beast" (1-px eyes), or
    "cyclops" (one big central eye + brow; uses eye_iris if given).
    South-facing puts features on the south face; east-facing on the east
    face; north/west show the back of the head — nothing drawn.
    """
    if facing not in ("s", "e"):
        return
    dz = frame.get(face.get("dz_key") or "", 0.0)
    fx0, fx1, fy0, fy1 = rotate_xy(face["fp"], facing)
    fz0, fz1 = face["fz"][0] + dz, face["fz"][1] + dz
    head_h = fz1 - fz0
    eye_fz = fz0 + face.get("eye_h", 0.55) * head_h
    mouth_fz = fz0 + face.get("mouth_h", 0.25) * head_h
    lo, hi = face.get("eye_span", (0.28, 0.72))
    if face.get("style") == "cyclops":
        lo = hi = 0.5

    if facing == "s":
        pts = [(fx0 + t * (fx1 - fx0), fy0) for t in (lo, hi)]
        eyes = [project(x, y, eye_fz, anchor) for (x, y) in pts]
        mouth = [project(x, y, mouth_fz, anchor) for (x, y) in pts]
    else:
        pts = [(fx1, fy0 + t * (fy1 - fy0)) for t in (lo, hi)]
        eyes = [project(x, y, eye_fz, anchor) for (x, y) in pts]
        mouth = [project(x, y, mouth_fz, anchor) for (x, y) in pts]

    blink = frame.get("blink", False)
    for p in eyes:
        _draw_eye(img, p, blink, face)
    if face.get("mouth") and face.get("style", "human") == "human":
        _line(img, mouth[0][0], mouth[0][1], mouth[1][0], mouth[1][1],
              face["mouth"])


def _draw_eye(img, p, blink, face):
    x, y = p
    style = face.get("style", "human")
    if blink:
        shut = scale_color(face["skin"], 0.6)
        span = (-2, -1, 0, 1, 2) if style == "cyclops" else (-1, 0, 1)
        for dx in span:
            _px(img, x + dx, y, shut)
        return
    if style == "beast":
        _px(img, x, y, face["eye_pupil"])
        _px(img, x - 1, y, face["eye_white"])
        return
    if style == "cyclops":
        for dy in (-2, -1, 0, 1):
            for dx in (-2, -1, 0, 1):
                _px(img, x + dx, y + dy, face["eye_white"])
        iris = face.get("eye_iris", face["eye_pupil"])
        for dy in (-1, 0):
            for dx in (-1, 0):
                _px(img, x + dx, y + dy, iris)
        _px(img, x, y, face["eye_pupil"])
        brow = scale_color(face["skin"], 0.45)
        for dx in range(-3, 4):
            _px(img, x + dx, y - 3, brow)
        return
    for dy in (-1, 0):
        for dx in (-1, 0):
            _px(img, x + dx, y + dy, face["eye_white"])
    _px(img, x - 1, y, face["eye_pupil"])


# ── Standard animation frames ────────────────────────────────────────────────

def frame_dict(**overrides):
    out = dict(
        body_dz=0.0,
        l_foot_dz=0.0, r_foot_dz=0.0,
        l_foot_swing=0.0, r_foot_swing=0.0,
        l_arm_swing=0.0, r_arm_swing=0.0,
        fl_swing=0.0, fr_swing=0.0, hl_swing=0.0, hr_swing=0.0,
        hair_dy=0.0, tail_dy=0.0,
        blink=False,
    )
    out.update(overrides)
    return out


IDLE_FRAMES = [
    frame_dict(body_dz=0.0, hair_dy=0.0, blink=False),
    frame_dict(body_dz=-0.018, hair_dy=0.0, blink=False),
    frame_dict(body_dz=-0.018, hair_dy=0.01, blink=False),
    frame_dict(body_dz=0.0, hair_dy=0.0, blink=True),
]

WALK_FRAMES = [
    frame_dict(body_dz=-0.01,
               l_foot_swing=0.045, r_foot_swing=-0.045,
               l_foot_dz=0.020, r_foot_dz=0.000,
               l_arm_swing=-0.040, r_arm_swing=0.040),
    frame_dict(body_dz=0.025),
    frame_dict(body_dz=-0.01,
               l_foot_swing=-0.045, r_foot_swing=0.045,
               l_foot_dz=0.000, r_foot_dz=0.020,
               l_arm_swing=0.040, r_arm_swing=-0.040),
    frame_dict(body_dz=0.025),
]

# Quadruped gait: diagonal pairs (fore-left + hind-right, then the opposite).
QUAD_IDLE_FRAMES = [
    frame_dict(body_dz=0.0, tail_dy=0.0),
    frame_dict(body_dz=-0.012, tail_dy=0.0),
    frame_dict(body_dz=-0.012, tail_dy=0.015),
    frame_dict(body_dz=0.0, tail_dy=0.0, blink=True),
]

QUAD_WALK_FRAMES = [
    frame_dict(body_dz=-0.008,
               fl_swing=0.05, hr_swing=0.05, fr_swing=-0.05, hl_swing=-0.05),
    frame_dict(body_dz=0.015),
    frame_dict(body_dz=-0.008,
               fl_swing=-0.05, hr_swing=-0.05, fr_swing=0.05, hl_swing=0.05),
    frame_dict(body_dz=0.015),
]


# Row order matches the metadata clip convention (idle_s … walk_w).
def standard_rows(idle=None, walk=None):
    idle = idle or IDLE_FRAMES
    walk = walk or WALK_FRAMES
    return [(f, frames)
            for f in ("s", "n", "e", "w")
            for frames in (idle, walk)]


# ── Body-plan builders ───────────────────────────────────────────────────────

def _slim_fp(fp, slim):
    """Scale a footprint's fx extent about the tile centre 0.5."""
    fx0, fx1, fy0, fy1 = fp
    return (0.5 + (fx0 - 0.5) * slim, 0.5 + (fx1 - 0.5) * slim, fy0, fy1)


def humanoid_parts(*, skin, hair, top, pants, boots, belt=None, apron=None,
                   scale=1.0, slim=1.0):
    """Player-proportioned biped. Colour args are (front, side, top) triples
    (use `triple(base)` to derive). `scale` multiplies every fz band —
    0.7 ≈ goblin, 1.0 ≈ human, 1.5+ ≈ boss (use a larger frame). `slim`
    scales every footprint's width about the centre (0.8 ≈ skeletal).
    Returns (parts, face_spec); face colours must be filled by the caller
    via face_spec (skin base is already set)."""
    s = scale
    boot_top, pants_top, belt_top = 0.08 * s, 0.46 * s, 0.52 * s
    torso_top, neck_top = 0.86 * s, 0.91 * s
    head_top, hair_top = 1.08 * s, 1.10 * s
    arm_bottom, sleeve_bottom = 0.52 * s, 0.70 * s

    LEFT_LEG = _slim_fp((0.34, 0.45, 0.40, 0.60), slim)
    RIGHT_LEG = _slim_fp((0.55, 0.66, 0.40, 0.60), slim)
    TORSO = _slim_fp((0.32, 0.68, 0.38, 0.62), slim)
    NECK = _slim_fp((0.46, 0.54, 0.43, 0.57), slim)
    HEAD = _slim_fp((0.36, 0.64, 0.36, 0.64), slim)
    LEFT_ARM = _slim_fp((0.28, 0.34, 0.42, 0.58), slim)
    RIGHT_ARM = _slim_fp((0.66, 0.72, 0.42, 0.58), slim)

    parts = [
        # legs: boots then pant-legs, sharing swing/dz keys
        dict(fp=LEFT_LEG, fz=(0.0, boot_top), colors=boots,
             swing_key="l_foot_swing", dz_key="l_foot_dz"),
        dict(fp=RIGHT_LEG, fz=(0.0, boot_top), colors=boots,
             swing_key="r_foot_swing", dz_key="r_foot_dz"),
        dict(fp=LEFT_LEG, fz=(boot_top, pants_top), colors=pants,
             swing_key="l_foot_swing", dz_key="l_foot_dz"),
        dict(fp=RIGHT_LEG, fz=(boot_top, pants_top), colors=pants,
             swing_key="r_foot_swing", dz_key="r_foot_dz"),
        # belt band + torso
        dict(fp=TORSO, fz=(pants_top, belt_top),
             colors=belt or pants, dz_key="body_dz"),
        dict(fp=TORSO, fz=(belt_top, torso_top), colors=top, dz_key="body_dz"),
        # arms: forearm (skin) below, sleeve (top colour) above
        dict(fp=LEFT_ARM, fz=(arm_bottom, sleeve_bottom), colors=skin,
             swing_key="l_arm_swing", dz_key="body_dz"),
        dict(fp=RIGHT_ARM, fz=(arm_bottom, sleeve_bottom), colors=skin,
             swing_key="r_arm_swing", dz_key="body_dz"),
        dict(fp=LEFT_ARM, fz=(sleeve_bottom, torso_top), colors=top,
             swing_key="l_arm_swing", dz_key="body_dz"),
        dict(fp=RIGHT_ARM, fz=(sleeve_bottom, torso_top), colors=top,
             swing_key="r_arm_swing", dz_key="body_dz"),
        # neck, head, hair cap
        dict(fp=NECK, fz=(torso_top, neck_top), colors=skin, dz_key="body_dz"),
        dict(fp=HEAD, fz=(neck_top, head_top), colors=skin, dz_key="body_dz"),
    ]
    if hair:
        parts.append(dict(fp=HEAD, fz=(head_top, hair_top), colors=hair,
                          dz_key="body_dz", fy_shift_key="hair_dy"))
    if apron:
        # Thin slab proud of the torso front, hanging from chest to shin.
        parts.append(dict(fp=_slim_fp((0.35, 0.65, 0.345, 0.385), slim),
                          fz=(boot_top + 0.10 * s, torso_top - 0.08 * s),
                          colors=apron, dz_key="body_dz"))

    face = dict(fp=HEAD, fz=(neck_top, head_top), dz_key="body_dz",
                style="human", skin=skin[0])
    return parts, face


def quadruped_parts(*, fur, fur_dark, belly=None, scale=1.0, ears=True,
                    snout=None, tail="bushy"):
    """Four-legged body plan, canonical facing south (head toward -fy).
    `fur`/`fur_dark`/`belly`/`snout` are colour triples. `scale` multiplies
    heights: 0.5 ≈ rat, 1.0 ≈ wolf/dog. Returns (parts, face_spec)."""
    s = scale
    body_z0, body_z1 = 0.14 * s, 0.42 * s
    head_z0, head_z1 = 0.26 * s, 0.54 * s
    leg_z1 = body_z0 + 0.04 * s

    BODY = (0.36, 0.64, 0.30, 0.76)
    HEAD = (0.39, 0.61, 0.12, 0.32)
    SNOUT = (0.45, 0.55, 0.04, 0.13)
    EAR_L = (0.41, 0.47, 0.16, 0.24)
    EAR_R = (0.53, 0.59, 0.16, 0.24)
    LEG_FL = (0.38, 0.46, 0.32, 0.42)
    LEG_FR = (0.54, 0.62, 0.32, 0.42)
    LEG_HL = (0.38, 0.46, 0.62, 0.72)
    LEG_HR = (0.54, 0.62, 0.62, 0.72)
    TAIL = (0.46, 0.54, 0.76, 0.92)

    parts = [
        dict(fp=LEG_FL, fz=(0.0, leg_z1), colors=fur_dark, swing_key="fl_swing"),
        dict(fp=LEG_FR, fz=(0.0, leg_z1), colors=fur_dark, swing_key="fr_swing"),
        dict(fp=LEG_HL, fz=(0.0, leg_z1), colors=fur_dark, swing_key="hl_swing"),
        dict(fp=LEG_HR, fz=(0.0, leg_z1), colors=fur_dark, swing_key="hr_swing"),
        dict(fp=BODY, fz=(body_z0, body_z1), colors=fur, dz_key="body_dz"),
        dict(fp=HEAD, fz=(head_z0, head_z1), colors=fur, dz_key="body_dz"),
    ]
    if belly:
        parts.append(dict(fp=(BODY[0] + 0.02, BODY[1] - 0.02, BODY[2] - 0.015,
                              BODY[2] + 0.025),
                          fz=(body_z0 + 0.02 * s, body_z1 - 0.06 * s),
                          colors=belly, dz_key="body_dz"))
    if snout:
        parts.append(dict(fp=SNOUT, fz=(head_z0 + 0.03 * s, head_z0 + 0.15 * s),
                          colors=snout, dz_key="body_dz"))
    if ears:
        for fp in (EAR_L, EAR_R):
            parts.append(dict(fp=fp, fz=(head_z1, head_z1 + 0.10 * s),
                              colors=fur_dark, dz_key="body_dz"))
    if tail == "bushy":
        parts.append(dict(fp=TAIL, fz=(body_z0 + 0.04 * s, body_z1 - 0.02 * s),
                          colors=fur_dark, dz_key="body_dz",
                          fy_shift_key="tail_dy"))
    elif tail == "thin":
        parts.append(dict(fp=(0.48, 0.52, 0.76, 0.95),
                          fz=(0.02, 0.05 * s + 0.04),
                          colors=fur_dark, fy_shift_key="tail_dy"))

    face = dict(fp=HEAD, fz=(head_z0, head_z1), dz_key="body_dz",
                style="beast", skin=fur[0], eye_h=0.6, eye_span=(0.25, 0.75))
    return parts, face


# ── Sheet assembly ───────────────────────────────────────────────────────────

def assemble(cfg, rows=None, cols=4):
    """Render a sheet: `rows` is a list of (facing, frames) — default the
    standard 8-row 4-facing layout."""
    rows = rows or standard_rows()
    sheet = Image.new("RGBA", (cfg["frame_w"] * cols,
                               cfg["frame_h"] * len(rows)), BG)
    for row_idx, (facing, frames) in enumerate(rows):
        for col_idx in range(cols):
            frame = frames[col_idx % len(frames)]
            img = render_frame(cfg, facing, frame)
            sheet.paste(img, (col_idx * cfg["frame_w"],
                              row_idx * cfg["frame_h"]))
    return sheet


def save(image, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    image.save(path)
    print(f"Saved {path}  ({image.width}x{image.height})")
