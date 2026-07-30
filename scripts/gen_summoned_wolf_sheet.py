"""
Generates assets/overworld_objects/summoned_wolf/sheet.png and sprite.png.

4-facing oblique quadruped on scripts/char_rig.py (see docs/sprite_style.md):
frame 96×64, sheet 4 cols × 8 rows (idle_s … walk_w). A spectral blue-grey
wolf (the player's summon) — scale 1.0, bushy tail, dark muzzle, pale spectral
belly so the ghostly tint reads. sprite.png is the static south idle fallback.
"""

from char_rig import (
    QUAD_IDLE_FRAMES,
    QUAD_WALK_FRAMES,
    assemble,
    quadruped_parts,
    render_frame,
    save,
    standard_rows,
    triple,
)

FRAME_W = 96
FRAME_H = 64
OUT_DIR = "assets/overworld_objects/summoned_wolf"

FUR = triple((122, 130, 150, 255))
FUR_DARK = triple((80, 88, 110, 255))
BELLY = triple((170, 180, 200, 255))
MUZZLE = triple((66, 72, 92, 255))

EYE_WHITE = (196, 226, 244, 255)   # icy spectral glint
PUPIL = (30, 40, 66, 255)


def make_cfg():
    parts, face = quadruped_parts(
        fur=FUR, fur_dark=FUR_DARK, belly=BELLY,
        scale=1.0, ears=True, snout=MUZZLE, tail="bushy",
    )
    face.update(eye_white=EYE_WHITE, eye_pupil=PUPIL)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


def main():
    cfg = make_cfg()
    rows = standard_rows(idle=QUAD_IDLE_FRAMES, walk=QUAD_WALK_FRAMES)
    save(assemble(cfg, rows=rows), f"{OUT_DIR}/sheet.png")
    save(render_frame(cfg, "s", QUAD_IDLE_FRAMES[0]), f"{OUT_DIR}/sprite.png")


if __name__ == "__main__":
    main()
