#!/usr/bin/env python3
"""Generate the arcane_mote projectile sprite.

A small darting mote of violet arcane force with a faint sparkle halo, on a
32x32 transparent canvas. Homing missile for the Magic Dart spell
(`ProjectileSpec.sprite = arcane_mote`).
"""

from pathlib import Path

from PIL import Image

SIZE = 32
OUT = Path("assets/overworld_objects/arcane_mote/sprite.png")

CORE = (245, 235, 255, 255)
HOT = (205, 170, 255, 255)
MID = (165, 120, 240, 255)
EDGE = (120, 80, 210, 235)
HALO = (140, 100, 230, 90)
SPARK = (230, 215, 255, 200)


def main() -> None:
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    px = img.load()
    cx, cy = 16.0, 16.0

    def disc(centre_x, centre_y, radius, color):
        r2 = radius * radius
        for y in range(SIZE):
            for x in range(SIZE):
                dx = x + 0.5 - centre_x
                dy = y + 0.5 - centre_y
                if dx * dx + dy * dy <= r2:
                    px[x, y] = color

    # Soft outer halo, then the bright mote core.
    disc(cx, cy, 7.5, HALO)
    disc(cx, cy, 5.0, EDGE)
    disc(cx, cy, 3.8, MID)
    disc(cx, cy, 2.4, HOT)
    disc(cx - 0.5, cy - 0.5, 1.1, CORE)

    # Four-point sparkle: short arms reaching out from the core.
    for d in range(2, 9):
        for (x, y) in ((cx + d, cy), (cx - d, cy), (cx, cy + d), (cx, cy - d)):
            xi, yi = int(x), int(y)
            if 0 <= xi < SIZE and 0 <= yi < SIZE and d <= 7:
                # Fade the arms toward the tips.
                a = max(60, SPARK[3] - (d - 2) * 22)
                px[xi, yi] = (SPARK[0], SPARK[1], SPARK[2], a)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    img.save(OUT)
    print(f"wrote {OUT} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()
