#!/usr/bin/env python3
"""Generate the fireball_missile projectile sprite.

A small glowing sphere of fire with a short trailing flame, drawn on a 32x32
transparent canvas. Used as the flying missile for the Fireball spell
(`ProjectileSpec.sprite = fireball_missile`). The projectile renderer scales
this to the spell's on-screen size, so we only care about the silhouette and
shading here.
"""

from pathlib import Path

from PIL import Image

SIZE = 32
OUT = Path("assets/overworld_objects/fireball_missile/sprite.png")

# Palette (RGBA) — from hot white core out to a dark ember edge.
CORE = (255, 248, 220, 255)
HOT = (255, 220, 120, 255)
MID = (255, 160, 50, 255)
EDGE = (232, 96, 28, 255)
EMBER = (170, 56, 16, 220)
TRAIL = (236, 130, 44, 170)
TRAIL_FAINT = (220, 96, 30, 90)


def main() -> None:
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    px = img.load()

    # The ball sits slightly right-of-center; the trail streams to the left so
    # the missile reads as moving rightward (orientation is cosmetic).
    cx, cy = 19.5, 16.0

    def disc(centre_x, centre_y, radius, color):
        r2 = radius * radius
        for y in range(SIZE):
            for x in range(SIZE):
                dx = x + 0.5 - centre_x
                dy = y + 0.5 - centre_y
                if dx * dx + dy * dy <= r2:
                    px[x, y] = color

    # Trailing flame: a few faint discs streaming back and thinning out.
    disc(cx - 9.0, cy, 4.2, TRAIL_FAINT)
    disc(cx - 6.0, cy, 5.0, TRAIL)
    disc(cx - 3.0, cy, 6.0, MID)

    # The fireball body, layered hot core outward.
    disc(cx, cy, 7.0, EMBER)
    disc(cx, cy, 6.2, EDGE)
    disc(cx, cy, 4.8, MID)
    disc(cx, cy, 3.2, HOT)
    disc(cx - 0.6, cy - 0.6, 1.6, CORE)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    img.save(OUT)
    print(f"wrote {OUT} ({SIZE}x{SIZE})")


if __name__ == "__main__":
    main()
