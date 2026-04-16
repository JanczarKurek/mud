"""
Generates assets/overworld_objects/campfire/sheet.png
Sheet layout: 4 columns × 1 row, each frame 32×32 px  (128×32 total)
  Row 0: burn animation (4 frames, flickering fire)
"""

from PIL import Image
import os

FRAME_W = 32
FRAME_H = 32
COLS = 4
ROWS = 1
OUT_PATH = "assets/overworld_objects/campfire/sheet.png"

BG          = (0,   0,   0,   0)
STONE       = (110, 100,  90, 255)   # ash-grey stones in ring
STONE_DARK  = ( 70,  60,  52, 255)
ASH         = (160, 150, 130, 255)   # pale ash center
LOG         = ( 90,  55,  20, 255)   # dark wood log
LOG_HI      = (130,  80,  30, 255)
EMBER       = (220,  80,  10, 255)   # deep orange ember glow
FLAME_LO    = (220, 120,  10, 255)   # low flame orange
FLAME_MID   = (240, 190,  20, 255)   # bright yellow
FLAME_TIP   = (255, 240, 120, 255)   # near-white hot tip
SMOKE       = (180, 170, 160, 128)   # semi-transparent smoke


def make_frame(flame_height, flame_offset, smoke_show):
    """
    flame_height  – how tall the main flame column is (pixels)
    flame_offset  – x wobble offset for the flame tip (±1)
    smoke_show    – whether to draw a smoke wisp at the top
    """
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c):
        for dy in range(h):
            for dx in range(w):
                px(x + dx, y + dy, c)

    # ── Stone ring (bottom, y: 22-28) ───────────────────────────────────────
    # Rough oval of stones
    stone_coords = [
        (11,24),(12,24),(13,24),(14,24),(15,24),(16,24),(17,24),(18,24),(19,24),(20,24),
        (10,25),(10,26),(10,27),(21,25),(21,26),(21,27),
        (11,28),(12,28),(13,28),(14,28),(15,28),(16,28),(17,28),(18,28),(19,28),(20,28),
    ]
    for (sx, sy) in stone_coords:
        px(sx, sy, STONE)
    # Mortar/shadow
    for (sx, sy) in stone_coords[:5]:
        px(sx, sy-1, STONE_DARK)

    # ── Ash center ──────────────────────────────────────────────────────────
    rect(12, 25, 8, 3, ASH)

    # ── Logs (two crossing logs) ────────────────────────────────────────────
    # Horizontal log
    rect(11, 25, 10, 2, LOG)
    rect(11, 25, 10,  1, LOG_HI)
    # Diagonal log (pixel by pixel)
    for i in range(8):
        px(12 + i, 27 - i//2, LOG)

    # ── Ember glow at base of flame ─────────────────────────────────────────
    rect(13, 22, 6, 3, EMBER)

    # ── Flame column ────────────────────────────────────────────────────────
    # Base (widest)
    flame_base_y = 21
    base_x = 12 + flame_offset // 2

    # Draw from base up
    for i in range(flame_height):
        y = flame_base_y - i
        # Width narrows as we go up
        w = max(1, 8 - i)
        x = base_x + (8 - w) // 2
        if i < 2:
            c = FLAME_LO
        elif i < flame_height - 2:
            c = FLAME_MID
        else:
            c = FLAME_TIP
        rect(x + flame_offset * (i // 3), y, w, 1, c)

    # Side flame wisps
    px(11 + flame_offset, flame_base_y - 1, FLAME_LO)
    px(11 + flame_offset, flame_base_y - 2, EMBER)
    px(20 - flame_offset, flame_base_y - 1, FLAME_LO)
    px(20 - flame_offset, flame_base_y - 2, EMBER)

    # ── Smoke ────────────────────────────────────────────────────────────────
    if smoke_show:
        smoke_x = 15 + flame_offset
        for i in range(4):
            px(smoke_x + (i % 2), flame_base_y - flame_height - i, SMOKE)

    return img


# Four frames: vary flame height and offset for flicker effect
frames = [
    make_frame(flame_height=7, flame_offset=0,  smoke_show=False),
    make_frame(flame_height=9, flame_offset=1,  smoke_show=True),
    make_frame(flame_height=8, flame_offset=-1, smoke_show=False),
    make_frame(flame_height=10, flame_offset=0, smoke_show=True),
]

sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
for col, frame in enumerate(frames):
    sheet.paste(frame, (col * FRAME_W, 0))

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
sheet.save(OUT_PATH)
print(f"Saved {OUT_PATH}  ({sheet.width}×{sheet.height})")
