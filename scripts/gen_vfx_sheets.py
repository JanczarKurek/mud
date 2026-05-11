"""
Generates 48x48 sprite sheets for all built-in VFX effects.

One PNG per effect at assets/vfx/<id>/sheet.png. Each sheet is a single-row
strip of frames the size of one tile (48x48), played by the "play" clip in
the corresponding metadata.yaml. One-shots are non-looping; sticky overlays
are looping.

Run with:
    nix-shell -p python3Packages.pillow --run \
        "python3 scripts/gen_vfx_sheets.py"
"""

from __future__ import annotations

import math
import os
from pathlib import Path

from PIL import Image

FRAME = 48
ASSETS = Path(__file__).resolve().parent.parent / "assets" / "vfx"
BG = (0, 0, 0, 0)


def blank_sheet(frame_count: int) -> Image.Image:
    return Image.new("RGBA", (FRAME * frame_count, FRAME), BG)


def paste_frame(sheet: Image.Image, frame: Image.Image, idx: int) -> None:
    sheet.paste(frame, (idx * FRAME, 0), frame)


def save_sheet(sheet: Image.Image, effect_id: str) -> None:
    out_dir = ASSETS / effect_id
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / "sheet.png"
    sheet.save(out_path)
    print(f"wrote {out_path}")


def draw_disc(img: Image.Image, cx: float, cy: float, r: float, color) -> None:
    if r <= 0:
        return
    r2 = r * r
    for y in range(int(cy - r) - 1, int(cy + r) + 2):
        for x in range(int(cx - r) - 1, int(cx + r) + 2):
            if 0 <= x < img.width and 0 <= y < img.height:
                dx = x + 0.5 - cx
                dy = y + 0.5 - cy
                if dx * dx + dy * dy <= r2:
                    img.putpixel((x, y), color)


def draw_ring(img: Image.Image, cx: float, cy: float, r: float, thickness: float, color) -> None:
    if r <= 0:
        return
    r_outer2 = (r + thickness * 0.5) ** 2
    r_inner2 = max(0.0, r - thickness * 0.5) ** 2
    rng = int(r + thickness) + 1
    for y in range(int(cy) - rng, int(cy) + rng + 1):
        for x in range(int(cx) - rng, int(cx) + rng + 1):
            if 0 <= x < img.width and 0 <= y < img.height:
                dx = x + 0.5 - cx
                dy = y + 0.5 - cy
                d2 = dx * dx + dy * dy
                if r_inner2 <= d2 <= r_outer2:
                    img.putpixel((x, y), color)


def alpha(color, a: float):
    a = max(0.0, min(1.0, a))
    return (color[0], color[1], color[2], int(round(color[3] * a)))


# ─── One-shot effects ─────────────────────────────────────────────────────────


def gen_blood_splash() -> None:
    """Six frames: a red splatter that bursts outward and fades."""
    frames = 6
    sheet = blank_sheet(frames)
    base = (176, 28, 24, 255)
    dark = (108, 12, 12, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / (frames - 1)
        r_core = 4 + 6 * t
        a_core = 1.0 - 0.5 * t
        draw_disc(img, FRAME / 2, FRAME / 2, r_core, alpha(base, a_core))
        # splatter dots
        for ang_deg, dist_mul in [(20, 1.0), (95, 0.85), (165, 1.1), (215, 0.9), (305, 1.05)]:
            ang = math.radians(ang_deg + i * 5)
            d = (6 + 10 * t) * dist_mul
            x = FRAME / 2 + math.cos(ang) * d
            y = FRAME / 2 + math.sin(ang) * d
            draw_disc(img, x, y, 2 - 0.7 * t, alpha(dark, 1.0 - t * 0.6))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "blood_splash")


def gen_cast_flash() -> None:
    """Six frames: golden ring expanding outward from caster."""
    frames = 6
    sheet = blank_sheet(frames)
    gold = (252, 220, 96, 255)
    gold_dark = (200, 150, 30, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / (frames - 1)
        r = 4 + 14 * t
        a = 1.0 - t * 0.85
        draw_ring(img, FRAME / 2, FRAME / 2, r, 2.0, alpha(gold, a))
        draw_ring(img, FRAME / 2, FRAME / 2, r + 1, 1.0, alpha(gold_dark, a * 0.6))
        if i < 3:
            draw_disc(img, FRAME / 2, FRAME / 2, 2 + i, alpha(gold, 0.8 - 0.2 * i))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "cast_flash")


def gen_hit_flash() -> None:
    """Six frames: white/cyan starburst snapping outward."""
    frames = 6
    sheet = blank_sheet(frames)
    white = (255, 255, 255, 255)
    cyan = (140, 220, 255, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / (frames - 1)
        cx, cy = FRAME / 2, FRAME / 2
        # cross spikes
        spike_len = int(4 + 14 * t)
        a = 1.0 - t
        for k in range(spike_len):
            kt = k / max(1, spike_len)
            color = alpha(white if kt < 0.5 else cyan, a * (1.0 - kt * 0.5))
            for (dx, dy) in [(k, 0), (-k, 0), (0, k), (0, -k)]:
                xi = int(cx + dx)
                yi = int(cy + dy)
                if 0 <= xi < FRAME and 0 <= yi < FRAME:
                    img.putpixel((xi, yi), color)
        # diagonal weaker spikes
        spike_d = int(2 + 10 * t)
        for k in range(spike_d):
            color = alpha(cyan, a * 0.6)
            for sx, sy in [(1, 1), (-1, 1), (1, -1), (-1, -1)]:
                xi = int(cx + sx * k)
                yi = int(cy + sy * k)
                if 0 <= xi < FRAME and 0 <= yi < FRAME:
                    img.putpixel((xi, yi), color)
        draw_disc(img, cx, cy, max(0.5, 4 - 4 * t), alpha(white, a))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "hit_flash")


def gen_heal_sparkle() -> None:
    """Six frames: green motes rising and fading."""
    frames = 6
    sheet = blank_sheet(frames)
    green = (140, 240, 140, 255)
    green_hi = (220, 255, 220, 255)
    motes = [
        # (x_offset, y_offset, phase)
        (-8, 14, 0.0),
        (0, 18, 0.15),
        (10, 12, 0.30),
        (-4, 8, 0.5),
        (6, 4, 0.65),
        (-10, 0, 0.8),
    ]
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / (frames - 1)
        for (mx, my, phase) in motes:
            local_t = (t - phase) % 1.0
            if local_t < 0.1:
                continue
            rise = local_t * 22
            x = FRAME / 2 + mx
            y = FRAME / 2 + my - rise
            a = max(0.0, 1.0 - local_t)
            draw_disc(img, x, y, 1.6, alpha(green, a))
            draw_disc(img, x, y - 1, 1.0, alpha(green_hi, a * 0.9))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "heal_sparkle")


def gen_death_poof() -> None:
    """Six frames: gray smoke puff growing and fading."""
    frames = 6
    sheet = blank_sheet(frames)
    smoke = (180, 180, 180, 255)
    smoke_dark = (100, 100, 100, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / (frames - 1)
        cx = FRAME / 2
        cy = FRAME / 2 + 8 - 6 * t
        r = 5 + 9 * t
        a = 1.0 - t * 0.85
        draw_disc(img, cx, cy, r, alpha(smoke_dark, a * 0.7))
        draw_disc(img, cx - 4, cy - 1, r * 0.7, alpha(smoke, a))
        draw_disc(img, cx + 4, cy + 1, r * 0.7, alpha(smoke, a))
        draw_disc(img, cx, cy - 3, r * 0.6, alpha(smoke, a))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "death_poof")


def gen_teleport_flash() -> None:
    """Six frames: bright concentric spiral collapsing inward then bursting."""
    frames = 6
    sheet = blank_sheet(frames)
    inner = (200, 180, 255, 255)
    outer = (130, 80, 220, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / (frames - 1)
        cx, cy = FRAME / 2, FRAME / 2
        # spiral arms
        arms = 3
        steps = 24
        for arm in range(arms):
            for s in range(steps):
                st = s / steps
                radius = 16 * (1.0 - 0.6 * t) * (1.0 - st * 0.9)
                angle = arm * 2 * math.pi / arms + st * math.pi * 1.8 + t * math.pi
                x = cx + math.cos(angle) * radius
                y = cy + math.sin(angle) * radius
                color = alpha(outer if st < 0.5 else inner, 1.0 - st * 0.5)
                draw_disc(img, x, y, 1.2, color)
        draw_disc(img, cx, cy, 2.5 + 3 * t, alpha(inner, 1.0 - t * 0.8))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "teleport_flash")


# ─── Sticky overlay effects (looping) ─────────────────────────────────────────


def gen_shield_bubble() -> None:
    """Four frames: a translucent blue bubble that pulses."""
    frames = 4
    sheet = blank_sheet(frames)
    blue = (90, 160, 255, 255)
    blue_hi = (200, 230, 255, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / frames
        pulse = 0.85 + 0.15 * math.sin(t * 2 * math.pi)
        r = 16 * pulse
        cx, cy = FRAME / 2, FRAME / 2
        draw_ring(img, cx, cy, r, 1.6, alpha(blue, 0.55))
        draw_ring(img, cx, cy, r - 2, 1.0, alpha(blue_hi, 0.35))
        # specular highlight
        draw_disc(img, cx - r * 0.5, cy - r * 0.55, 2, alpha(blue_hi, 0.7))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "shield_bubble")


def gen_bless_aura() -> None:
    """Six frames: a golden halo with rotating sparks above the head."""
    frames = 6
    sheet = blank_sheet(frames)
    gold = (252, 220, 96, 255)
    gold_hi = (255, 248, 200, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / frames
        cx, cy = FRAME / 2, FRAME / 2 - 16
        # halo ring
        draw_ring(img, cx, cy, 7, 1.2, alpha(gold, 0.85))
        # rotating sparks on the ring
        for k in range(4):
            ang = t * 2 * math.pi + k * math.pi / 2
            x = cx + math.cos(ang) * 7
            y = cy + math.sin(ang) * 7
            draw_disc(img, x, y, 1.5, alpha(gold_hi, 1.0))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "bless_aura")


def gen_sleep_zs() -> None:
    """Four frames: stacked 'Z' letters drifting up."""
    frames = 4
    sheet = blank_sheet(frames)
    blue = (140, 200, 255, 255)
    blue_dark = (90, 130, 220, 255)

    def stamp_z(img: Image.Image, x: int, y: int, color) -> None:
        # tiny 5x5 Z
        for k in range(5):
            img.putpixel((x + k, y), color)
            img.putpixel((x + k, y + 4), color)
        for k in range(5):
            xi = x + 4 - k
            yi = y + k
            if 0 <= xi < img.width and 0 <= yi < img.height:
                img.putpixel((xi, yi), color)

    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / frames
        rise = int(t * 12)
        for k, dy in enumerate([0, 7, 14]):
            yy = FRAME // 2 - 8 - dy - rise + k * 2
            xx = FRAME // 2 + 6 + k * 3
            color = alpha(blue if k % 2 == 0 else blue_dark, 1.0 - k * 0.2)
            stamp_z(img, xx, yy, color)
        paste_frame(sheet, img, i)
    save_sheet(sheet, "sleep_zs")


def gen_slow_drag() -> None:
    """Four frames: dripping downward arrows around the target's feet."""
    frames = 4
    sheet = blank_sheet(frames)
    purple = (160, 100, 200, 255)
    purple_dark = (90, 50, 140, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        t = i / frames
        for side_x in [FRAME // 2 - 10, FRAME // 2 + 10]:
            drip_y = int(FRAME / 2 + 4 + t * 8)
            draw_disc(img, side_x, drip_y, 1.8, alpha(purple, 0.85))
            draw_disc(img, side_x, drip_y - 3, 1.2, alpha(purple_dark, 0.7))
            # arrow tip
            img.putpixel((side_x, drip_y + 2), alpha(purple, 0.85))
            img.putpixel((side_x - 1, drip_y + 1), alpha(purple, 0.5))
            img.putpixel((side_x + 1, drip_y + 1), alpha(purple, 0.5))
        paste_frame(sheet, img, i)
    save_sheet(sheet, "slow_drag")


def gen_glimmer_aura() -> None:
    """Six frames: pale yellow sparkles drifting around the body."""
    frames = 6
    sheet = blank_sheet(frames)
    spark = (255, 240, 160, 255)
    spark_hi = (255, 255, 220, 255)
    seeds = [
        (-12, -6),
        (10, -10),
        (-8, 8),
        (12, 6),
        (-2, -14),
        (2, 14),
    ]
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        phase = i / frames
        for k, (sx, sy) in enumerate(seeds):
            bob = math.sin(phase * 2 * math.pi + k * 0.7) * 2
            x = FRAME / 2 + sx
            y = FRAME / 2 + sy + bob
            draw_disc(img, x, y, 1.5, alpha(spark, 0.8))
            img.putpixel((int(x), int(y)), spark_hi)
        paste_frame(sheet, img, i)
    save_sheet(sheet, "glimmer_aura")


def gen_haste_streaks() -> None:
    """Four frames: horizontal motion streaks trailing the target."""
    frames = 4
    sheet = blank_sheet(frames)
    streak = (220, 240, 255, 255)
    streak_dark = (140, 180, 220, 255)
    for i in range(frames):
        img = Image.new("RGBA", (FRAME, FRAME), BG)
        offset = i * 4
        for row_y, length in [(FRAME // 2 - 8, 14), (FRAME // 2, 18), (FRAME // 2 + 8, 12)]:
            base_x = 4 + (offset % 12)
            for k in range(length):
                xi = base_x + k
                if 0 <= xi < FRAME:
                    color = alpha(streak if k > length // 2 else streak_dark, 0.7)
                    img.putpixel((xi, row_y), color)
        paste_frame(sheet, img, i)
    save_sheet(sheet, "haste_streaks")


def main() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    gen_blood_splash()
    gen_cast_flash()
    gen_hit_flash()
    gen_heal_sparkle()
    gen_death_poof()
    gen_teleport_flash()
    gen_shield_bubble()
    gen_bless_aura()
    gen_sleep_zs()
    gen_slow_drag()
    gen_glimmer_aura()
    gen_haste_streaks()


if __name__ == "__main__":
    main()
