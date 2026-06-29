#!/usr/bin/env python3
"""Generate the lightning_spark VFX sheet.

Six 48x48 frames of an electric burst: a bright node at the tile center with a
handful of jagged branches that flare out, peak, then dissipate. Played once
per tile by the Chain Spark spell's spiral pattern (`vfx_on_tile`).

Deterministic: branch geometry is derived from fixed integer seeds so the sheet
is byte-stable across runs (no RNG module needed).
"""

import math
from pathlib import Path

from PIL import Image

FRAME = 48
FRAMES = 6
OUT = Path("assets/vfx/lightning_spark/sheet.png")

CORE = (245, 250, 255, 255)
HOT = (180, 225, 255, 255)
ARC = (120, 190, 255, 255)
DIM = (90, 150, 240, 220)
GLOW = (120, 180, 255, 70)


def lerp(a, b, t):
    return a + (b - a) * t


def blend(px, x, y, color):
    """Alpha-over a color onto pixel (x, y), if in bounds."""
    if not (0 <= x < FRAME and 0 <= y < FRAME):
        return
    sr, sg, sb, sa = color
    a = sa / 255.0
    dr, dg, db, da = px[x, y]
    px[x, y] = (
        int(sr * a + dr * (1 - a)),
        int(sg * a + dg * (1 - a)),
        int(sb * a + db * (1 - a)),
        max(da, sa),
    )


def draw_bolt(px, cx, cy, angle, length, color):
    """A jagged segmented bolt from the center outward along `angle`."""
    steps = max(3, int(length))
    x, y = cx, cy
    # Pseudo-random zigzag from a fixed hash of the angle.
    seed = int(angle * 1000) & 0xFFFF
    for i in range(steps):
        t = i / steps
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        jitter = ((seed >> 8) % 5 - 2) * (0.6 * t)
        perp = angle + math.pi / 2
        nx = cx + math.cos(angle) * length * t + math.cos(perp) * jitter
        ny = cy + math.sin(angle) * length * t + math.sin(perp) * jitter
        # Draw a short thick segment between (x, y) and (nx, ny).
        seg = max(1, int(math.hypot(nx - x, ny - y)) + 1)
        for s in range(seg + 1):
            u = s / seg
            bx = int(round(lerp(x, nx, u)))
            by = int(round(lerp(y, ny, u)))
            blend(px, bx, by, color)
            if t < 0.6:  # thicken the inner part of the bolt
                blend(px, bx + 1, by, color)
        x, y = nx, ny


def make_frame(frame_idx):
    img = Image.new("RGBA", (FRAME, FRAME), (0, 0, 0, 0))
    px = img.load()
    cx, cy = FRAME / 2, FRAME / 2

    t = frame_idx / (FRAMES - 1)  # 0..1 over the animation
    # Envelope: flare in over the first third, fade out after the peak.
    if t < 0.33:
        intensity = t / 0.33
    else:
        intensity = max(0.0, 1.0 - (t - 0.33) / 0.67)
    if intensity <= 0.0:
        return img

    reach = lerp(8.0, 20.0, t)  # branches grow outward over time

    # Soft glow halo behind the bolts.
    gr = int(lerp(5, 16, t))
    for y in range(FRAME):
        for x in range(FRAME):
            if (x - cx) ** 2 + (y - cy) ** 2 <= gr * gr:
                ga = int(GLOW[3] * intensity)
                blend(px, x, y, (GLOW[0], GLOW[1], GLOW[2], ga))

    # Six radiating bolts at staggered angles (rotate a little per frame).
    n_bolts = 6
    base = frame_idx * 0.4
    body = ARC if t < 0.5 else DIM
    for k in range(n_bolts):
        ang = base + k * (2 * math.pi / n_bolts)
        col = (body[0], body[1], body[2], int(body[3] * intensity))
        draw_bolt(px, cx, cy, ang, reach, col)

    # Bright central node, shrinking as the burst fades.
    node = max(1, int(lerp(4, 1, t)))
    for y in range(FRAME):
        for x in range(FRAME):
            d2 = (x - cx) ** 2 + (y - cy) ** 2
            if d2 <= node * node:
                blend(px, x, y, (CORE[0], CORE[1], CORE[2], int(255 * intensity)))
            elif d2 <= (node + 1) ** 2:
                blend(px, x, y, (HOT[0], HOT[1], HOT[2], int(220 * intensity)))

    return img


def main() -> None:
    sheet = Image.new("RGBA", (FRAME * FRAMES, FRAME), (0, 0, 0, 0))
    for i in range(FRAMES):
        sheet.paste(make_frame(i), (i * FRAME, 0))
    OUT.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(OUT)
    print(f"wrote {OUT} ({FRAME * FRAMES}x{FRAME}, {FRAMES} frames)")


if __name__ == "__main__":
    main()
