"""
Generates assets/overworld_objects/sheep/sheet.png and sprite.png.

4-facing oblique quadruped on scripts/char_rig.py (see docs/sprite_style.md):
frame 96×64, sheet 4 cols × 8 rows (idle_s … walk_w). A woolly pasture sheep —
cream fleece over dark legs and face, stubby "thin" tail, slightly smaller
than a wolf (scale 0.9). sprite.png is the static south idle fallback.
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
OUT_DIR = "assets/overworld_objects/sheep"

WOOL = triple((228, 224, 214, 255))     # cream fleece
DARK = triple((74, 66, 58, 255))        # legs / face / ears
BELLY = triple((205, 200, 188, 255))
MUZZLE = triple((88, 78, 68, 255))

EYE_WHITE = (240, 238, 232, 255)
PUPIL = (30, 26, 22, 255)


def make_cfg():
    parts, face = quadruped_parts(
        fur=WOOL, fur_dark=DARK, belly=BELLY,
        scale=0.9, ears=True, snout=MUZZLE, tail="thin",
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
