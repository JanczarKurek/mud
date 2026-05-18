"""
Generate 12 damage-type hit VFX sprite sheets.

Each sheet:
  - 48x48 px per frame
  - 6 frames in a single row (sheet 288x48)
  - transparent background
  - 18 fps non-looping (duration 0.36s, configured in metadata.yaml)

Output: assets/vfx/<type>_hit/sheet.png for every DamageType variant.

The progression across the 6 frames is roughly:
  f0 birth -> f1-3 peak -> f4-5 fade-out

Each damage type has a distinct silhouette, not just a palette swap, so a
glance during combat tells you what kind of damage just landed.
"""

from PIL import Image
import math
import os

FRAME = 48
FRAMES = 6
SHEET_W = FRAME * FRAMES
SHEET_H = FRAME
CX = FRAME // 2  # 24
CY = FRAME // 2  # 24
BG = (0, 0, 0, 0)
OUT_ROOT = "assets/vfx"


# ------------------------------------------------------------------------
# Primitive helpers
# ------------------------------------------------------------------------

def new_frame():
    return Image.new("RGBA", (FRAME, FRAME), BG)


def put(img, x, y, color):
    if 0 <= x < FRAME and 0 <= y < FRAME:
        img.putpixel((x, y), color)


def fill_rect(img, x, y, w, h, color):
    for dy in range(h):
        for dx in range(w):
            put(img, x + dx, y + dy, color)


def stroke_circle(img, cx, cy, r, color, thickness=1):
    """Filled annulus from r-thickness+1 to r."""
    r2_outer = r * r
    r2_inner = max(0, r - thickness) ** 2
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            d2 = dx * dx + dy * dy
            if r2_inner < d2 <= r2_outer:
                put(img, cx + dx, cy + dy, color)


def filled_circle(img, cx, cy, r, color):
    r2 = r * r
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            if dx * dx + dy * dy <= r2:
                put(img, cx + dx, cy + dy, color)


def line(img, x0, y0, x1, y1, color):
    """Bresenham."""
    dx = abs(x1 - x0)
    dy = -abs(y1 - y0)
    sx = 1 if x0 < x1 else -1
    sy = 1 if y0 < y1 else -1
    err = dx + dy
    while True:
        put(img, x0, y0, color)
        if x0 == x1 and y0 == y1:
            break
        e2 = 2 * err
        if e2 >= dy:
            err += dy
            x0 += sx
        if e2 <= dx:
            err += dx
            y0 += sy


def thick_line(img, x0, y0, x1, y1, color, thickness=1):
    line(img, x0, y0, x1, y1, color)
    for t in range(1, thickness):
        line(img, x0 + t, y0, x1 + t, y1, color)
        line(img, x0, y0 + t, x1, y1 + t, color)


def sparkle(img, cx, cy, color, size=2):
    """A small +-shaped sparkle."""
    for d in range(-size, size + 1):
        put(img, cx + d, cy, color)
        put(img, cx, cy + d, color)


def write_sheet(name, frames):
    sheet = Image.new("RGBA", (SHEET_W, SHEET_H), BG)
    for i, f in enumerate(frames):
        sheet.paste(f, (i * FRAME, 0))
    out_dir = os.path.join(OUT_ROOT, name)
    os.makedirs(out_dir, exist_ok=True)
    sheet.save(os.path.join(out_dir, "sheet.png"))
    print(f"wrote {out_dir}/sheet.png")


# ------------------------------------------------------------------------
# Damage-type painters. Each returns a list of 6 frames.
# ------------------------------------------------------------------------

def blunt_frames():
    # Grey expanding shockwave rings.
    pale = (220, 220, 220, 255)
    mid = (170, 170, 170, 220)
    dark = (110, 110, 110, 180)
    frames = []
    radii = [3, 7, 12, 17, 21, 23]
    alphas = [pale, pale, mid, mid, dark, (90, 90, 90, 120)]
    for r, c in zip(radii, alphas):
        f = new_frame()
        stroke_circle(f, CX, CY, r, c, thickness=2)
        # smaller inner ring trailing behind
        if r > 6:
            stroke_circle(f, CX, CY, r - 5, (200, 200, 200, 120), thickness=1)
        frames.append(f)
    return frames


def cut_frames():
    # Diagonal white slash + red bleed.
    slash = (255, 255, 255, 255)
    edge = (220, 220, 220, 200)
    blood = (200, 30, 30, 255)
    blood_dark = (140, 10, 10, 230)
    frames = []
    # Slash extends from top-right to bottom-left across the frame.
    extents = [(20, 28, 4), (16, 32, 6), (12, 36, 8), (10, 38, 6), (12, 36, 4), (16, 32, 2)]
    for i, (s, e, thick) in enumerate(extents):
        f = new_frame()
        # diagonal: (FRAME-s, s) -> (FRAME-e, e)
        x0, y0 = FRAME - s, s
        x1, y1 = FRAME - e, e
        for t in range(-(thick // 2), thick // 2 + 1):
            color = slash if t == 0 else edge
            line(f, x0 + t, y0, x1 + t, y1, color)
        # blood droplets growing then falling
        if i >= 1:
            put(f, CX - 4, CY + 2, blood)
            put(f, CX - 5, CY + 3, blood_dark)
            put(f, CX + 6, CY - 4, blood)
        if i >= 2:
            put(f, CX - 8, CY + 5, blood)
            put(f, CX + 10, CY - 6, blood_dark)
        if i >= 3:
            put(f, CX - 10, CY + 8, blood_dark)
            put(f, CX + 12, CY - 9, blood_dark)
        frames.append(f)
    return frames


def pierce_frames():
    # Red 4-pointed star/puncture with radial droplets.
    star = (220, 30, 30, 255)
    star_hi = (255, 120, 120, 255)
    droplet = (170, 20, 20, 230)
    frames = []
    lengths = [2, 5, 9, 12, 10, 6]
    for i, L in enumerate(lengths):
        f = new_frame()
        # 4-axis star
        for d in range(-L, L + 1):
            put(f, CX + d, CY, star)
            put(f, CX, CY + d, star)
        # center highlight
        filled_circle(f, CX, CY, 2, star_hi)
        # diagonal droplets at later frames
        if i >= 2:
            r = i * 3
            for ang in (45, 135, 225, 315):
                rad = math.radians(ang)
                dx = int(round(math.cos(rad) * r))
                dy = int(round(math.sin(rad) * r))
                put(f, CX + dx, CY + dy, droplet)
                put(f, CX + dx + 1, CY + dy, droplet)
        frames.append(f)
    return frames


def fire_frames():
    # Orange/red flame burst with rising sparks.
    inner = (255, 240, 130, 255)
    mid = (255, 160, 30, 255)
    outer = (220, 60, 20, 230)
    smoke = (60, 50, 50, 140)
    spark = (255, 220, 90, 255)
    frames = []
    radii = [(2, 4, 6), (4, 7, 10), (5, 10, 14), (5, 11, 16), (3, 9, 14), (2, 6, 10)]
    for i, (a, b, c) in enumerate(radii):
        f = new_frame()
        filled_circle(f, CX, CY, c, outer)
        filled_circle(f, CX, CY, b, mid)
        filled_circle(f, CX, CY, a, inner)
        # rising flame tongues — taller in mid-frames
        tongue_h = [2, 4, 6, 7, 5, 3][i]
        for tx, base in ((CX - 6, CY - c + 2), (CX + 6, CY - c + 4), (CX, CY - c)):
            for dy in range(tongue_h):
                col = mid if dy < tongue_h // 2 else outer
                put(f, tx, base - dy, col)
        # sparks at peak frames
        if 2 <= i <= 4:
            for sx, sy in ((CX - 10, CY - 12), (CX + 11, CY - 9), (CX - 4, CY - 16), (CX + 7, CY - 14)):
                put(f, sx, sy - (i - 2), spark)
        # late smoke
        if i >= 4:
            put(f, CX - 2, CY - c - 2, smoke)
            put(f, CX + 3, CY - c - 3, smoke)
        frames.append(f)
    return frames


def frost_frames():
    # Pale-blue crystalline shards bursting outward.
    pale = (220, 240, 255, 255)
    mid = (130, 200, 240, 255)
    deep = (60, 120, 200, 220)
    glint = (255, 255, 255, 255)
    frames = []
    extents = [3, 7, 12, 16, 18, 14]
    for i, L in enumerate(extents):
        f = new_frame()
        # 8 shards radiating
        for k in range(8):
            ang = math.radians(k * 45 + (i * 5))
            dx = math.cos(ang)
            dy = math.sin(ang)
            for step in range(L):
                x = int(round(CX + dx * step))
                y = int(round(CY + dy * step))
                col = pale if step < 2 else (mid if step < L - 2 else deep)
                put(f, x, y, col)
                # crystalline thickness near the tip
                if step >= L - 3 and step > 1:
                    nx = int(round(CX + dx * step - dy))
                    ny = int(round(CY + dy * step + dx))
                    put(f, nx, ny, deep)
        # glints in peak frames
        if 2 <= i <= 4:
            sparkle(f, CX - 8, CY - 8, glint, 1)
            sparkle(f, CX + 7, CY + 6, glint, 1)
        # center frost dot
        filled_circle(f, CX, CY, 2, pale)
        frames.append(f)
    return frames


def earth_frames():
    # Brown rock chunks + dust puff.
    rock = (110, 78, 50, 255)
    rock_hi = (160, 120, 80, 255)
    rock_dk = (70, 48, 30, 255)
    dust = (180, 160, 130, 150)
    dust_d = (140, 120, 95, 110)
    frames = []
    for i in range(FRAMES):
        f = new_frame()
        # dust expanding cloud
        r = [4, 8, 12, 15, 16, 14][i]
        filled_circle(f, CX, CY + 2, r, dust_d)
        filled_circle(f, CX, CY + 2, max(0, r - 3), dust)
        # chunks at 6 angles flying outward
        dist = [2, 6, 10, 14, 16, 18][i]
        for k in range(6):
            ang = math.radians(k * 60 + 15)
            dx = int(round(math.cos(ang) * dist))
            dy = int(round(math.sin(ang) * dist))
            # 3x2 chunk
            fill_rect(f, CX + dx - 1, CY + dy - 1, 3, 2, rock)
            put(f, CX + dx - 1, CY + dy - 1, rock_hi)
            put(f, CX + dx + 1, CY + dy, rock_dk)
        frames.append(f)
    return frames


def lightning_frames():
    # Yellow forked bolt + white afterimage.
    bright = (255, 255, 180, 255)
    yellow = (255, 230, 60, 255)
    glow = (255, 240, 120, 140)
    frames = []
    # The bolt zigzags from top to bottom of the frame; jaggedness varies per frame.
    bolts = [
        [(CX, 4), (CX - 3, 12), (CX + 2, 20), (CX - 2, 28), (CX + 1, 36), (CX - 1, 44)],
        [(CX + 1, 3), (CX - 4, 11), (CX + 3, 21), (CX - 3, 29), (CX + 2, 37), (CX, 45)],
        [(CX, 4), (CX + 4, 13), (CX - 4, 22), (CX + 4, 30), (CX - 3, 38), (CX + 1, 44)],
        [(CX - 1, 5), (CX - 5, 13), (CX + 3, 22), (CX - 4, 30), (CX + 4, 38), (CX, 44)],
        [(CX, 6), (CX - 2, 14), (CX + 2, 23), (CX - 1, 31), (CX + 1, 39), (CX, 45)],
        [(CX, 12), (CX, 22), (CX, 32), (CX, 40)],
    ]
    for i, path in enumerate(bolts):
        f = new_frame()
        # outer glow first (drawn under)
        for (a, b) in zip(path, path[1:]):
            thick_line(f, a[0] - 1, a[1], b[0] - 1, b[1], glow, thickness=2)
            thick_line(f, a[0] + 1, a[1], b[0] + 1, b[1], glow, thickness=2)
        # inner bright bolt
        for (a, b) in zip(path, path[1:]):
            line(f, a[0], a[1], b[0], b[1], bright)
            line(f, a[0] + 1, a[1], b[0] + 1, b[1], yellow)
        # short fork in peak frames
        if 1 <= i <= 3 and len(path) >= 4:
            mid = path[len(path) // 2]
            line(f, mid[0], mid[1], mid[0] + 5, mid[1] + 4, yellow)
            line(f, mid[0], mid[1], mid[0] - 5, mid[1] + 3, yellow)
        frames.append(f)
    return frames


def poison_frames():
    # Green bubble cloud rising and dispersing.
    pale = (180, 240, 130, 255)
    mid = (90, 180, 60, 255)
    dark = (40, 100, 30, 230)
    bubble_hi = (220, 255, 200, 255)
    frames = []
    # Each frame has bubbles at scripted positions+sizes.
    scripts = [
        [(CX, CY, 3)],
        [(CX, CY, 4), (CX - 5, CY - 3, 2)],
        [(CX, CY - 1, 5), (CX - 6, CY - 4, 3), (CX + 5, CY - 2, 2)],
        [(CX - 1, CY - 3, 5), (CX - 7, CY - 7, 3), (CX + 6, CY - 5, 3), (CX + 2, CY + 4, 2)],
        [(CX - 2, CY - 6, 4), (CX - 8, CY - 10, 2), (CX + 7, CY - 9, 3), (CX + 1, CY + 2, 2)],
        [(CX - 3, CY - 10, 3), (CX + 8, CY - 12, 2)],
    ]
    for sc in scripts:
        f = new_frame()
        for (x, y, r) in sc:
            filled_circle(f, x, y, r, dark)
            filled_circle(f, x, y, max(0, r - 1), mid)
            filled_circle(f, x, y, max(0, r - 2), pale)
            put(f, x - r // 2, y - r // 2, bubble_hi)
        frames.append(f)
    return frames


def acid_frames():
    # Yellow-green dripping splash with corrosive halo.
    splash = (200, 230, 60, 255)
    splash_dk = (140, 170, 30, 255)
    drip = (160, 200, 40, 230)
    halo = (220, 255, 80, 90)
    frames = []
    for i in range(FRAMES):
        f = new_frame()
        # halo
        r = [4, 8, 12, 14, 13, 10][i]
        stroke_circle(f, CX, CY, r, halo, thickness=2)
        # central splash (irregular)
        sr = [3, 5, 7, 7, 6, 4][i]
        filled_circle(f, CX, CY, sr, splash_dk)
        filled_circle(f, CX, CY, max(0, sr - 2), splash)
        # gobs flying outward
        for k, ang in enumerate((10, 60, 130, 200, 270, 330)):
            d = [2, 5, 9, 12, 14, 15][i] + (k % 2)
            rad = math.radians(ang)
            x = int(round(CX + math.cos(rad) * d))
            y = int(round(CY + math.sin(rad) * d))
            put(f, x, y, splash)
            put(f, x + 1, y, splash_dk)
        # drips appear later
        if i >= 3:
            for dx in (-6, -1, 4, 9):
                put(f, CX + dx, CY + 8, drip)
                put(f, CX + dx, CY + 9, drip)
                put(f, CX + dx, CY + 10 + (i - 3), splash_dk)
        frames.append(f)
    return frames


def death_frames():
    # Dark purple void wisp with skull silhouette.
    void = (60, 20, 80, 200)
    void_d = (30, 5, 50, 230)
    wisp = (140, 80, 180, 220)
    bone = (220, 215, 200, 255)
    bone_d = (140, 130, 110, 255)
    frames = []
    for i in range(FRAMES):
        f = new_frame()
        # swirling void cloud
        r = [3, 7, 11, 14, 13, 9][i]
        filled_circle(f, CX, CY, r, void_d)
        filled_circle(f, CX, CY, max(0, r - 2), void)
        # wispy curl
        ang_off = i * 30
        for step in range(8):
            ang = math.radians(ang_off + step * 25)
            rr = r - step // 2
            x = int(round(CX + math.cos(ang) * rr))
            y = int(round(CY + math.sin(ang) * rr))
            put(f, x, y, wisp)
        # skull at peak frames
        if 2 <= i <= 4:
            # cranium
            fill_rect(f, CX - 3, CY - 4, 7, 5, bone)
            put(f, CX - 3, CY - 4, bone_d)
            put(f, CX + 3, CY - 4, bone_d)
            # eye sockets
            put(f, CX - 2, CY - 2, void_d)
            put(f, CX + 2, CY - 2, void_d)
            # jaw
            fill_rect(f, CX - 2, CY + 1, 5, 2, bone)
            put(f, CX - 1, CY + 2, void_d)
            put(f, CX + 1, CY + 2, void_d)
        frames.append(f)
    return frames


def holy_frames():
    # White-gold radiant cross-burst with sparkles.
    bright = (255, 255, 220, 255)
    gold = (255, 220, 110, 255)
    halo = (255, 240, 160, 130)
    sparkle_c = (255, 255, 255, 255)
    frames = []
    lengths = [2, 6, 12, 16, 14, 8]
    for i, L in enumerate(lengths):
        f = new_frame()
        # soft halo
        stroke_circle(f, CX, CY, max(2, L - 1), halo, thickness=2)
        # 4 cardinal beams + 4 diagonals
        for k in range(8):
            ang = math.radians(k * 45)
            dx, dy = math.cos(ang), math.sin(ang)
            for step in range(L):
                x = int(round(CX + dx * step))
                y = int(round(CY + dy * step))
                col = bright if step < L - 2 else gold
                put(f, x, y, col)
                # thicken cardinal beams
                if k % 2 == 0 and step > 0:
                    nx = int(round(CX + dx * step + (-dy)))
                    ny = int(round(CY + dy * step + dx))
                    put(f, nx, ny, gold)
        # central glow
        filled_circle(f, CX, CY, 3, bright)
        # sparkles in peak frames
        if 2 <= i <= 4:
            sparkle(f, CX - 11, CY - 4, sparkle_c, 2)
            sparkle(f, CX + 9, CY + 7, sparkle_c, 2)
            sparkle(f, CX - 6, CY + 11, sparkle_c, 1)
        frames.append(f)
    return frames


def arcane_frames():
    # Violet runic ring with twinkling stars.
    ring = (180, 100, 230, 255)
    ring_dk = (120, 50, 180, 255)
    glow = (210, 160, 255, 140)
    star = (255, 255, 255, 255)
    frames = []
    radii = [5, 9, 13, 16, 14, 10]
    for i, r in enumerate(radii):
        f = new_frame()
        # outer glow halo
        stroke_circle(f, CX, CY, r + 1, glow, thickness=2)
        # main ring
        stroke_circle(f, CX, CY, r, ring, thickness=1)
        stroke_circle(f, CX, CY, max(1, r - 2), ring_dk, thickness=1)
        # rune tick-marks on the ring (every 30°)
        for k in range(12):
            ang = math.radians(k * 30 + i * 10)
            x_inner = int(round(CX + math.cos(ang) * (r - 2)))
            y_inner = int(round(CY + math.sin(ang) * (r - 2)))
            x_outer = int(round(CX + math.cos(ang) * (r + 1)))
            y_outer = int(round(CY + math.sin(ang) * (r + 1)))
            line(f, x_inner, y_inner, x_outer, y_outer, ring)
        # twinkling stars at peak frames
        if 2 <= i <= 4:
            for ang in (30, 110, 200, 290):
                rad = math.radians(ang + i * 15)
                rr = r - 4
                sx = int(round(CX + math.cos(rad) * rr))
                sy = int(round(CY + math.sin(rad) * rr))
                sparkle(f, sx, sy, star, 1)
        frames.append(f)
    return frames


# ------------------------------------------------------------------------
# Main
# ------------------------------------------------------------------------

PAINTERS = {
    "blunt_hit": blunt_frames,
    "cut_hit": cut_frames,
    "pierce_hit": pierce_frames,
    "fire_hit": fire_frames,
    "frost_hit": frost_frames,
    "earth_hit": earth_frames,
    "lightning_hit": lightning_frames,
    "poison_hit": poison_frames,
    "acid_hit": acid_frames,
    "death_hit": death_frames,
    "holy_hit": holy_frames,
    "arcane_hit": arcane_frames,
}


def main():
    for name, painter in PAINTERS.items():
        frames = painter()
        assert len(frames) == FRAMES, f"{name} produced {len(frames)} frames"
        write_sheet(name, frames)


if __name__ == "__main__":
    main()
