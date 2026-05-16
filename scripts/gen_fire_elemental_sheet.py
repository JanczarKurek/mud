"""
Generates assets/overworld_objects/fire_elemental/sheet.png  and  sprite.png
Sheet layout: 4 columns x 2 rows, each frame 64x80 px (cyclops-scale)
Sheet size: 256x160 px
  Row 0: idle  (4 frames - body flickers, core pulses, ember sparks rise)
  Row 1: walk  (4 frames - body sways with rising flame trails)

Fire elemental is a teardrop / flame-shaped column of fire, NOT a humanoid box.
The silhouette is built row-by-row with per-row half-widths that taper from a
wide base to a pointed crown, with frame-by-frame flicker jitter so no edge
ever looks straight.
"""

from PIL import Image
import os

FRAME_W = 64
FRAME_H = 80
COLS    = 4
ROWS    = 2
OUT_SHEET  = "assets/overworld_objects/fire_elemental/sheet.png"
OUT_SPRITE = "assets/overworld_objects/fire_elemental/sprite.png"

# Palette - layered fire from dark outline to white-hot core
BG          = (  0,   0,   0,   0)
FLAME_DARK  = (110,  18,   8, 255)   # outer flame outline / shadow
FLAME_RED   = (200,  42,  16, 255)   # outer body
FLAME_ORG   = (240, 108,  24, 255)   # mid flame
FLAME_YLW   = (252, 192,  46, 255)   # inner flame
FLAME_WHT   = (255, 244, 196, 255)   # hot highlight
CORE_HOT    = (255, 230, 130, 255)   # molten core glow
CORE_PEAK   = (255, 255, 235, 255)   # core white-hot peak
EMBER       = (255, 168,  64, 255)   # rising sparks
EMBER_DIM   = (190,  78,  20, 255)
SMOKE       = ( 88,  56,  44, 255)   # subtle dark wisp

CENTER_X = 32

# Body silhouette half-widths by y. y=8 is the top, y=72 is the wide base.
# These define the resting silhouette; flicker jitter is added per-frame.
# Tuned so the shape reads as a teardrop / candle flame.
def base_silhouette():
    rows = {}
    # Crown tip (very narrow, pointed)
    for y in range(8, 12):
        rows[y] = 2
    # Crown spreading
    rows[12] = 3
    rows[13] = 4
    rows[14] = 5
    rows[15] = 6
    # Upper body widening
    rows[16] = 8
    rows[17] = 9
    rows[18] = 10
    rows[19] = 11
    rows[20] = 12
    rows[21] = 13
    rows[22] = 14
    rows[23] = 15
    rows[24] = 16
    rows[25] = 17
    # Shoulder bulge
    rows[26] = 18
    rows[27] = 19
    rows[28] = 20
    rows[29] = 21
    rows[30] = 21
    rows[31] = 21
    # Body pinch (waist) - subtle hourglass
    rows[32] = 20
    rows[33] = 19
    rows[34] = 19
    # Widening hips
    rows[35] = 20
    rows[36] = 21
    rows[37] = 22
    rows[38] = 23
    rows[39] = 24
    rows[40] = 24
    rows[41] = 24
    rows[42] = 24
    rows[43] = 24
    # Lower body bell
    rows[44] = 23
    rows[45] = 22
    rows[46] = 22
    rows[47] = 23
    rows[48] = 24
    rows[49] = 25
    rows[50] = 26
    # Wide bottom
    rows[51] = 26
    rows[52] = 26
    rows[53] = 26
    rows[54] = 26
    rows[55] = 26
    rows[56] = 25
    rows[57] = 25
    rows[58] = 24
    rows[59] = 23
    rows[60] = 22
    rows[61] = 21
    rows[62] = 20
    # Trailing base ribbon
    rows[63] = 19
    rows[64] = 18
    rows[65] = 16
    rows[66] = 14
    return rows


def make_frame(body_dy=0, jitter_seed=0, core_pulse=0, sway=0,
               crown_lash=0, ember_phase=0,
               left_lick=0, right_lick=0):
    """body_dy: vertical bob; jitter_seed: which edge-jitter table to use;
    core_pulse: extra brightness/size at the core; sway: horizontal lean;
    crown_lash: extra height on the crown tip; ember_phase: which spark set;
    left_lick/right_lick: an outward bulge on one side at mid-body (arm
    flame tongues whipping)."""
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def hline(x_left, x_right, y, c):
        if not (0 <= y < FRAME_H):
            return
        x0 = max(0, x_left)
        x1 = min(FRAME_W - 1, x_right)
        for x in range(x0, x1 + 1):
            img.putpixel((x, y), c)

    rows = base_silhouette()

    # Per-frame jitter tables. Each entry adds +/-1 to the half-width on
    # the left and right at a given row, giving the silhouette its
    # crackling, irregular fire edge. Four tables cycled by jitter_seed.
    jitter_tables = [
        # (y, left_delta, right_delta)
        [(14,  0,  1), (18, -1,  0), (22,  1, -1), (28,  0,  1), (34, -1,  1),
         (40,  1, -1), (46,  0,  1), (52, -1,  0), (58,  1,  0), (62, -1,  1)],
        [(13,  1,  0), (19,  0, -1), (24, -1,  1), (29,  1,  0), (36,  0, -1),
         (41, -1,  1), (47,  1,  0), (53,  0,  1), (57, -1,  0), (61,  0, -1)],
        [(15, -1,  1), (20,  1,  0), (25,  0, -1), (30, -1,  1), (37,  1, -1),
         (42,  0,  1), (48, -1,  0), (54,  1,  1), (59,  0, -1), (63,  1,  0)],
        [(16,  0, -1), (21, -1,  1), (26,  1,  0), (31,  0,  1), (38, -1,  0),
         (43,  1,  1), (49,  0, -1), (55, -1,  0), (60,  1,  0), (64, -1,  1)],
    ]
    jt = jitter_tables[jitter_seed % 4]
    left_extra = {y: dl for (y, dl, _) in jt}
    right_extra = {y: dr for (y, _, dr) in jt}

    bd = body_dy
    sw = sway

    # Apply crown_lash by extending the crown rows upward
    if crown_lash > 0:
        for k in range(crown_lash):
            rows[8 - k - 1] = max(1, 2 - k // 2)

    # Apply arm-flame "licks" - bulge mid-body silhouette outward asymmetrically
    if left_lick:
        for y in range(28, 36):
            rows[y] = rows.get(y, 0) + 3
            left_extra[y] = left_extra.get(y, 0) + 2
    if right_lick:
        for y in range(28, 36):
            rows[y] = rows.get(y, 0) + 3
            right_extra[y] = right_extra.get(y, 0) + 2

    # Draw the body as concentric color rings.
    # Outermost: FLAME_DARK (outline)
    # Then:      FLAME_RED  (1 px in)
    # Then:      FLAME_ORG  (2 px in)
    # Then:      FLAME_YLW  (3 px in, only where wide enough)
    # Then:      core/highlights drawn separately
    sorted_ys = sorted(rows.keys())
    for y in sorted_ys:
        hw = rows[y]
        ly = y + bd
        l = CENTER_X + sw - hw - left_extra.get(y, 0)
        r = CENTER_X + sw + hw + right_extra.get(y, 0)
        # Outline
        hline(l, r, ly, FLAME_DARK)
        # Inner red (inset by 1)
        if r - l >= 2:
            hline(l + 1, r - 1, ly, FLAME_RED)
        # Mid orange (inset by 2)
        if r - l >= 4:
            hline(l + 2, r - 2, ly, FLAME_ORG)
        # Inner yellow (inset by 4) - only on wide rows
        if r - l >= 8:
            hline(l + 4, r - 4, ly, FLAME_YLW)

    # Molten core - sits in mid-torso, brightest spot.
    core_cx = CENTER_X + sw
    core_cy = 38 + bd
    core_r = 5 + core_pulse
    # Outer glow ring
    for y in range(core_cy - core_r, core_cy + core_r + 1):
        dy = y - core_cy
        if abs(dy) > core_r:
            continue
        span = int((core_r * core_r - dy * dy) ** 0.5)
        hline(core_cx - span, core_cx + span, y, CORE_HOT)
    # Inner white-hot
    inner_r = max(2, core_r - 2)
    for y in range(core_cy - inner_r, core_cy + inner_r + 1):
        dy = y - core_cy
        if abs(dy) > inner_r:
            continue
        span = int((inner_r * inner_r - dy * dy) ** 0.5)
        hline(core_cx - span, core_cx + span, y, CORE_PEAK)

    # Eye-glow embers - twin bright points near the crown
    eye_y = 22 + bd
    eye_l = CENTER_X + sw - 4
    eye_r = CENTER_X + sw + 4
    px(eye_l,     eye_y,     FLAME_DARK)
    px(eye_l + 1, eye_y,     CORE_HOT)
    px(eye_l + 1, eye_y - 1, CORE_PEAK)
    px(eye_r,     eye_y,     FLAME_DARK)
    px(eye_r - 1, eye_y,     CORE_HOT)
    px(eye_r - 1, eye_y - 1, CORE_PEAK)

    # Swirling diagonal tendrils across the inner body - hint at rotation.
    for i in range(7):
        px(CENTER_X + sw - 10 + i, 34 + bd + i, FLAME_YLW)
        px(CENTER_X + sw + 10 - i, 34 + bd + i, FLAME_YLW)
        px(CENTER_X + sw - 12 + i, 48 + bd - i, EMBER)
        px(CENTER_X + sw + 12 - i, 48 + bd - i, EMBER)

    # Faint trailing smoke wisps near the base
    smoke_y = 70 + bd
    for sx in (CENTER_X - 16 + sw, CENTER_X - 8 + sw, CENTER_X + 8 + sw, CENTER_X + 16 + sw):
        px(sx, smoke_y, SMOKE)

    # Rising ember sparks - vary by phase so they look in motion.
    # Each ember is 1 hot pixel + 1 dim pixel below it.
    spark_sets = [
        [(-22, 26), (24, 22), (0, 6), (-18, 44), (20, 40)],
        [(-20, 18), (22, 30), (-2, 2),  (-14, 36), (18, 48)],
        [(-25, 28), (26, 18), (4,  6), (-10, 38), (22, 42)],
        [(-19, 14), (20, 26), (-4, 0), (-22, 32), (16, 36)],
    ]
    palette = (EMBER, EMBER_DIM)
    for i, (dx, ey) in enumerate(spark_sets[ember_phase % 4]):
        x = CENTER_X + sw + dx
        y = ey + bd
        ec = palette[i % 2]
        px(x, y, ec)
        px(x, y + 1, EMBER_DIM)

    # Extra crown-tip sparks for animation pop
    if crown_lash > 0:
        tip_y = 8 + bd - crown_lash - 1
        px(CENTER_X + sw, tip_y, CORE_PEAK)
        px(CENTER_X + sw - 1, tip_y + 1, EMBER)
        px(CENTER_X + sw + 1, tip_y + 1, EMBER)

    return img


# Frame definitions

idle_frames = [
    make_frame(body_dy= 0, jitter_seed=0, core_pulse=0, crown_lash=0, ember_phase=0),
    make_frame(body_dy=-1, jitter_seed=1, core_pulse=1, crown_lash=1, ember_phase=1),
    make_frame(body_dy= 0, jitter_seed=2, core_pulse=0, crown_lash=2, ember_phase=2),
    make_frame(body_dy=-1, jitter_seed=3, core_pulse=1, crown_lash=1, ember_phase=3),
]

walk_frames = [
    make_frame(body_dy=-1, sway=-1, jitter_seed=0, core_pulse=1, crown_lash=1, ember_phase=0, left_lick=1),
    make_frame(body_dy= 1, sway= 0, jitter_seed=1, core_pulse=0, crown_lash=0, ember_phase=1),
    make_frame(body_dy=-1, sway= 1, jitter_seed=2, core_pulse=1, crown_lash=1, ember_phase=2, right_lick=1),
    make_frame(body_dy= 1, sway= 0, jitter_seed=3, core_pulse=0, crown_lash=0, ember_phase=3),
]

# Assemble sheet
sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
for col, frame in enumerate(idle_frames):
    sheet.paste(frame, (col * FRAME_W, 0))
for col, frame in enumerate(walk_frames):
    sheet.paste(frame, (col * FRAME_W, FRAME_H))

os.makedirs(os.path.dirname(OUT_SHEET), exist_ok=True)
sheet.save(OUT_SHEET)
print(f"Saved {OUT_SHEET}  ({sheet.width}x{sheet.height})")

sprite_full = idle_frames[0].copy()
sprite_full.save(OUT_SPRITE)
print(f"Saved {OUT_SPRITE}  ({sprite_full.width}x{sprite_full.height})")
