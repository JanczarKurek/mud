"""
Generates assets/overworld_objects/judge/sheet.png and sprite.png.

4-facing oblique humanoid on scripts/char_rig.py (see docs/sprite_style.md):
frame 96×96, sheet 384×768 (4 cols × 8 rows, idle_s … walk_w). A magistrate of
the Emberbrook Watch who takes coin to clear a criminal's guilt: black robe
over a deep crimson sash, grey hair, and a gold-topped staff of office in the
right hand. Read as authority rather than a fighter — no shield, no blade.
sprite.png is the static south idle fallback.
"""

from char_rig import (
    IDLE_FRAMES,
    assemble,
    humanoid_parts,
    render_frame,
    save,
    triple,
)

# 96 wide is enough: the staff is only a little taller than the head, unlike
# the guard's overtopping spear.
FRAME_W = 96
FRAME_H = 96
OUT_DIR = "assets/overworld_objects/judge"

SKIN = triple((208, 160, 118, 255))
HAIR = triple((198, 198, 202, 255))     # grey, for age and gravity
ROBE = triple((38, 36, 46, 255))        # near-black magistrate's robe
PANTS = triple((44, 42, 52, 255))       # robe continues below the belt
BOOTS = triple((40, 30, 22, 255))
SASH = triple((132, 32, 44, 255))       # crimson sash worn as the belt

STAFF = triple((96, 70, 40, 255))       # dark wood shaft
GOLD = triple((198, 164, 74, 255))      # gilt finial

EYE_WHITE = (238, 238, 240, 255)
PUPIL = (40, 34, 30, 255)
MOUTH = (150, 92, 74, 255)

# Canonical facing is south; rotate_xy re-seats these per facing. The right
# arm sits at fp x 0.66–0.72 (see humanoid_parts).
STAFF_SHAFT_FP = (0.70, 0.74, 0.46, 0.53)
STAFF_FINIAL_FP = (0.685, 0.755, 0.45, 0.54)


def judge_gear():
    """Staff of office in the right hand, gilt finial on top."""
    return [
        dict(fp=STAFF_SHAFT_FP, fz=(0.0, 1.12), colors=STAFF,
             swing_key="r_arm_swing"),
        dict(fp=STAFF_FINIAL_FP, fz=(1.12, 1.22), colors=GOLD,
             swing_key="r_arm_swing"),
    ]


def make_cfg():
    parts, face = humanoid_parts(
        skin=SKIN, hair=HAIR, top=ROBE, pants=PANTS, boots=BOOTS, belt=SASH,
    )
    parts.extend(judge_gear())
    face.update(eye_white=EYE_WHITE, eye_pupil=PUPIL, mouth=MOUTH)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


def main():
    cfg = make_cfg()
    save(assemble(cfg), f"{OUT_DIR}/sheet.png")
    save(render_frame(cfg, "s", IDLE_FRAMES[0]), f"{OUT_DIR}/sprite.png")


if __name__ == "__main__":
    main()
