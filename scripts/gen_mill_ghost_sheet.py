"""
Generates assets/modules/haunted_mill/overworld_objects/mill_ghost/sheet.png
and sprite.png — Old Maple, the drowned miller: a translucent pale-blue
badger ghost in a flour-dusted apron. No feet: the body tapers into a wisp.

4-facing oblique sheet on scripts/char_rig.py (see docs/sprite_style.md):
frame 96×96, 4 cols × 8 rows (idle_s … walk_w). Ghosts don't stride — the
"walk" rows are a stronger drifting bob with arm sway.

`ghost_badger_parts()` is shared with gen_hollow_bell_sheets.py (Grandam
Bellow is also a badger ghost, in a different palette).
"""

from char_rig import (
    assemble,
    frame_dict,
    render_frame,
    save,
    standard_rows,
    triple,
)
from wall_perspective import project, _line

FRAME_W = 96
FRAME_H = 96
OUT_DIR = "assets/modules/haunted_mill/overworld_objects/mill_ghost"

A = 170
GHOST = triple((192, 216, 238, A))
GHOST_DK = triple((150, 180, 212, A))
SNOUT = triple((210, 226, 244, A))
APRON = triple((224, 224, 210, A + 15))
STRIPE = (78, 94, 122, A + 25)
EYE = (44, 56, 84, A + 60)
EYE_HI = (210, 226, 248, A + 40)


def ghost_badger_parts(*, body, body_dk, snout, apron):
    """Translucent stooped badger ghost: tapering wisp base, apron slab,
    drifting arms, striped head. Colour args are (front, side, top) triples
    (alpha carried through). Returns (parts, face) like the rig builders."""
    parts = [
        # Wisp taper — narrow faint base up into the robe.
        dict(fp=(0.44, 0.56, 0.44, 0.56), fz=(0.0, 0.09),
             colors=tuple(( r, g, b, max(0, a - 60)) for (r, g, b, a) in body_dk),
             dz_key="body_dz"),
        dict(fp=(0.38, 0.62, 0.40, 0.60), fz=(0.06, 0.28),
             colors=tuple((r, g, b, max(0, a - 25)) for (r, g, b, a) in body),
             dz_key="body_dz"),
        # Torso + apron slab on the front.
        dict(fp=(0.34, 0.66, 0.38, 0.62), fz=(0.24, 0.66),
             colors=body, dz_key="body_dz"),
        dict(fp=(0.36, 0.64, 0.365, 0.40), fz=(0.18, 0.58),
             colors=apron, dz_key="body_dz"),
        # Arms (drift with the walk sway).
        dict(fp=(0.28, 0.34, 0.42, 0.58), fz=(0.34, 0.58),
             colors=body_dk, swing_key="l_arm_swing", dz_key="body_dz"),
        dict(fp=(0.66, 0.72, 0.42, 0.58), fz=(0.34, 0.58),
             colors=body_dk, swing_key="r_arm_swing", dz_key="body_dz"),
        # Head, muzzle, round ears.
        dict(fp=(0.36, 0.64, 0.36, 0.64), fz=(0.68, 0.90),
             colors=body, dz_key="body_dz"),
        dict(fp=(0.44, 0.56, 0.30, 0.38), fz=(0.70, 0.78),
             colors=snout, dz_key="body_dz"),
        dict(fp=(0.37, 0.44, 0.44, 0.56), fz=(0.90, 0.97),
             colors=body_dk, dz_key="body_dz"),
        dict(fp=(0.56, 0.63, 0.44, 0.56), fz=(0.90, 0.97),
             colors=body_dk, dz_key="body_dz"),
    ]
    face = dict(fp=(0.36, 0.64, 0.36, 0.64), fz=(0.68, 0.90),
                dz_key="body_dz", style="beast", skin=body[0],
                eye_h=0.55, eye_span=(0.24, 0.76))
    return parts, face


def _stripes_post(img, facing, frame, anchor):
    """Badger face stripes down the south face of the head."""
    if facing != "s":
        return
    dz = frame.get("body_dz", 0.0)
    for fx in (0.435, 0.565):
        a = project(fx, 0.355, 0.70 + dz, anchor)
        b = project(fx, 0.355, 0.90 + dz, anchor)
        _line(img, a[0], a[1], b[0], b[1], STRIPE)


DRIFT_IDLE = [
    frame_dict(body_dz=0.00),
    frame_dict(body_dz=0.02),
    frame_dict(body_dz=0.03, blink=False),
    frame_dict(body_dz=0.01, blink=True),
]
DRIFT_WALK = [
    frame_dict(body_dz=0.01, l_arm_swing=0.03, r_arm_swing=-0.03),
    frame_dict(body_dz=0.04),
    frame_dict(body_dz=0.01, l_arm_swing=-0.03, r_arm_swing=0.03),
    frame_dict(body_dz=0.04),
]


def make_cfg():
    parts, face = ghost_badger_parts(body=GHOST, body_dk=GHOST_DK,
                                     snout=SNOUT, apron=APRON)
    face.update(eye_white=EYE_HI, eye_pupil=EYE)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=_stripes_post)


def main():
    cfg = make_cfg()
    rows = standard_rows(idle=DRIFT_IDLE, walk=DRIFT_WALK)
    save(assemble(cfg, rows=rows), f"{OUT_DIR}/sheet.png")
    save(render_frame(cfg, "s", DRIFT_IDLE[0]), f"{OUT_DIR}/sprite.png")


if __name__ == "__main__":
    main()
