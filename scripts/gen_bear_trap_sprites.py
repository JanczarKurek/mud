"""
Generates the two bear-trap state sprites:
  assets/overworld_objects/bear_trap/armed.png   (jaws open, teeth radiating)
  assets/overworld_objects/bear_trap/sprung.png  (jaws clamped shut, hint of red)

Authored at 16x16 then nearest-neighbor scaled 3x to 48x48 (matches the
water/blazing_fire convention). Top-down view: round iron base ring with
serrated teeth pointing inward toward a central plate; sprung version
closes those teeth into a tight crown with blood traces.
"""

from PIL import Image
import os

AUTHOR_W = 16
AUTHOR_H = 16
SCALE = 3
FRAME_W = AUTHOR_W * SCALE
FRAME_H = AUTHOR_H * SCALE

BG          = (0,   0,   0,   0)
SHADOW      = ( 30,  28,  28, 180)  # ground shadow under trap
IRON_DARK   = ( 55,  55,  60, 255)  # darkest iron
IRON_MID    = ( 95,  95, 105, 255)  # main iron
IRON_HI     = (160, 160, 170, 255)  # highlight on rim
TOOTH       = (210, 210, 215, 255)  # bright tooth glint
PLATE       = ( 80,  68,  50, 255)  # exposed wooden / rusted pressure plate
PLATE_HI    = (120, 100,  70, 255)  # plate highlight
CHAIN       = ( 70,  70,  78, 255)  # small chain link at edge
BLOOD_DARK  = (110,  18,  18, 255)
BLOOD       = (170,  30,  25, 255)


def render(spec, out_path):
    img = Image.new("RGBA", (AUTHOR_W, AUTHOR_H), BG)
    for x, y, color in spec:
        if 0 <= x < AUTHOR_W and 0 <= y < AUTHOR_H:
            img.putpixel((x, y), color)
    scaled = img.resize((FRAME_W, FRAME_H), Image.NEAREST)
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    scaled.save(out_path)
    print(f"wrote {out_path} ({scaled.size[0]}x{scaled.size[1]})")


# ── ARMED ──────────────────────────────────────────────────────────────────
# A roughly circular trap, jaws spread wide. Center plate visible. Teeth
# point inward in two arcs (top half + bottom half). Tiny chain stub at one
# side suggests it's pegged to the ground.

ARMED = [
    # Ground shadow (soft oval below trap)
    (4, 13, SHADOW), (5, 13, SHADOW), (6, 13, SHADOW), (7, 13, SHADOW),
    (8, 13, SHADOW), (9, 13, SHADOW), (10, 13, SHADOW), (11, 13, SHADOW),
    (5, 14, SHADOW), (10, 14, SHADOW),

    # Outer rim (rough octagon)
    (5, 3, IRON_DARK), (6, 3, IRON_DARK), (7, 3, IRON_DARK),
    (8, 3, IRON_DARK), (9, 3, IRON_DARK), (10, 3, IRON_DARK),
    (4, 4, IRON_DARK), (11, 4, IRON_DARK),
    (3, 5, IRON_DARK), (12, 5, IRON_DARK),
    (3, 6, IRON_DARK), (12, 6, IRON_DARK),
    (3, 7, IRON_DARK), (12, 7, IRON_DARK),
    (3, 8, IRON_DARK), (12, 8, IRON_DARK),
    (3, 9, IRON_DARK), (12, 9, IRON_DARK),
    (3, 10, IRON_DARK), (12, 10, IRON_DARK),
    (4, 11, IRON_DARK), (11, 11, IRON_DARK),
    (5, 12, IRON_DARK), (6, 12, IRON_DARK), (7, 12, IRON_DARK),
    (8, 12, IRON_DARK), (9, 12, IRON_DARK), (10, 12, IRON_DARK),

    # Inner rim highlight (top-left only — implied light from top-left)
    (5, 4, IRON_HI), (6, 4, IRON_HI), (7, 4, IRON_HI),
    (4, 5, IRON_HI), (4, 6, IRON_HI),

    # Rim mid-tone fill (forms the iron ring body)
    (5, 5, IRON_MID), (6, 5, IRON_MID), (7, 5, IRON_MID),
    (8, 5, IRON_MID), (9, 5, IRON_MID), (10, 5, IRON_MID),
    (8, 4, IRON_MID), (9, 4, IRON_MID), (10, 4, IRON_MID),
    (4, 7, IRON_MID), (4, 8, IRON_MID), (4, 9, IRON_MID), (4, 10, IRON_MID),
    (11, 5, IRON_MID), (11, 6, IRON_MID), (11, 7, IRON_MID),
    (11, 8, IRON_MID), (11, 9, IRON_MID), (11, 10, IRON_MID),
    (5, 11, IRON_MID), (6, 11, IRON_MID), (7, 11, IRON_MID),
    (8, 11, IRON_MID), (9, 11, IRON_MID), (10, 11, IRON_MID),

    # Inner cavity (pressure plate area) — wooden plate with highlight
    (6, 7, PLATE),    (7, 7, PLATE_HI), (8, 7, PLATE_HI), (9, 7, PLATE),
    (6, 8, PLATE),    (7, 8, PLATE),    (8, 8, PLATE),    (9, 8, PLATE),
    (6, 9, PLATE),    (7, 9, PLATE),    (8, 9, PLATE),    (9, 9, PLATE),

    # Teeth — top row pointing down into the cavity
    (5, 6, TOOTH), (6, 6, TOOTH), (7, 6, TOOTH),
    (8, 6, TOOTH), (9, 6, TOOTH), (10, 6, TOOTH),
    # Teeth — bottom row pointing up
    (5, 10, TOOTH), (6, 10, TOOTH), (7, 10, TOOTH),
    (8, 10, TOOTH), (9, 10, TOOTH), (10, 10, TOOTH),

    # Chain stub at right edge
    (13, 7, CHAIN), (14, 7, CHAIN),
    (13, 8, CHAIN),
]


# ── SPRUNG ─────────────────────────────────────────────────────────────────
# Same trap, but the jaws have closed: the teeth converge into a tight
# meeting line down the center, with hints of dried blood at the seam.

SPRUNG = [
    # Ground shadow
    (4, 13, SHADOW), (5, 13, SHADOW), (6, 13, SHADOW), (7, 13, SHADOW),
    (8, 13, SHADOW), (9, 13, SHADOW), (10, 13, SHADOW), (11, 13, SHADOW),
    (5, 14, SHADOW), (10, 14, SHADOW),

    # Outer rim (same octagon)
    (5, 3, IRON_DARK), (6, 3, IRON_DARK), (7, 3, IRON_DARK),
    (8, 3, IRON_DARK), (9, 3, IRON_DARK), (10, 3, IRON_DARK),
    (4, 4, IRON_DARK), (11, 4, IRON_DARK),
    (3, 5, IRON_DARK), (12, 5, IRON_DARK),
    (3, 6, IRON_DARK), (12, 6, IRON_DARK),
    (3, 7, IRON_DARK), (12, 7, IRON_DARK),
    (3, 8, IRON_DARK), (12, 8, IRON_DARK),
    (3, 9, IRON_DARK), (12, 9, IRON_DARK),
    (3, 10, IRON_DARK), (12, 10, IRON_DARK),
    (4, 11, IRON_DARK), (11, 11, IRON_DARK),
    (5, 12, IRON_DARK), (6, 12, IRON_DARK), (7, 12, IRON_DARK),
    (8, 12, IRON_DARK), (9, 12, IRON_DARK), (10, 12, IRON_DARK),

    # Highlight on top-left rim
    (5, 4, IRON_HI), (6, 4, IRON_HI), (7, 4, IRON_HI),
    (4, 5, IRON_HI), (4, 6, IRON_HI),

    # Rim mid fill
    (5, 5, IRON_MID), (6, 5, IRON_MID), (7, 5, IRON_MID),
    (8, 5, IRON_MID), (9, 5, IRON_MID), (10, 5, IRON_MID),
    (8, 4, IRON_MID), (9, 4, IRON_MID), (10, 4, IRON_MID),
    (4, 7, IRON_MID), (4, 8, IRON_MID), (4, 9, IRON_MID), (4, 10, IRON_MID),
    (11, 5, IRON_MID), (11, 6, IRON_MID), (11, 7, IRON_MID),
    (11, 8, IRON_MID), (11, 9, IRON_MID), (11, 10, IRON_MID),

    # Jaws clamped shut — central iron body fills cavity, teeth meet
    # in a serrated seam across the middle.
    (5, 6, IRON_MID), (6, 6, IRON_MID), (7, 6, IRON_MID),
    (8, 6, IRON_MID), (9, 6, IRON_MID), (10, 6, IRON_MID),
    (5, 7, IRON_MID), (6, 7, IRON_MID), (7, 7, IRON_MID),
    (8, 7, IRON_MID), (9, 7, IRON_MID), (10, 7, IRON_MID),
    (5, 9, IRON_MID), (6, 9, IRON_MID), (7, 9, IRON_MID),
    (8, 9, IRON_MID), (9, 9, IRON_MID), (10, 9, IRON_MID),
    (5, 10, IRON_MID), (6, 10, IRON_MID), (7, 10, IRON_MID),
    (8, 10, IRON_MID), (9, 10, IRON_MID), (10, 10, IRON_MID),

    # Bright meeting line down the middle: alternating tooth pixels
    (5, 8, TOOTH), (6, 8, IRON_DARK), (7, 8, TOOTH),
    (8, 8, IRON_DARK), (9, 8, TOOTH), (10, 8, IRON_DARK),

    # Blood smears around the seam
    (6, 7, BLOOD_DARK),
    (9, 7, BLOOD),
    (7, 9, BLOOD),
    (8, 9, BLOOD_DARK),

    # Chain stub
    (13, 7, CHAIN), (14, 7, CHAIN),
    (13, 8, CHAIN),
]


def main():
    render(ARMED, "assets/overworld_objects/bear_trap/armed.png")
    render(SPRUNG, "assets/overworld_objects/bear_trap/sprung.png")


if __name__ == "__main__":
    main()
