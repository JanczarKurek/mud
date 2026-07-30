"""
Generates 4-facing sheets for the three gobbos on scripts/char_rig.py
(see docs/sprite_style.md): one shared short-and-green body (scale 0.72,
big side-sticking ears, yellow eyes), three kits:

  goblin         – brown tunic warrior (bald)
  archer_goblin  – lighter green skin, gray tunic, leather cap, back quiver
  goblin_mage    – dark purple robe + hood, gold trim belt

Each gets assets/overworld_objects/<id>/sheet.png (384×768, 4 cols × 8 rows,
idle_s … walk_w) plus sprite.png (96×96 static south idle fallback).

Supersedes gen_goblin_sheet.py and gen_goblin_mage_sheet.py (deleted when
this landed); the archer's legacy art lived in gen_ranged_assets.py.
"""

from char_rig import (
    IDLE_FRAMES,
    assemble,
    humanoid_parts,
    render_frame,
    save,
    triple,
)

FRAME_W = 96
FRAME_H = 96
SCALE = 0.72

EYE = (255, 220, 30, 255)      # yellow goblin eyes
PUPIL = (20, 20, 20, 255)
MOUTH = (40, 20, 10, 255)

GOBBOS = {
    "goblin": dict(
        skin=triple((92, 140, 52, 255)),
        hair=None,                               # bald
        top=triple((80, 50, 20, 255)),           # brown tunic
        pants=triple((56, 36, 12, 255)),
        boots=triple((40, 25, 8, 255)),
        belt=triple((120, 80, 20, 255)),
    ),
    "archer_goblin": dict(
        skin=triple((112, 176, 92, 255)),        # lighter green
        hair=triple((100, 70, 30, 255)),         # leather cap
        top=triple((80, 80, 92, 255)),           # gray tunic
        pants=triple((56, 44, 30, 255)),
        boots=triple((40, 30, 18, 255)),
        belt=triple((80, 60, 40, 255)),
        quiver=triple((110, 72, 34, 255)),
    ),
    "goblin_mage": dict(
        skin=triple((92, 140, 52, 255)),
        hair=triple((44, 22, 70, 255)),          # hood
        top=triple((62, 30, 82, 255)),           # dark purple robe
        pants=triple((62, 30, 82, 255)),         # robe skirt
        boots=triple((38, 24, 50, 255)),
        belt=triple((200, 170, 60, 255)),        # gold trim
    ),
}


def make_cfg(spec):
    parts, face = humanoid_parts(
        skin=spec["skin"], hair=spec["hair"], top=spec["top"],
        pants=spec["pants"], boots=spec["boots"], belt=spec["belt"],
        scale=SCALE,
    )
    # Big goblin ears sticking out sideways at mid-head height.
    ear_z0, ear_z1 = 0.96 * SCALE, 1.04 * SCALE
    parts.append(dict(fp=(0.26, 0.36, 0.44, 0.56), fz=(ear_z0, ear_z1),
                      colors=spec["skin"], dz_key="body_dz"))
    parts.append(dict(fp=(0.64, 0.74, 0.44, 0.56), fz=(ear_z0, ear_z1),
                      colors=spec["skin"], dz_key="body_dz"))
    # Archer's quiver on the back (north side, so it shows when walking away).
    if spec.get("quiver"):
        parts.append(dict(fp=(0.44, 0.56, 0.615, 0.675),
                          fz=(0.40 * SCALE, 1.0 * SCALE),
                          colors=spec["quiver"], dz_key="body_dz"))
    face.update(eye_white=EYE, eye_pupil=PUPIL, mouth=MOUTH,
                eye_h=0.6, eye_span=(0.26, 0.74))
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


def main():
    for object_id, spec in GOBBOS.items():
        cfg = make_cfg(spec)
        out = f"assets/overworld_objects/{object_id}"
        save(assemble(cfg), f"{out}/sheet.png")
        save(render_frame(cfg, "s", IDLE_FRAMES[0]), f"{out}/sprite.png")


if __name__ == "__main__":
    main()
