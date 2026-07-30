"""
Generates the townsfolk sprite set under assets/overworld_objects/townsfolk/:

  sheet.png  – base anim, 4 cols × 8 rows (384×768): 4-facing idle/walk
               (idle_s, walk_s, idle_n, walk_n, idle_e, walk_e, idle_w, walk_w)
  work.png   – "working" pose, 4 cols × 1 row (384×96): hammer up/down cycle,
               south-facing (station poses stay single-facing; the state's
               unsuffixed `idle` clip falls back correctly for any facing)
  sleep.png  – "sleeping" pose, 2 cols × 1 row (192×96): eyes shut + Zzz bob
  sprite.png – single static south idle frame (96×96), no-animation fallback

A friendly villager: tan skin, brown hair, blue tunic under a leather apron.
Built on scripts/char_rig.py (the shared 4-facing oblique box rig — see
docs/sprite_style.md), so the townsfolk reads at the same scale and style as
the player. Frame size 96×96, logical height ~1.2 tiles.
"""

from char_rig import (
    IDLE_FRAMES,
    _px,
    assemble,
    draw_box,
    frame_dict,
    humanoid_parts,
    render_frame,
    save,
    triple,
)
from wall_perspective import project

FRAME_W = 96
FRAME_H = 96
OUT_DIR = "assets/overworld_objects/townsfolk"

# ── Palette ──────────────────────────────────────────────────────────────────
SKIN = triple((214, 165, 120, 255))
HAIR = triple((104, 68, 38, 255))
TUNIC = triple((64, 104, 138, 255))
APRON = triple((158, 116, 74, 255))
PANTS = triple((70, 58, 46, 255))
BOOTS = triple((46, 34, 22, 255))
BELT = triple((92, 62, 30, 255))

EYE_WHITE = (238, 238, 240, 255)
PUPIL = (40, 34, 30, 255)
MOUTH = (150, 92, 74, 255)
HAMMER_WOOD = triple((132, 92, 52, 255))
HAMMER_HEAD = triple((110, 112, 120, 255))
SPARK = (255, 214, 120, 255)
ZZZ = (226, 232, 244, 255)


def _post_paint(img, facing, frame, anchor):
    """Work-hammer and sleep-Zzz overlays (south-facing pose sheets only)."""
    if frame.get("hammer") and facing == "s":
        dz = frame.get("ham_dz", 0.0)
        # Haft held in the right hand, out in front of the body.
        draw_box(img, anchor, 0.68, 0.72, 0.30, 0.38,
                 0.55 + dz, 0.95 + dz, HAMMER_WOOD)
        draw_box(img, anchor, 0.60, 0.80, 0.28, 0.40,
                 0.92 + dz, 1.04 + dz, HAMMER_HEAD)
    if frame.get("spark") and facing == "s":
        for (fx, fz) in ((0.78, 0.10), (0.84, 0.16), (0.74, 0.20)):
            x, y = project(fx, 0.20, fz, anchor)
            _px(img, x, y, SPARK)
            _px(img, x + 1, y, SPARK)
    z = frame.get("zzz", 0)
    if z and facing == "s":
        zx, zy = project(0.86, 0.40, 1.25, anchor)
        _draw_z(img, zx, zy, 2)
        if z > 1:
            zx, zy = project(0.97, 0.42, 1.42, anchor)
            _draw_z(img, zx, zy, 3)


def _draw_z(img, x, y, s):
    for dx in range(s + 1):
        _px(img, x + dx, y, ZZZ)
        _px(img, x + dx, y + s, ZZZ)
        _px(img, x + s - dx, y + dx, ZZZ)


def make_cfg():
    parts, face = humanoid_parts(
        skin=SKIN, hair=HAIR, top=TUNIC, pants=PANTS, boots=BOOTS,
        belt=BELT, apron=APRON,
    )
    face.update(eye_white=EYE_WHITE, eye_pupil=PUPIL, mouth=MOUTH)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=_post_paint)


# ── Pose frames ──────────────────────────────────────────────────────────────
WORK_FRAMES = [
    frame_dict(hammer=True, ham_dz=0.35, r_arm_swing=-0.02),
    frame_dict(hammer=True, ham_dz=0.15, r_arm_swing=0.01),
    frame_dict(hammer=True, ham_dz=-0.08, r_arm_swing=0.04,
               spark=True, body_dz=-0.015),
    frame_dict(hammer=True, ham_dz=0.15, r_arm_swing=0.01),
]

SLEEP_FRAMES = [
    frame_dict(blink=True, body_dz=-0.02, zzz=1),
    frame_dict(blink=True, body_dz=-0.02, zzz=2),
]


def main():
    cfg = make_cfg()
    save(assemble(cfg), f"{OUT_DIR}/sheet.png")
    save(assemble(cfg, rows=[("s", WORK_FRAMES)]), f"{OUT_DIR}/work.png")
    save(assemble(cfg, rows=[("s", SLEEP_FRAMES)], cols=2),
         f"{OUT_DIR}/sleep.png")
    save(render_frame(cfg, "s", IDLE_FRAMES[0]), f"{OUT_DIR}/sprite.png")


if __name__ == "__main__":
    main()
