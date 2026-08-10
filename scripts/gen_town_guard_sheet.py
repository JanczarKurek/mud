"""
Generates assets/overworld_objects/town_guard/sheet.png and sprite.png.

4-facing oblique humanoid on scripts/char_rig.py (see docs/sprite_style.md):
frame 96×96, sheet 384×768 (4 cols × 8 rows, idle_s … walk_w). A town guard
in steel-and-blue livery — the `hair` slot doubles as a steel helmet, blue
tunic over grey trousers, dark boots, brown belt — carrying a tall spear in
the right hand (wood shaft, steel head, planted butt) and a blue kite-ish
shield with a steel boss on the left arm. Both ride the matching arm's swing
key so they march with the stride. sprite.png is the static south idle
fallback.
"""

from char_rig import (
    IDLE_FRAMES,
    assemble,
    humanoid_parts,
    render_frame,
    save,
    triple,
)

# 128 wide (boss-style frame, precedented by cyclops/ogre sheets): the spear
# overtops the helmet, and the oblique shear pushes tall art up-left further
# than a 96-wide frame can hold once the north facing mirrors it.
FRAME_W = 128
FRAME_H = 96
OUT_DIR = "assets/overworld_objects/town_guard"

SKIN = triple((214, 165, 120, 255))
HELMET = triple((150, 156, 168, 255))   # polished steel
TUNIC = triple((70, 88, 140, 255))      # town livery blue
PANTS = triple((92, 94, 100, 255))
BOOTS = triple((52, 40, 28, 255))
BELT = triple((110, 78, 40, 255))

SHAFT = triple((124, 92, 52, 255))      # ash-wood spear shaft
STEEL = triple((176, 182, 194, 255))    # spearhead + shield boss
SHIELD = triple((56, 72, 122, 255))     # shield face, a shade deeper than the tunic
SHIELD_RIM = triple((150, 156, 168, 255))

EYE_WHITE = (238, 238, 240, 255)
PUPIL = (40, 34, 30, 255)
MOUTH = (150, 92, 74, 255)

# Canonical facing is south; rotate_xy re-seats these per facing. The right
# arm sits at fp x 0.66–0.72, the left at 0.28–0.34 (see humanoid_parts).
SPEAR_SHAFT_FP = (0.70, 0.74, 0.46, 0.53)
SPEAR_HEAD_FP = (0.695, 0.745, 0.455, 0.535)
SHIELD_FP = (0.21, 0.27, 0.34, 0.66)
SHIELD_RIM_FP = (0.21, 0.27, 0.32, 0.68)
SHIELD_BOSS_FP = (0.185, 0.215, 0.44, 0.56)


def guard_gear():
    """Spear (right hand) + shield (left arm) as extra rig boxes."""
    return [
        # Spear: planted shaft rising well past the helmet, leaf head on top.
        dict(fp=SPEAR_SHAFT_FP, fz=(0.0, 1.30), colors=SHAFT,
             swing_key="r_arm_swing"),
        dict(fp=SPEAR_HEAD_FP, fz=(1.30, 1.42), colors=STEEL,
             swing_key="r_arm_swing"),
        # Shield: rim slab behind, face slab proud of it, steel boss proudest.
        dict(fp=SHIELD_RIM_FP, fz=(0.26, 0.84), colors=SHIELD_RIM,
             swing_key="l_arm_swing"),
        dict(fp=SHIELD_FP, fz=(0.30, 0.80), colors=SHIELD,
             swing_key="l_arm_swing"),
        dict(fp=SHIELD_BOSS_FP, fz=(0.48, 0.62), colors=STEEL,
             swing_key="l_arm_swing"),
    ]


def make_cfg():
    parts, face = humanoid_parts(
        skin=SKIN, hair=HELMET, top=TUNIC, pants=PANTS, boots=BOOTS, belt=BELT,
    )
    parts.extend(guard_gear())
    face.update(eye_white=EYE_WHITE, eye_pupil=PUPIL, mouth=MOUTH)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


def main():
    cfg = make_cfg()
    save(assemble(cfg), f"{OUT_DIR}/sheet.png")
    save(render_frame(cfg, "s", IDLE_FRAMES[0]), f"{OUT_DIR}/sprite.png")


if __name__ == "__main__":
    main()
