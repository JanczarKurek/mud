"""
Generates assets/overworld_objects/blazing_fire/sheet.png

Sheet layout: 4 columns × 1 row, each frame authored at 16x16 px and
nearest-neighbor scaled 3x to 48x48 (total 192x48). Matches the water-tile
convention (author small, scale 3x) so the chunky pixel-art style is
consistent with the rest of the overworld props.

A single looping `idle` clip — 4 frames of flame flicker. Fills most of the
tile (debug_size 0.85 in metadata.yaml).
"""

from PIL import Image
import os

AUTHOR_W = 16
AUTHOR_H = 16
SCALE = 3
FRAME_W = AUTHOR_W * SCALE
FRAME_H = AUTHOR_H * SCALE
COLS = 4
ROWS = 1
OUT_PATH = "assets/overworld_objects/blazing_fire/sheet.png"

# ── Palette ───────────────────────────────────────────────────────────────
BG       = (0,   0,   0,   0)
EMBER    = (140,  30,  10, 255)   # darkest red base — coals
DEEP     = (210,  60,  20, 255)   # deep orange-red
ORANGE   = (240, 120,  30, 255)   # main flame body
YELLOW   = (250, 200,  60, 255)   # mid-flame
HOT      = (255, 240, 150, 255)   # bright tip
SPARK    = (255, 255, 220, 200)   # near-white airborne speck

# Per-frame shape spec. Each entry is a list of (x, y, color) for the
# 16x16 author grid; the base is transparent, the flame is built up
# pixel-by-pixel. Frames vary the flame silhouette to suggest licking
# motion.

def base_flame():
    """Coal bed common across all frames."""
    return [
        (5, 13, EMBER), (6, 13, EMBER), (7, 13, EMBER), (8, 13, EMBER),
        (9, 13, EMBER), (10, 13, EMBER),
        (4, 12, EMBER), (11, 12, EMBER),
        (5, 12, DEEP),  (6, 12, DEEP),  (7, 12, DEEP),
        (8, 12, DEEP),  (9, 12, DEEP),  (10, 12, DEEP),
    ]


FRAMES = [
    # Frame 0 — tall central column, mild side wisps
    base_flame() + [
        (6, 11, DEEP),   (7, 11, ORANGE), (8, 11, ORANGE), (9, 11, DEEP),
        (6, 10, ORANGE), (7, 10, ORANGE), (8, 10, ORANGE), (9, 10, ORANGE),
        (6,  9, ORANGE), (7,  9, YELLOW), (8,  9, YELLOW), (9,  9, ORANGE),
        (7,  8, YELLOW), (8,  8, YELLOW),
        (7,  7, HOT),    (8,  7, YELLOW),
        (8,  6, HOT),
        (4, 11, DEEP),   (11, 11, DEEP),
        (5, 10, ORANGE), (10, 10, ORANGE),
        (12,  9, SPARK),
    ],
    # Frame 1 — flame leans right, taller crown
    base_flame() + [
        (6, 11, DEEP),   (7, 11, ORANGE), (8, 11, ORANGE), (9, 11, ORANGE),
        (6, 10, ORANGE), (7, 10, ORANGE), (8, 10, ORANGE), (9, 10, YELLOW),
        (7,  9, ORANGE), (8,  9, YELLOW), (9,  9, YELLOW),
        (8,  8, YELLOW), (9,  8, YELLOW),
        (9,  7, HOT),    (8,  7, YELLOW),
        (9,  6, HOT),
        (4, 11, ORANGE),
        (10, 11, ORANGE),
        (10,  9, ORANGE), (11, 10, DEEP),
        (3, 10, SPARK),
    ],
    # Frame 2 — wider base, lower crown, side licks
    base_flame() + [
        (5, 11, DEEP),   (6, 11, ORANGE), (7, 11, ORANGE), (8, 11, ORANGE),
        (9, 11, ORANGE), (10, 11, DEEP),
        (6, 10, ORANGE), (7, 10, YELLOW), (8, 10, YELLOW), (9, 10, ORANGE),
        (7,  9, YELLOW), (8,  9, YELLOW),
        (7,  8, HOT),    (8,  8, HOT),
        (8,  7, HOT),
        (4, 12, DEEP),   (11, 12, DEEP),
        (4, 11, ORANGE), (11, 11, ORANGE),
        (5,  9, DEEP),   (10,  9, DEEP),
        (13, 11, SPARK),
    ],
    # Frame 3 — flame leans left, tongue arcs upward
    base_flame() + [
        (5, 11, DEEP),   (6, 11, ORANGE), (7, 11, ORANGE), (8, 11, ORANGE),
        (9, 11, ORANGE),
        (5, 10, ORANGE), (6, 10, YELLOW), (7, 10, YELLOW), (8, 10, ORANGE),
        (5,  9, ORANGE), (6,  9, YELLOW), (7,  9, YELLOW),
        (6,  8, HOT),    (7,  8, YELLOW),
        (6,  7, HOT),
        (6,  6, HOT),
        (4, 11, ORANGE),
        (10, 11, DEEP),
        (9, 10, DEEP),
        (3, 12, SPARK),
    ],
]


def make_frame(spec):
    small = Image.new("RGBA", (AUTHOR_W, AUTHOR_H), BG)
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
    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
    sheet.save(OUT_PATH)
    print(f"wrote {OUT_PATH} ({sheet.size[0]}x{sheet.size[1]} px, {COLS} frames at {FRAME_W}x{FRAME_H})")


if __name__ == "__main__":
    main()
