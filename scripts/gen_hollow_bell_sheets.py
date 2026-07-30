"""
Generates the animated character sheets for the `hollow_bell` module on
scripts/char_rig.py (see docs/sprite_style.md). 4-facing oblique sheets,
4 cols × 8 rows (idle_s … walk_w), plus a static sprite.png each.

  beastfolk (96×96):  marten_coalbright (badger pit-captain),
                      sister_wick (mouse), tobin_ashfoot (fox),
                      hettie_marl (otter)
  ghost (96×96):      grandam_bellow (badger ghost — shares
                      ghost_badger_parts with gen_mill_ghost_sheet.py)
  creatures (96×96):  tallow_drip (wax blob), sump_crawler (centipede),
                      wax_wight (wax humanoid), seam_shrike (crystal bird),
                      hollow_spawn (eyeless mole-thing)
  bosses (136×112):   cinderjack (wax mound + clapper crown),
                      knell (rotating shard ring), deeplistener (vast mole)

Run under nix-shell:
  nix-shell -p python3Packages.pillow --run "python3 scripts/gen_hollow_bell_sheets.py"
"""

import math

from char_rig import (
    IDLE_FRAMES,
    WALK_FRAMES,
    _px,
    anchor_for,
    assemble,
    draw_box,
    frame_dict,
    humanoid_parts,
    render_frame,
    save,
    standard_rows,
    triple,
)
from gen_mill_ghost_sheet import ghost_badger_parts, DRIFT_IDLE, DRIFT_WALK
from wall_perspective import project, _line

MODULE_DIR = "assets/modules/hollow_bell/overworld_objects"

FRAME_W, FRAME_H = 96, 96
BOSS_W, BOSS_H = 136, 112


# ── Beastfolk (humanoid + muzzle + ears) ─────────────────────────────────────

EAR_SHAPES = {
    "round": [((0.38, 0.45, 0.44, 0.56), 0.07)],
    "big_round": [((0.33, 0.45, 0.42, 0.58), 0.11)],
    "point": [((0.39, 0.45, 0.45, 0.55), 0.13)],
    "flat": [((0.34, 0.43, 0.44, 0.56), 0.04)],
}


def beastfolk_cfg(*, fur, top, pants, boots, belt, ears, muzzle=None,
                  apron=None, eye=(24, 20, 18, 255), post_paint=None):
    """Upright beastfolk: humanoid frame at scale 0.85 with a muzzle box and
    species ears. Colour args are triples except `eye`."""
    s = 0.85
    parts, face = humanoid_parts(skin=fur, hair=fur, top=top, pants=pants,
                                 boots=boots, belt=belt, apron=apron, scale=s)
    parts.append(dict(fp=(0.44, 0.56, 0.30, 0.38),
                      fz=(0.93 * s, 1.00 * s),
                      colors=muzzle or fur, dz_key="body_dz"))
    for (fp, h) in EAR_SHAPES[ears]:
        fx0, fx1, fy0, fy1 = fp
        for mirrored in (False, True):
            e = (1.0 - fx1, 1.0 - fx0, fy0, fy1) if mirrored else fp
            parts.append(dict(fp=e, fz=(1.10 * s, (1.10 + h) * s),
                              colors=fur, dz_key="body_dz"))
    face.update(style="beast", eye_white=triple((255, 255, 255, 255))[0],
                eye_pupil=eye, eye_h=0.62, eye_span=(0.28, 0.72))
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=post_paint)


def _badger_stripes(img, facing, frame, anchor):
    if facing != "s":
        return
    dz = frame.get("body_dz", 0.0)
    for fx in (0.44, 0.56):
        a = project(fx, 0.355, 0.78 + dz, anchor)
        b = project(fx, 0.355, 0.93 + dz, anchor)
        _line(img, a[0], a[1], b[0], b[1], (120, 120, 126, 255))


def marten_cfg():
    return beastfolk_cfg(
        fur=triple((196, 196, 200, 255)), top=triple((44, 42, 46, 255)),
        pants=triple((28, 26, 30, 255)), boots=triple((36, 28, 22, 255)),
        belt=triple((178, 138, 56, 255)), ears="round",
        post_paint=_badger_stripes,
    )


def wick_cfg():
    return beastfolk_cfg(
        fur=triple((158, 132, 104, 255)), top=triple((196, 186, 162, 255)),
        pants=triple((150, 140, 118, 255)), boots=triple((36, 28, 22, 255)),
        belt=triple((120, 96, 62, 255)), ears="big_round",
    )


def tobin_cfg():
    return beastfolk_cfg(
        fur=triple((186, 92, 46, 255)), top=triple((96, 68, 42, 255)),
        pants=triple((66, 46, 28, 255)), boots=triple((36, 28, 22, 255)),
        belt=triple((140, 106, 58, 255)), ears="point",
        muzzle=triple((226, 214, 200, 255)), apron=triple((80, 56, 34, 255)),
    )


def hettie_cfg():
    return beastfolk_cfg(
        fur=triple((122, 96, 70, 255)), top=triple((118, 112, 88, 255)),
        pants=triple((84, 78, 60, 255)), boots=triple((36, 28, 22, 255)),
        belt=triple((70, 62, 46, 255)), ears="flat",
    )


def grandam_cfg():
    A = 200
    body = triple((150, 196, 224, A))
    parts, face = ghost_badger_parts(
        body=body, body_dk=triple((104, 150, 186, A - 10)),
        snout=triple((206, 232, 246, A)), apron=triple((206, 206, 196, A + 10)),
    )
    face.update(eye_white=(230, 240, 250, A + 40), eye_pupil=(40, 54, 80, A + 55))

    def stripes(img, facing, frame, anchor):
        if facing != "s":
            return
        dz = frame.get("body_dz", 0.0)
        for fx in (0.44, 0.56):
            a = project(fx, 0.355, 0.70 + dz, anchor)
            b = project(fx, 0.355, 0.90 + dz, anchor)
            _line(img, a[0], a[1], b[0], b[1], (96, 134, 168, A + 20))

    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=stripes)


# ── Creatures ────────────────────────────────────────────────────────────────

def tallow_drip_cfg():
    WAX = triple((214, 190, 92, 255))
    WAX_D = triple((162, 138, 58, 255))
    parts = [
        dict(fp=(0.28, 0.72, 0.28, 0.72), fz=(0.0, 0.12), colors=WAX_D,
             dz_key="body_dz"),
        dict(fp=(0.33, 0.67, 0.32, 0.68), fz=(0.10, 0.24), colors=WAX,
             dz_key="body_dz"),
        dict(fp=(0.40, 0.60, 0.38, 0.62), fz=(0.22, 0.34), colors=WAX,
             dz_key="body_dz"),
        dict(fp=(0.47, 0.53, 0.47, 0.53), fz=(0.36, 0.46),
             colors=triple((252, 176, 64, 255)), dz_key="flame_dz"),
    ]
    face = dict(fp=(0.40, 0.60, 0.38, 0.62), fz=(0.22, 0.34),
                dz_key="body_dz", style="beast", skin=WAX[0],
                eye_white=(254, 232, 150, 255), eye_pupil=(120, 84, 20, 255),
                eye_h=0.5)
    idle = [frame_dict(body_dz=0.0, flame_dz=0.0),
            frame_dict(body_dz=-0.012, flame_dz=0.03),
            frame_dict(body_dz=-0.012, flame_dz=-0.01),
            frame_dict(body_dz=0.0, flame_dz=0.04, blink=True)]
    walk = [frame_dict(body_dz=-0.02, flame_dz=0.02),
            frame_dict(body_dz=0.015, flame_dz=-0.01),
            frame_dict(body_dz=-0.02, flame_dz=0.04),
            frame_dict(body_dz=0.015, flame_dz=0.0)]
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face), \
        standard_rows(idle=idle, walk=walk)


def sump_crawler_cfg():
    SHELL = triple((206, 198, 176, 255))
    SHELL_D = triple((150, 144, 124, 255))
    parts = []
    for i, y0 in enumerate((0.12, 0.29, 0.46, 0.63)):
        parts.append(dict(fp=(0.41, 0.59, y0, y0 + 0.16),
                          fz=(0.0, 0.16),
                          colors=SHELL if i % 2 == 0 else SHELL_D,
                          dz_key=f"s{i}"))
    face = dict(fp=(0.41, 0.59, 0.12, 0.28), fz=(0.0, 0.16), dz_key="s0",
                style="beast", skin=SHELL[0],
                eye_white=(232, 226, 208, 255), eye_pupil=(60, 50, 40, 255),
                eye_h=0.65)
    def seg(*dzs):
        return frame_dict(**{f"s{i}": dz for i, dz in enumerate(dzs)})
    idle = [seg(0, 0, 0, 0), seg(0.01, 0, 0.01, 0),
            seg(0, 0.01, 0, 0.01), seg(0, 0, 0, 0)]
    walk = [seg(0.03, 0, 0.03, 0), seg(0.015, 0.015, 0.015, 0.015),
            seg(0, 0.03, 0, 0.03), seg(0.015, 0.015, 0.015, 0.015)]
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face), \
        standard_rows(idle=idle, walk=walk)


def wax_wight_cfg():
    WAX = triple((196, 168, 96, 255))
    APRON = triple((86, 62, 38, 255))
    parts, face = humanoid_parts(skin=WAX, hair=None, top=WAX, pants=APRON,
                                 boots=WAX, belt=triple((60, 44, 26, 255)),
                                 scale=0.9, slim=0.9)
    face.update(eye_white=(248, 150, 60, 255), eye_pupil=(120, 50, 10, 255),
                mouth=(120, 96, 48, 255), eye_h=0.6)

    def chest_glow(img, facing, frame, anchor):
        if facing != "s":
            return
        dz = frame.get("body_dz", 0.0)
        for (fx, fz) in ((0.48, 0.60), (0.52, 0.63), (0.50, 0.57)):
            x, y = project(fx, 0.395, fz * 0.9 + dz, anchor)
            _px(img, x, y, (248, 150, 60, 255))
            _px(img, x + 1, y, (200, 100, 30, 255))

    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=chest_glow), None


def seam_shrike_cfg():
    BODY = triple((92, 96, 104, 255))
    CRYST = triple((214, 228, 224, 255))
    parts = [
        dict(fp=(0.44, 0.47, 0.46, 0.52), fz=(0.0, 0.18),
             colors=triple((60, 64, 72, 255))),
        dict(fp=(0.53, 0.56, 0.46, 0.52), fz=(0.0, 0.18),
             colors=triple((60, 64, 72, 255))),
        dict(fp=(0.40, 0.60, 0.42, 0.64), fz=(0.16, 0.38), colors=BODY,
             dz_key="body_dz"),
        dict(fp=(0.44, 0.56, 0.64, 0.78), fz=(0.22, 0.32), colors=CRYST,
             dz_key="body_dz"),
        dict(fp=(0.24, 0.40, 0.44, 0.60), fz=(0.26, 0.34), colors=CRYST,
             dz_key="wing_dz"),
        dict(fp=(0.60, 0.76, 0.44, 0.60), fz=(0.26, 0.34), colors=CRYST,
             dz_key="wing_dz"),
        dict(fp=(0.42, 0.58, 0.30, 0.42), fz=(0.36, 0.52), colors=BODY,
             dz_key="body_dz"),
        dict(fp=(0.46, 0.54, 0.22, 0.31), fz=(0.40, 0.46),
             colors=triple((216, 200, 140, 255)), dz_key="body_dz"),
    ]
    face = dict(fp=(0.42, 0.58, 0.30, 0.42), fz=(0.36, 0.52),
                dz_key="body_dz", style="beast", skin=BODY[0],
                eye_white=(244, 250, 248, 255), eye_pupil=(150, 108, 40, 255),
                eye_h=0.6)
    idle = [frame_dict(body_dz=0.0, wing_dz=0.0),
            frame_dict(body_dz=-0.01, wing_dz=0.0),
            frame_dict(body_dz=-0.01, wing_dz=0.02),
            frame_dict(body_dz=0.0, wing_dz=0.0, blink=True)]
    walk = [frame_dict(body_dz=0.02, wing_dz=0.10),
            frame_dict(body_dz=0.04, wing_dz=0.02),
            frame_dict(body_dz=0.02, wing_dz=0.12),
            frame_dict(body_dz=0.04, wing_dz=0.04)]
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face), \
        standard_rows(idle=idle, walk=walk)


def hollow_spawn_cfg():
    EARTH = triple((58, 52, 48, 255))
    EARTH_H = triple((84, 74, 66, 255))
    CLAWS = triple((168, 158, 140, 255))
    parts = [
        dict(fp=(0.30, 0.70, 0.42, 0.72), fz=(0.0, 0.38), colors=EARTH,
             dz_key="body_dz"),
        dict(fp=(0.34, 0.66, 0.16, 0.48), fz=(0.12, 0.55), colors=EARTH,
             dz_key="body_dz"),
        dict(fp=(0.44, 0.56, 0.06, 0.18), fz=(0.24, 0.36), colors=EARTH_H,
             dz_key="body_dz"),
        dict(fp=(0.24, 0.38, 0.20, 0.34), fz=(0.0, 0.14), colors=CLAWS,
             swing_key="l_foot_swing"),
        dict(fp=(0.62, 0.76, 0.20, 0.34), fz=(0.0, 0.14), colors=CLAWS,
             swing_key="r_foot_swing"),
    ]

    def cracks(img, facing, frame, anchor):
        if facing != "s":
            return
        dz = frame.get("body_dz", 0.0)
        for (fx, fz) in ((0.42, 0.30), (0.55, 0.40), (0.48, 0.22),
                         (0.60, 0.28)):
            x, y = project(fx, 0.165, fz + dz, anchor)
            _px(img, x, y, (110, 170, 220, 255))

    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=None,
                post_paint=cracks), None


# ── Bosses (136×112) ─────────────────────────────────────────────────────────

def cinderjack_cfg():
    WAX = triple((232, 196, 72, 255))
    WAX_D = triple((172, 140, 42, 255))
    IRON = triple((108, 104, 100, 255))
    parts = [
        dict(fp=(0.16, 0.84, 0.20, 0.80), fz=(0.0, 0.30), colors=WAX_D,
             dz_key="body_dz"),
        dict(fp=(0.24, 0.76, 0.28, 0.72), fz=(0.26, 0.66), colors=WAX,
             dz_key="body_dz"),
        dict(fp=(0.34, 0.66, 0.36, 0.64), fz=(0.60, 0.95), colors=WAX,
             dz_key="body_dz"),
        # Clapper crown, worn tilted (offset east).
        dict(fp=(0.42, 0.70, 0.40, 0.60), fz=(0.95, 1.14), colors=IRON,
             dz_key="body_dz"),
        # Dripping arms.
        dict(fp=(0.08, 0.20, 0.40, 0.60), fz=(0.22, 0.55), colors=WAX_D,
             swing_key="l_arm_swing", dz_key="body_dz"),
        dict(fp=(0.80, 0.92, 0.40, 0.60), fz=(0.22, 0.55), colors=WAX_D,
             swing_key="r_arm_swing", dz_key="body_dz"),
    ]
    face = dict(fp=(0.34, 0.66, 0.36, 0.64), fz=(0.60, 0.95),
                dz_key="body_dz", style="human", skin=WAX[0],
                eye_white=(252, 168, 56, 255), eye_pupil=(120, 40, 8, 255),
                mouth=(110, 70, 20, 255), eye_h=0.62, mouth_h=0.3)

    def flames(img, facing, frame, anchor):
        phase = frame.get("phase", 0)
        for i in range(6):
            if (i + phase) % 2:
                continue
            ang = i * math.pi / 3.0
            fx = 0.5 + 0.38 * math.cos(ang)
            fy = 0.5 + 0.38 * math.sin(ang)
            x, y = project(fx, fy, 0.32, anchor)
            _px(img, x, y, (252, 168, 56, 255))
            _px(img, x, y - 1, (254, 236, 168, 255))

    idle = [frame_dict(body_dz=0.0, phase=0),
            frame_dict(body_dz=-0.015, phase=1),
            frame_dict(body_dz=-0.015, phase=0),
            frame_dict(body_dz=0.0, phase=1, blink=True)]
    walk = [frame_dict(body_dz=-0.02, phase=0,
                       l_arm_swing=0.04, r_arm_swing=-0.04),
            frame_dict(body_dz=0.02, phase=1),
            frame_dict(body_dz=-0.02, phase=0,
                       l_arm_swing=-0.04, r_arm_swing=0.04),
            frame_dict(body_dz=0.02, phase=1)]
    return dict(frame_w=BOSS_W, frame_h=BOSS_H, parts=parts, face=face,
                post_paint=flames), standard_rows(idle=idle, walk=walk)


def knell_cfg():
    SHARD = triple((206, 226, 236, 255))
    parts = [
        dict(fp=(0.36, 0.64, 0.36, 0.64), fz=(0.0, 0.02),
             colors=triple((96, 156, 200, 110))),
        dict(fp=(0.45, 0.55, 0.45, 0.55), fz=(0.40, 1.05), colors=SHARD,
             dz_key="body_dz"),
    ]

    def ring(img, facing, frame, anchor):
        spin = frame.get("spin", 0)
        shards = []
        for i in range(8):
            ang = (i + spin * 0.5) * math.pi / 4.0
            fx = 0.5 + 0.34 * math.cos(ang)
            fy = 0.5 + 0.34 * math.sin(ang)
            shards.append((fy, fx, i))
        for fy, fx, i in sorted(shards, key=lambda t: -t[0]):
            h = 0.14 + 0.05 * (i % 3)
            z0 = 0.50 + 0.06 * ((i + spin) % 3)
            draw_box(img, anchor, fx - 0.035, fx + 0.035, fy - 0.035,
                     fy + 0.035, z0, z0 + h, SHARD)

    idle = [frame_dict(body_dz=0.0, spin=0),
            frame_dict(body_dz=0.015, spin=1),
            frame_dict(body_dz=0.03, spin=2),
            frame_dict(body_dz=0.015, spin=3)]
    walk = [frame_dict(body_dz=0.01, spin=0),
            frame_dict(body_dz=0.02, spin=2),
            frame_dict(body_dz=0.01, spin=4),
            frame_dict(body_dz=0.02, spin=6)]
    return dict(frame_w=BOSS_W, frame_h=BOSS_H, parts=parts, face=None,
                post_paint=ring), standard_rows(idle=idle, walk=walk)


def deeplistener_cfg():
    HIDE = triple((48, 44, 44, 255))
    HIDE_H = triple((72, 66, 62, 255))
    CLAWS = triple((200, 194, 172, 255))
    parts = [
        dict(fp=(0.12, 0.88, 0.28, 0.88), fz=(0.0, 0.60), colors=HIDE,
             dz_key="body_dz"),
        dict(fp=(0.26, 0.74, 0.06, 0.34), fz=(0.08, 0.44), colors=HIDE,
             dz_key="body_dz"),
        dict(fp=(0.40, 0.60, 0.0, 0.10), fz=(0.16, 0.30), colors=HIDE_H,
             dz_key="body_dz"),
        dict(fp=(0.14, 0.30, 0.12, 0.28), fz=(0.0, 0.14), colors=CLAWS,
             swing_key="l_foot_swing"),
        dict(fp=(0.70, 0.86, 0.12, 0.28), fz=(0.0, 0.14), colors=CLAWS,
             swing_key="r_foot_swing"),
    ]

    def marks(img, facing, frame, anchor):
        dz = frame.get("body_dz", 0.0)
        # Roots across the back (top face).
        for (fx, fy) in ((0.30, 0.55), (0.55, 0.70), (0.70, 0.42)):
            a = project(fx, fy, 0.60 + dz, anchor)
            b = project(fx + 0.10, fy + 0.06, 0.60 + dz, anchor)
            _line(img, a[0], a[1], b[0], b[1], (92, 80, 56, 255))
        if facing != "s":
            return
        # Bell-shaped scars, faint blue, on the south flank.
        for (fx, fz) in ((0.30, 0.35), (0.62, 0.25), (0.76, 0.42)):
            x, y = project(fx, 0.285, fz + dz, anchor)
            _px(img, x, y, (110, 176, 226, 255))
            _px(img, x - 1, y + 1, (110, 176, 226, 255))
            _px(img, x + 1, y + 1, (110, 176, 226, 255))
    return dict(frame_w=BOSS_W, frame_h=BOSS_H, parts=parts, face=None,
                post_paint=marks), None


# ── Driver ───────────────────────────────────────────────────────────────────

CHARACTERS = [
    ("marten_coalbright", lambda: (marten_cfg(), None)),
    ("sister_wick", lambda: (wick_cfg(), None)),
    ("tobin_ashfoot", lambda: (tobin_cfg(), None)),
    ("hettie_marl", lambda: (hettie_cfg(), None)),
    ("grandam_bellow",
     lambda: (grandam_cfg(), standard_rows(idle=DRIFT_IDLE, walk=DRIFT_WALK))),
    ("tallow_drip", tallow_drip_cfg),
    ("sump_crawler", sump_crawler_cfg),
    ("wax_wight", wax_wight_cfg),
    ("seam_shrike", seam_shrike_cfg),
    ("hollow_spawn", hollow_spawn_cfg),
    ("cinderjack", cinderjack_cfg),
    ("knell", knell_cfg),
    ("deeplistener", deeplistener_cfg),
]


def main():
    for obj_id, make in CHARACTERS:
        cfg, rows = make()
        out = f"{MODULE_DIR}/{obj_id}"
        save(assemble(cfg, rows=rows), f"{out}/sheet.png")
        first = rows[0][1][0] if rows else IDLE_FRAMES[0]
        save(render_frame(cfg, "s", first), f"{out}/sprite.png")


if __name__ == "__main__":
    main()
