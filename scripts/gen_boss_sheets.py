"""
Generates 4-facing sheets for the three overworld bosses on
scripts/char_rig.py (see docs/sprite_style.md). Boss frames are 136×112 —
wide enough to absorb the up-LEFT head lean at scale 1.5 (head top ~1.62
floors → 58 px of lean; anchor_x = 136/2 − 24 = 44 keeps it in-frame).

  cyclops        – rocky grey-green hide, single fiery eye + mono-brow,
                   wooden club planted at its side
  ogre_brute     – tawny hide, fur loincloth, club
  fire_elemental – NOT a humanoid: a tapering column of flame (centered,
                   so every facing renders the same), pulsing bands,
                   rising embers, white-hot eyes

Each gets assets/overworld_objects/<id>/sheet.png (544×896, 4 cols × 8 rows,
idle_s … walk_w) plus sprite.png (136×112 static south idle fallback).

Supersedes gen_cyclops_sheet.py, gen_ogre_brute_sheet.py and
gen_fire_elemental_sheet.py (deleted when this landed).
"""

import hashlib

from char_rig import (
    IDLE_FRAMES,
    _px,
    assemble,
    frame_dict,
    humanoid_parts,
    render_frame,
    save,
    standard_rows,
    triple,
)
from wall_perspective import project

FRAME_W = 136
FRAME_H = 112
SCALE = 1.5

CLUB = triple((95, 65, 25, 255))


# ── Cyclops / ogre (humanoid plan) ───────────────────────────────────────────

def make_cyclops_cfg():
    skin = triple((112, 132, 84, 255))
    loin = triple((80, 55, 20, 255))
    parts, face = humanoid_parts(
        skin=skin, hair=None, top=skin, pants=loin, boots=skin,
        belt=triple((60, 40, 14, 255)), scale=SCALE,
    )
    parts.append(dict(fp=(0.76, 0.86, 0.42, 0.56), fz=(0.0, 1.05 * SCALE),
                      colors=CLUB, swing_key="r_arm_swing"))
    face.update(style="cyclops", eye_white=(240, 230, 180, 255),
                eye_iris=(210, 80, 20, 255), eye_pupil=(25, 10, 5, 255),
                eye_h=0.55)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


def make_ogre_cfg():
    skin = triple((150, 118, 82, 255))
    hide = triple((92, 62, 30, 255))
    parts, face = humanoid_parts(
        skin=skin, hair=triple((70, 52, 30, 255)), top=skin, pants=hide,
        boots=skin, belt=triple((58, 40, 16, 255)), scale=SCALE,
    )
    parts.append(dict(fp=(0.76, 0.86, 0.42, 0.56), fz=(0.0, 1.05 * SCALE),
                      colors=triple((100, 70, 30, 255)),
                      swing_key="r_arm_swing"))
    face.update(eye_white=(235, 225, 190, 255), eye_pupil=(30, 15, 8, 255),
                mouth=(40, 22, 10, 255), eye_h=0.6)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


# ── Fire elemental (flame-column plan) ───────────────────────────────────────

FLAME_DARK = triple((110, 18, 8, 255))
FLAME_RED = triple((200, 42, 16, 255))
FLAME_ORG = triple((240, 108, 24, 255))
FLAME_YLW = triple((252, 192, 46, 255))
FLAME_WHT = triple((255, 244, 196, 255))
EMBER = (255, 168, 64, 255)
EMBER_DIM = (190, 78, 20, 255)


def _elemental_post(img, facing, frame, anchor):
    """Rising embers around the column, hash-placed per pulse phase."""
    phase = frame.get("phase", 0)
    for i in range(7):
        d = hashlib.md5(f"ember:{i}:{phase}".encode()).digest()
        fx = 0.18 + (d[0] % 100) / 100.0 * 0.64
        fy = 0.30 + (d[1] % 100) / 100.0 * 0.30
        fz = 0.15 + (d[2] % 100) / 100.0 * 1.35
        x, y = project(fx, fy, fz, anchor)
        _px(img, x, y, EMBER if d[3] % 2 else EMBER_DIM)


def make_elemental_cfg():
    parts = [
        dict(fp=(0.28, 0.72, 0.28, 0.72), fz=(0.0, 0.22),
             colors=FLAME_DARK, dz_key="f0"),
        dict(fp=(0.33, 0.67, 0.33, 0.67), fz=(0.16, 0.70),
             colors=FLAME_RED, dz_key="f1"),
        dict(fp=(0.38, 0.62, 0.38, 0.62), fz=(0.60, 1.10),
             colors=FLAME_ORG, dz_key="f2"),
        dict(fp=(0.43, 0.57, 0.43, 0.57), fz=(1.00, 1.42),
             colors=FLAME_YLW, dz_key="f3"),
        dict(fp=(0.47, 0.53, 0.47, 0.53), fz=(1.36, 1.60),
             colors=FLAME_WHT, dz_key="f3"),
    ]
    face = dict(fp=(0.38, 0.62, 0.38, 0.62), fz=(0.60, 1.10), dz_key="f2",
                style="human", skin=FLAME_ORG[0],
                eye_white=(255, 244, 196, 255), eye_pupil=(110, 18, 8, 255),
                eye_h=0.75)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=_elemental_post)


def _flick(phase, *dzs):
    return frame_dict(phase=phase, f0=dzs[0], f1=dzs[1], f2=dzs[2], f3=dzs[3])


ELEMENTAL_IDLE = [
    _flick(0, 0.00, 0.00, 0.00, 0.00),
    _flick(1, 0.00, 0.02, -0.02, 0.03),
    _flick(2, 0.00, -0.02, 0.03, -0.02),
    _flick(3, 0.00, 0.01, 0.01, 0.05),
]
ELEMENTAL_WALK = [
    _flick(0, 0.00, 0.03, 0.05, 0.08),
    _flick(1, 0.00, -0.02, -0.03, -0.04),
    _flick(2, 0.00, 0.04, 0.06, 0.10),
    _flick(3, 0.00, -0.01, -0.02, -0.03),
]


def main():
    for object_id, cfg, rows in (
        ("cyclops", make_cyclops_cfg(), None),
        ("ogre_brute", make_ogre_cfg(), None),
        ("fire_elemental", make_elemental_cfg(),
         standard_rows(idle=ELEMENTAL_IDLE, walk=ELEMENTAL_WALK)),
    ):
        out = f"assets/overworld_objects/{object_id}"
        save(assemble(cfg, rows=rows), f"{out}/sheet.png")
        first = rows[0][1][0] if rows else IDLE_FRAMES[0]
        save(render_frame(cfg, "s", first), f"{out}/sprite.png")


if __name__ == "__main__":
    main()
