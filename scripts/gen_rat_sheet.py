"""
Generates assets/overworld_objects/rat/sheet.png and sprite.png.

4-facing oblique quadruped on scripts/char_rig.py (see docs/sprite_style.md):
frame 96×64, sheet 4 cols × 8 rows (idle_s … walk_w). A low brown-grey rat —
scale 0.45, big ears, pink thin tail and snout, beady eyes. sprite.png is the
static south idle fallback.
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
OUT_DIR = "assets/overworld_objects/rat"

FUR = triple((130, 105, 80, 255))
FUR_DARK = triple((92, 72, 55, 255))
BELLY = triple((168, 142, 112, 255))
PINK = triple((196, 130, 120, 255))

EYE_WHITE = (230, 226, 220, 255)
PUPIL = (24, 18, 16, 255)


def make_cfg():
    parts, face = quadruped_parts(
        fur=FUR, fur_dark=FUR_DARK, belly=BELLY,
        scale=0.45, ears=True, snout=PINK, tail="thin",
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
