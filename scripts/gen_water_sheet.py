"""
Generates assets/overworld_objects/water/sheet.png

Sheet layout: 4 columns × 1 row, each frame authored at 16×16 px then
nearest-neighbor scaled 3× to 48×48 (total 192×48). The 3× scale matches
how the static sprite.png is rendered in-game (tile_size = 48, debug_size
= 1.0), so enabling animation doesn't change the displayed size.

A single looping `idle` clip — subtle highlight drift on a deep-blue base,
sampled from the existing static sprite.png palette.
"""

from PIL import Image

AUTHOR_W = 16
AUTHOR_H = 16
SCALE = 3
FRAME_W = AUTHOR_W * SCALE
FRAME_H = AUTHOR_H * SCALE
COLS = 4
ROWS = 1
OUT_PATH = "assets/overworld_objects/water/sheet.png"

# ── Palette ────────────────────────────────────────────────────────────────────
# Base + highlight are sampled directly from the existing sprite.png.
# Mid and deep are derived shades for the ripple animation.
BG        = (0, 0, 0, 0)
BASE      = (47,  111, 201, 255)  # deep blue body (from sprite.png)
HIGHLIGHT = (111, 182, 255, 255)  # glint (from sprite.png)
MID       = (74,  142, 226, 255)  # halfway between base and highlight
DEEP      = (32,   82, 158, 255)  # slightly darker for occasional shadow flecks


# Highlight glints drift one pixel between frames to suggest a slow
# surface ripple. Two long-lived glints + one short-lived sparkle per
# frame keeps the motion subtle (Tibia-style; no flashy splashes).
#
# Each entry: list of (x, y, color) drawn after the flat base fill.
FRAMES = [
    # Frame 0
    [
        (3, 4,  HIGHLIGHT),
        (10, 11, HIGHLIGHT),
        (6,  9,  MID),
        (13, 3,  MID),
        (1,  13, DEEP),
    ],
    # Frame 1 — glints slid right by one px, sparkle drifts down-right
    [
        (4, 4,  HIGHLIGHT),
        (11, 11, HIGHLIGHT),
        (7,  9,  MID),
        (13, 4,  MID),
        (2,  13, DEEP),
        (8,  2,  MID),
    ],
    # Frame 2 — glints slid down by one px, additional faint mid blob
    [
        (4, 5,  HIGHLIGHT),
        (11, 12, HIGHLIGHT),
        (7,  10, MID),
        (12, 4,  MID),
        (2,  14, DEEP),
        (5,  7,  MID),
    ],
    # Frame 3 — glints slid left, returning toward frame 0 next cycle
    [
        (3, 5,  HIGHLIGHT),
        (10, 12, HIGHLIGHT),
        (6,  10, MID),
        (12, 3,  MID),
        (1,  14, DEEP),
    ],
]


def make_frame(spec):
    # Author at 16x16 then nearest-neighbor scale to FRAME_W x FRAME_H.
    small = Image.new("RGBA", (AUTHOR_W, AUTHOR_H), BASE)
    for x, y, color in spec:
        if 0 <= x < AUTHOR_W and 0 <= y < AUTHOR_H:
            small.putpixel((x, y), color)
    return small.resize((FRAME_W, FRAME_H), Image.NEAREST)


def main():
    sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
    assert len(FRAMES) == COLS, f"expected {COLS} frames, got {len(FRAMES)}"
    for col, spec in enumerate(FRAMES):
        frame = make_frame(spec)
        sheet.paste(frame, (col * FRAME_W, 0))
    sheet.save(OUT_PATH)
    print(f"wrote {OUT_PATH} ({sheet.size[0]}x{sheet.size[1]} px, {COLS} frames at {FRAME_W}x{FRAME_H})")


if __name__ == "__main__":
    main()
