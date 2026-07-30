"""
Generates 4-facing sheets for the two dialog NPCs that previously reused the
player's static sprite with rotation_by_facing (a flat human spinning on the
ground — pre-dates the character conventions in docs/sprite_style.md):

  villager    – the shopkeeper: green tunic, grey pants, dark hair
  chatterbox  – the bard: crimson tunic, mustard pants, flame-red cap

Each gets assets/overworld_objects/<id>/sheet.png (384×768, 4 cols × 8 rows,
idle_s … walk_w) plus sprite.png (96×96 static south idle fallback). Built on
scripts/char_rig.py.
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

EYE_WHITE = (238, 238, 240, 255)
PUPIL = (40, 34, 30, 255)

CHARACTERS = {
    "villager": dict(
        skin=triple((214, 165, 120, 255)),
        hair=triple((60, 46, 30, 255)),
        top=triple((84, 128, 74, 255)),      # merchant green
        pants=triple((88, 82, 74, 255)),
        boots=triple((56, 40, 24, 255)),
        belt=triple((124, 88, 40, 255)),
        mouth=(150, 92, 74, 255),
    ),
    "chatterbox": dict(
        skin=triple((222, 176, 132, 255)),
        hair=triple((196, 60, 40, 255)),     # flame-red bard cap
        top=triple((156, 52, 68, 255)),      # crimson doublet
        pants=triple((172, 140, 58, 255)),   # mustard hose
        boots=triple((70, 48, 26, 255)),
        belt=triple((60, 50, 44, 255)),
        mouth=(140, 80, 66, 255),
    ),
}


def make_cfg(spec):
    parts, face = humanoid_parts(
        skin=spec["skin"], hair=spec["hair"], top=spec["top"],
        pants=spec["pants"], boots=spec["boots"], belt=spec["belt"],
    )
    face.update(eye_white=EYE_WHITE, eye_pupil=PUPIL, mouth=spec["mouth"])
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


def main():
    for object_id, spec in CHARACTERS.items():
        cfg = make_cfg(spec)
        out = f"assets/overworld_objects/{object_id}"
        save(assemble(cfg), f"{out}/sheet.png")
        save(render_frame(cfg, "s", IDLE_FRAMES[0]), f"{out}/sprite.png")


if __name__ == "__main__":
    main()
