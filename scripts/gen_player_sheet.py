"""
Generates assets/overworld_objects/player/sheet.png plus three recolor-layer
sheets under assets/overworld_objects/player/layers/.

Sheet layout: 4 columns × 2 rows, each frame 32×48 px
  Row 0: idle (4 frames, breathing + hair sway + blink)
  Row 1: walk (4 frames, stride cycle)

Outputs:
  sheet.png            full-color base (skin + accessories + tunic + pants + hair)
  layers/hair.png      hair-region pixels in white-tone palette (tintable)
  layers/torso.png     tunic-region pixels in white-tone palette (tintable)
  layers/trousers.png  pants-region pixels in white-tone palette (tintable)

The layer PNGs share the base sheet's frame grid exactly — they are tinted by
`Sprite::color` (multiplicative) at runtime, with the chosen per-character RGB
defining the visible hue. The base sheet still carries the original hair/tunic/
pants colors so a default unmodified character looks complete even before any
recolor layers spawn, and so the static `sprite_large.png` fallback (used for
inventory icons etc.) keeps matching art.
"""

from PIL import Image, ImageDraw
import os

FRAME_W = 32
FRAME_H = 48
COLS    = 4
ROWS    = 2
OUT_BASE = "assets/overworld_objects/player/sheet.png"
OUT_LAYER_DIR = "assets/overworld_objects/player/layers"

# ── Palette ────────────────────────────────────────────────────────────────────
BG          = (0,   0,   0,   0)    # transparent

SKIN        = (220, 170, 120, 255)
SKIN_DARK   = (180, 130,  85, 255)
SKIN_HI     = (240, 195, 150, 255)

HAIR        = (220, 180,  35, 255)
HAIR_DARK   = (170, 130,  20, 255)
HAIR_HI     = (250, 215,  80, 255)

EYE_WHITE   = (240, 240, 240, 255)
EYE_IRIS    = ( 60, 110, 200, 255)
EYE_PUPIL   = ( 20,  20,  30, 255)

TUNIC       = (145,  55, 165, 255)  # purple
TUNIC_HI    = (175,  90, 200, 255)
TUNIC_DARK  = (100,  30, 120, 255)

BELT        = (130,  85,  25, 255)
BELT_BUCKLE = (210, 175,  50, 255)

PANTS       = ( 55,  70, 105, 255)  # dark blue-grey
PANTS_DARK  = ( 35,  48,  75, 255)

BOOT        = ( 72,  44,  18, 255)
BOOT_HI     = ( 95,  62,  28, 255)

CAPE        = (110,  30,  30, 255)  # dark red cape
CAPE_HI     = (145,  50,  50, 255)

SATCHEL     = (140,  95,  40, 255)  # small bag on hip

# White-tone palette used by tintable layers. Pixel × `Sprite::color` defines
# the final visible color; brightness levels here roughly match the original
# palette's tonal ratios so a "default" hue stays close to the original art.
LAYER_BRIGHT = (255, 255, 255, 255)  # 100% — highlight rim
LAYER_BASE   = (220, 220, 220, 255)  # ~86% — primary body color
LAYER_SHADE  = (160, 160, 160, 255)  # ~63% — shadow / dark edges


def _make_painter(img, dx_default=0, dy_default=0):
    """Return (px, rect) helpers bound to `img`. Mirrors the inline helpers
    from the original `make_frame` but reusable across layer images."""
    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)

    def rect(x, y, w, h, c, dy=0, dx=0):
        for ry in range(h):
            for rx in range(w):
                px(x + rx + dx, y + ry + dy, c)

    return px, rect


# ── Region painters ────────────────────────────────────────────────────────────
# Each painter writes one region into `img` using the supplied color set. The
# base sheet calls every painter with the original palette; layer sheets call
# only one painter with the white-tone palette so just that region is opaque.

def paint_pants(img, *, body_dy, c_base, c_dark):
    _, rect = _make_painter(img)
    bd = body_dy
    rect(10, 29+bd, 5, 10, c_base, 0)   # left leg
    rect(17, 29+bd, 5, 10, c_base, 0)   # right leg
    rect(13, 29+bd, 6,  5, c_base, 0)   # crotch
    rect(10, 29+bd, 1, 10, c_dark, 0)   # left seam
    rect(21, 29+bd, 1, 10, c_dark, 0)   # right seam


def paint_tunic(img, *, body_dy, l_arm_dy, r_arm_dy,
                c_base, c_hi, c_dark):
    _, rect = _make_painter(img)
    bd = body_dy
    # Main body
    rect( 9, 15+bd, 14, 13, c_base, 0)
    rect( 9, 15+bd,  1, 13, c_dark, 0)   # left edge
    rect(22, 15+bd,  1, 13, c_dark, 0)   # right edge
    rect( 9, 15+bd, 14,  1, c_hi,   0)   # collar highlight
    # Chest detail line
    for y in range(17+bd, 26+bd):
        img.putpixel((15, y), c_dark) if 0 <= 15 < FRAME_W and 0 <= y < FRAME_H else None
    # Left arm sleeve (top portion only — the forearm/wrist is skin)
    lad = l_arm_dy
    rect( 6, 17+bd+lad, 4, 8, c_base, 0)
    rect( 6, 17+bd+lad, 1, 8, c_dark, 0)   # sleeve edge
    # Right arm sleeve
    rad = r_arm_dy
    rect(22, 17+bd+rad, 4, 8, c_base, 0)
    rect(25, 17+bd+rad, 1, 8, c_dark, 0)


def paint_hair(img, *, body_dy, hair_dx, c_base, c_hi, c_dark):
    _, rect = _make_painter(img)
    bd = body_dy
    hx, hy = 9, 2+bd
    hdx = hair_dx
    # Top + sides
    rect(hx-1, hy-2,  16,  4, c_base, hdx)  # top hair
    rect(hx-1, hy-2,  16,  1, c_hi,   hdx)  # highlight
    rect(hx-1, hy+2,   2,  6, c_base, hdx)  # left sideburn
    rect(hx+13,hy+2,   2,  6, c_base, hdx)  # right sideburn
    # Tuft at top
    def px(x, y, c):
        if 0 <= x < FRAME_W and 0 <= y < FRAME_H:
            img.putpixel((x, y), c)
    px(hx+6+hdx, hy-3, c_base)
    px(hx+7+hdx, hy-3, c_hi)
    px(hx+8+hdx, hy-3, c_base)
    # Eyebrows (color-matched to hair)
    rect(hx+2, hy+3, 4, 1, c_dark, 0)
    rect(hx+9, hy+3, 4, 1, c_dark, 0)


def paint_skin_and_accessories(img, *, body_dy, l_foot_dy, r_foot_dy,
                                l_arm_dy, r_arm_dy, blink):
    """Everything that is NOT recolored: skin, face features, boots, belt,
    cape, satchel. Renders into `img` using the fixed original palette."""
    px, rect = _make_painter(img)
    bd = body_dy

    # ── Boots ─────────────────────────────────────────────────────────────────
    lby = 38 + l_foot_dy
    rect(10, lby,     5, 7, BOOT,    0)
    rect(10, lby,     5, 1, BOOT_HI, 0)
    rect( 9, lby+1,   1, 5, BOOT,    0)

    rby = 38 + r_foot_dy
    rect(17, rby,     5, 7, BOOT,    0)
    rect(17, rby,     5, 1, BOOT_HI, 0)
    rect(22, rby+1,   1, 5, BOOT,    0)

    # ── Belt ──────────────────────────────────────────────────────────────────
    rect( 9, 27+bd, 14, 3, BELT,        0)
    rect(14, 27+bd,  3, 3, BELT_BUCKLE, 0)  # buckle

    # ── Cape (behind body, left side peek) ────────────────────────────────────
    rect(7, 16+bd, 3, 13, CAPE,    0)
    rect(7, 16+bd, 1, 13, CAPE_HI, 0)

    # ── Satchel (right hip) ───────────────────────────────────────────────────
    rect(22, 24+bd, 4, 5, SATCHEL, 0)
    rect(22, 24+bd, 4, 1, BELT,    0)   # strap top

    # ── Left arm forearm + wrist (sleeve part lives in tunic layer) ───────────
    lad = l_arm_dy
    rect( 6, 25+bd+lad, 4, 4, SKIN,      0)
    rect( 6, 29+bd+lad, 4, 2, SKIN_DARK, 0)

    # ── Right arm forearm + wrist ─────────────────────────────────────────────
    rad = r_arm_dy
    rect(22, 25+bd+rad, 4, 4, SKIN,      0)
    rect(22, 29+bd+rad, 4, 2, SKIN_DARK, 0)

    # ── Neck ──────────────────────────────────────────────────────────────────
    rect(14, 12+bd, 4, 4, SKIN, 0)

    # ── Head ──────────────────────────────────────────────────────────────────
    hx, hy = 9, 2+bd
    rect(hx,   hy,   14, 12, SKIN,      0)
    rect(hx,   hy,    1, 12, SKIN_DARK, 0)
    rect(hx+13,hy,    1, 12, SKIN_DARK, 0)
    rect(hx,   hy,   14,  1, SKIN_HI,   0)

    # ── Ears ──────────────────────────────────────────────────────────────────
    px(hx-1, hy+4, SKIN)
    px(hx-1, hy+5, SKIN)
    px(hx+14,hy+4, SKIN)
    px(hx+14,hy+5, SKIN)

    # ── Eyes ──────────────────────────────────────────────────────────────────
    if blink:
        rect(hx+2, hy+5, 3, 1, SKIN_DARK, 0)
        rect(hx+9, hy+5, 3, 1, SKIN_DARK, 0)
    else:
        rect(hx+2, hy+4, 4, 4, EYE_WHITE, 0)
        rect(hx+9, hy+4, 4, 4, EYE_WHITE, 0)
        rect(hx+3, hy+5, 2, 2, EYE_IRIS,  0)
        rect(hx+10,hy+5, 2, 2, EYE_IRIS,  0)
        px(hx+4,  hy+6, EYE_PUPIL)
        px(hx+11, hy+6, EYE_PUPIL)

    # ── Nose & mouth ──────────────────────────────────────────────────────────
    px(hx+6, hy+8, SKIN_DARK)
    px(hx+7, hy+8, SKIN_DARK)
    rect(hx+4, hy+10, 6, 1, SKIN_DARK, 0)   # mouth line
    px(hx+5,  hy+10, SKIN_HI)               # smile
    px(hx+9,  hy+10, SKIN_HI)


# ── Frame builders ─────────────────────────────────────────────────────────────

def make_base_frame(**kwargs):
    """Full-color frame for sheet.png — all four regions composited."""
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)

    # Order matters: cape behind, then pants (legs), then tunic on top of belt
    # area, then hair on top of head. The original `make_frame` had this exact
    # sequence; preserving it keeps the composite identical.
    paint_skin_and_accessories(
        img,
        body_dy=kwargs["body_dy"],
        l_foot_dy=kwargs["l_foot_dy"],
        r_foot_dy=kwargs["r_foot_dy"],
        l_arm_dy=kwargs["l_arm_dy"],
        r_arm_dy=kwargs["r_arm_dy"],
        blink=kwargs["blink"],
    )
    paint_pants(
        img,
        body_dy=kwargs["body_dy"],
        c_base=PANTS, c_dark=PANTS_DARK,
    )
    paint_tunic(
        img,
        body_dy=kwargs["body_dy"],
        l_arm_dy=kwargs["l_arm_dy"],
        r_arm_dy=kwargs["r_arm_dy"],
        c_base=TUNIC, c_hi=TUNIC_HI, c_dark=TUNIC_DARK,
    )
    paint_hair(
        img,
        body_dy=kwargs["body_dy"],
        hair_dx=kwargs["hair_dx"],
        c_base=HAIR, c_hi=HAIR_HI, c_dark=HAIR_DARK,
    )
    return img


def make_hair_layer_frame(**kwargs):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    paint_hair(
        img,
        body_dy=kwargs["body_dy"],
        hair_dx=kwargs["hair_dx"],
        c_base=LAYER_BASE, c_hi=LAYER_BRIGHT, c_dark=LAYER_SHADE,
    )
    return img


def make_torso_layer_frame(**kwargs):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    paint_tunic(
        img,
        body_dy=kwargs["body_dy"],
        l_arm_dy=kwargs["l_arm_dy"],
        r_arm_dy=kwargs["r_arm_dy"],
        c_base=LAYER_BASE, c_hi=LAYER_BRIGHT, c_dark=LAYER_SHADE,
    )
    return img


def make_trousers_layer_frame(**kwargs):
    img = Image.new("RGBA", (FRAME_W, FRAME_H), BG)
    paint_pants(
        img,
        body_dy=kwargs["body_dy"],
        c_base=LAYER_BASE, c_dark=LAYER_SHADE,
    )
    return img


# ── Frame schedule ─────────────────────────────────────────────────────────────
# Each entry is the kwargs dict for one frame, used identically across the base
# sheet and all three layer sheets so the layers stay frame-locked.

FRAME_DEFAULTS = dict(
    body_dy=0, l_foot_dy=0, r_foot_dy=0, l_arm_dy=0, r_arm_dy=0,
    hair_dx=0, blink=False,
)


def _frame(**overrides):
    out = FRAME_DEFAULTS.copy()
    out.update(overrides)
    return out


# Idle: gentle breathing, hair sway, blink on frame 3
IDLE_FRAMES = [
    _frame(body_dy=0,  hair_dx=0,  blink=False),
    _frame(body_dy=-1, hair_dx=0,  blink=False),
    _frame(body_dy=-1, hair_dx=1,  blink=False),
    _frame(body_dy=0,  hair_dx=0,  blink=True),
]

# Walk: 4-frame stride cycle with arm swing
WALK_FRAMES = [
    _frame(body_dy=-1, l_foot_dy=-3, r_foot_dy=2,  l_arm_dy=3,  r_arm_dy=-3),
    _frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  l_arm_dy=0,  r_arm_dy=0),
    _frame(body_dy=-1, l_foot_dy=2,  r_foot_dy=-3, l_arm_dy=-3, r_arm_dy=3),
    _frame(body_dy=1,  l_foot_dy=0,  r_foot_dy=0,  l_arm_dy=0,  r_arm_dy=0),
]


def assemble(make_fn):
    """Paste idle + walk frames into a 4×2 atlas using `make_fn` per frame."""
    sheet = Image.new("RGBA", (FRAME_W * COLS, FRAME_H * ROWS), BG)
    for col, kwargs in enumerate(IDLE_FRAMES):
        sheet.paste(make_fn(**kwargs), (col * FRAME_W, 0))
    for col, kwargs in enumerate(WALK_FRAMES):
        sheet.paste(make_fn(**kwargs), (col * FRAME_W, FRAME_H))
    return sheet


# ── Output ─────────────────────────────────────────────────────────────────────

def save(image, path):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    image.save(path)
    print(f"Saved {path}  ({image.width}×{image.height})")


save(assemble(make_base_frame),           OUT_BASE)
save(assemble(make_hair_layer_frame),     os.path.join(OUT_LAYER_DIR, "hair.png"))
save(assemble(make_torso_layer_frame),    os.path.join(OUT_LAYER_DIR, "torso.png"))
save(assemble(make_trousers_layer_frame), os.path.join(OUT_LAYER_DIR, "trousers.png"))
