"""
Generates 4-facing sheets for the undead pair on scripts/char_rig.py
(see docs/sprite_style.md):

  skeleton    – slim bone body, tattered cloth loincloth, green eye glow,
                rib lines scored across the chest
  dire_wight  – grave-grey burial shroud + hood, corpse-pale skin, cold
                blue eye glow, frost motes at the hem

Each gets assets/overworld_objects/<id>/sheet.png (384×768, 4 cols × 8 rows,
idle_s … walk_w) plus sprite.png (96×96 static south idle fallback).

Supersedes gen_skeleton_sheet.py and gen_dire_wight_sheet.py (deleted when
this landed).
"""

from char_rig import (
    IDLE_FRAMES,
    _px,
    assemble,
    humanoid_parts,
    render_frame,
    save,
    scale_color,
    triple,
)
from wall_perspective import project

FRAME_W = 96
FRAME_H = 96

BONE = triple((218, 210, 185, 255))
CLOTH = triple((72, 58, 38, 255))
GLOW_GREEN = (140, 210, 160, 255)
GLOW_GREEN_DK = (40, 90, 55, 255)

SHROUD = triple((92, 108, 122, 255))
PALE = triple((168, 185, 190, 255))
GLOW_BLUE = (120, 220, 255, 255)
GLOW_BLUE_DK = (30, 70, 100, 255)
FROST = (190, 225, 240, 200)


def _skeleton_post(img, facing, frame, anchor):
    """Rib lines scored across the chest (south face only)."""
    if facing != "s":
        return
    dz = frame.get("body_dz", 0.0)
    rib = scale_color(BONE[0], 0.62)
    for fz in (0.60, 0.68, 0.76):
        a = project(0.40, 0.395, fz + dz, anchor)
        b = project(0.60, 0.395, fz + dz, anchor)
        for x in range(a[0], b[0] + 1):
            _px(img, x, a[1], rib)


def _wight_post(img, facing, frame, anchor):
    """Frost motes drifting at the shroud hem."""
    for i, (fx, fy, fz) in enumerate(((0.28, 0.30, 0.06), (0.72, 0.32, 0.12),
                                      (0.20, 0.42, 0.20), (0.80, 0.44, 0.03))):
        if (i + frame.get("phase", 0)) % 2:
            continue
        x, y = project(fx, fy, fz, anchor)
        _px(img, x, y, FROST)


def make_skeleton_cfg():
    parts, face = humanoid_parts(
        skin=BONE, hair=None, top=BONE, pants=CLOTH, boots=BONE,
        belt=CLOTH, scale=1.0, slim=0.78,
    )
    face.update(eye_white=GLOW_GREEN, eye_pupil=GLOW_GREEN_DK,
                mouth=scale_color(BONE[0], 0.5), eye_h=0.6)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=_skeleton_post)


def make_wight_cfg():
    parts, face = humanoid_parts(
        skin=PALE, hair=SHROUD, top=SHROUD, pants=SHROUD, boots=SHROUD,
        belt=triple((58, 70, 82, 255)), scale=1.0, slim=0.9,
    )
    face.update(eye_white=GLOW_BLUE, eye_pupil=GLOW_BLUE_DK,
                mouth=(40, 50, 58, 255), eye_h=0.6)
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face,
                post_paint=_wight_post)


def main():
    for object_id, cfg in (("skeleton", make_skeleton_cfg()),
                           ("dire_wight", make_wight_cfg())):
        out = f"assets/overworld_objects/{object_id}"
        save(assemble(cfg), f"{out}/sheet.png")
        save(render_frame(cfg, "s", IDLE_FRAMES[0]), f"{out}/sprite.png")


if __name__ == "__main__":
    main()
