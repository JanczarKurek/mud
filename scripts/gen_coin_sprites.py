"""
Generates copper / silver / gold coin sprite tiers for the inventory and floor
display. Each coin type produces 12 PNGs (32x32 each):

  1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 20, 50

The numeric tiers map directly to the `stack_sprites` tiers declared in
`assets/overworld_objects/<coin>/metadata.yaml`. Visuals progress from a single
coin (tier 1) to a small handful (2-9), a small pile (10/20), and a heavy
mound (50).
"""

from __future__ import annotations

import os
import random
from PIL import Image

W, H = 32, 32
BG = (0, 0, 0, 0)
SHADOW = (0, 0, 0, 70)

PALETTES = {
    "copper": {
        "base":      (184, 115, 51, 255),
        "highlight": (228, 162, 92, 255),
        "edge_dark": (110,  66, 24, 255),
        "rim":       (148,  92, 38, 255),
    },
    "silver": {
        "base":      (200, 204, 212, 255),
        "highlight": (245, 248, 252, 255),
        "edge_dark": (118, 122, 130, 255),
        "rim":       (160, 164, 174, 255),
    },
    "gold": {
        "base":      (240, 200,  64, 255),
        "highlight": (255, 240, 150, 255),
        "edge_dark": (170, 120,  16, 255),
        "rim":       (210, 170,  40, 255),
    },
}


def new_img() -> Image.Image:
    return Image.new("RGBA", (W, H), BG)


def make_helpers(img: Image.Image):
    def px(x: int, y: int, c):
        if 0 <= x < W and 0 <= y < H:
            img.putpixel((x, y), c)

    def rect(x: int, y: int, w: int, h: int, c):
        for dy in range(h):
            for dx in range(w):
                px(x + dx, y + dy, c)

    return px, rect


# ---------- coin shapes ---------- #


def draw_coin(px, rect, cx: int, cy: int, palette: dict, scale: int = 1) -> None:
    """Draw a small disc-shaped coin centered at (cx, cy)."""
    base = palette["base"]
    hi = palette["highlight"]
    edge = palette["edge_dark"]
    rim = palette["rim"]

    if scale == 1:
        # 5x5 coin: rounded square approximating a circle
        rect(cx - 2, cy - 1, 5, 3, base)
        rect(cx - 1, cy - 2, 3, 5, base)
        # rim
        px(cx - 2, cy, rim)
        px(cx + 2, cy, rim)
        px(cx, cy - 2, rim)
        px(cx, cy + 2, rim)
        # corners darker
        px(cx - 2, cy - 1, edge)
        px(cx + 2, cy - 1, edge)
        px(cx - 2, cy + 1, edge)
        px(cx + 2, cy + 1, edge)
        # specular highlight
        px(cx - 1, cy - 1, hi)
    else:  # scale 2 (used for piles where each coin is small)
        rect(cx - 1, cy - 1, 3, 2, base)
        px(cx, cy - 1, hi)
        px(cx - 1, cy, edge)
        px(cx + 1, cy, edge)


def ground_shadow(rect, cx: int, cy: int, half_width: int) -> None:
    rect(cx - half_width, cy, half_width * 2 + 1, 1, SHADOW)
    rect(cx - max(half_width - 2, 1), cy + 1, max(half_width - 2, 1) * 2 + 1, 1, SHADOW)


# ---------- per-tier compositions ---------- #


def make_single(palette: dict) -> Image.Image:
    img = new_img()
    px, rect = make_helpers(img)
    ground_shadow(rect, 16, 22, 4)
    # bigger heroic coin in the center for tier 1 (8-px disc)
    base = palette["base"]
    hi = palette["highlight"]
    edge = palette["edge_dark"]
    rim = palette["rim"]
    cx, cy = 16, 17
    # disc
    rect(cx - 3, cy - 2, 7, 5, base)
    rect(cx - 2, cy - 3, 5, 7, base)
    # outer rim
    rect(cx - 3, cy - 1, 1, 3, rim)
    rect(cx + 3, cy - 1, 1, 3, rim)
    rect(cx - 1, cy - 3, 3, 1, rim)
    rect(cx - 1, cy + 3, 3, 1, rim)
    # corners
    px(cx - 3, cy - 2, edge); px(cx - 3, cy + 2, edge)
    px(cx + 3, cy - 2, edge); px(cx + 3, cy + 2, edge)
    px(cx - 2, cy - 3, edge); px(cx + 2, cy - 3, edge)
    px(cx - 2, cy + 3, edge); px(cx + 2, cy + 3, edge)
    # face highlight (top-left arc)
    rect(cx - 2, cy - 2, 2, 1, hi)
    px(cx - 2, cy - 1, hi)
    return img


def make_few(palette: dict, n: int) -> Image.Image:
    """A small handful: 2..9 individual coins arranged in a loose cluster."""
    img = new_img()
    px, rect = make_helpers(img)
    ground_shadow(rect, 16, 23, 6)

    # Deterministic positions per count so the same count always renders the
    # same way. 9 slots; we use the first `n`.
    positions = [
        (16, 16),  # center
        (12, 18),
        (20, 18),
        (14, 14),
        (18, 14),
        (10, 21),
        (22, 21),
        (16, 12),
        (16, 21),
    ]
    for cx, cy in positions[:n]:
        draw_coin(px, rect, cx, cy, palette, scale=1)
    return img


def make_small_pile(palette: dict) -> Image.Image:
    """Tier 10: a small mound — 2 rows of coins on top of a base layer."""
    img = new_img()
    px, rect = make_helpers(img)
    ground_shadow(rect, 16, 24, 9)
    base = palette["base"]
    hi = palette["highlight"]
    edge = palette["edge_dark"]
    rim = palette["rim"]

    # base row of coins
    for cx in (8, 12, 16, 20, 24):
        rect(cx - 1, 21, 3, 2, base)
        px(cx, 21, hi)
        px(cx - 1, 22, edge)
        px(cx + 1, 22, edge)
    # second row, offset
    for cx in (10, 14, 18, 22):
        rect(cx - 1, 18, 3, 2, base)
        px(cx, 18, hi)
        px(cx - 1, 19, edge)
        px(cx + 1, 19, edge)
    # third row, fewer
    for cx in (13, 17):
        rect(cx - 1, 15, 3, 2, base)
        px(cx, 15, hi)
    # crown coin
    rect(15, 12, 3, 2, base)
    px(16, 12, hi)
    # rim accents
    rect(7, 23, 19, 1, rim)
    return img


def make_medium_pile(palette: dict) -> Image.Image:
    """Tier 20: a wider, taller mound."""
    img = new_img()
    px, rect = make_helpers(img)
    ground_shadow(rect, 16, 26, 11)
    base = palette["base"]
    hi = palette["highlight"]
    edge = palette["edge_dark"]
    rim = palette["rim"]

    # base
    rect(5, 22, 22, 3, base)
    rect(5, 22, 22, 1, hi)
    rect(5, 24, 22, 1, edge)
    # second tier
    rect(7, 19, 18, 3, base)
    rect(7, 19, 18, 1, hi)
    rect(7, 21, 18, 1, edge)
    # third tier
    rect(10, 16, 12, 3, base)
    rect(10, 16, 12, 1, hi)
    rect(10, 18, 12, 1, edge)
    # peak
    rect(13, 13, 6, 3, base)
    rect(13, 13, 6, 1, hi)
    rect(13, 15, 6, 1, edge)

    # individual coin striations on each layer
    for y in (23, 20, 17, 14):
        for x in range(6, 26, 2):
            if 5 <= x <= 26:
                px(x, y, rim)
    return img


def make_large_pile(palette: dict) -> Image.Image:
    """Tier 50: a heavy mound with spilling coins."""
    img = new_img()
    px, rect = make_helpers(img)
    ground_shadow(rect, 16, 28, 13)
    base = palette["base"]
    hi = palette["highlight"]
    edge = palette["edge_dark"]
    rim = palette["rim"]

    # base spread
    rect(2, 23, 28, 4, base)
    rect(2, 23, 28, 1, hi)
    rect(2, 26, 28, 1, edge)
    # second tier
    rect(5, 19, 22, 4, base)
    rect(5, 19, 22, 1, hi)
    rect(5, 22, 22, 1, edge)
    # third
    rect(8, 15, 16, 4, base)
    rect(8, 15, 16, 1, hi)
    rect(8, 18, 16, 1, edge)
    # peak
    rect(12, 11, 8, 4, base)
    rect(12, 11, 8, 1, hi)
    rect(12, 14, 8, 1, edge)
    # very tip
    rect(14, 8, 4, 3, base)
    rect(14, 8, 4, 1, hi)

    # spilling coins on the ground edges
    draw_coin(px, rect, 1, 27, palette, scale=2)
    draw_coin(px, rect, 30, 27, palette, scale=2)
    draw_coin(px, rect, 4, 28, palette, scale=2)
    draw_coin(px, rect, 28, 28, palette, scale=2)

    # striations
    for y in (24, 20, 16, 12):
        for x in range(3, 29, 2):
            px(x, y, rim)
    return img


# ---------- batch driver ---------- #


TIERS_FEW = [1, 2, 3, 4, 5, 6, 7, 8, 9]


def render_tier(palette: dict, tier: int) -> Image.Image:
    if tier == 1:
        return make_single(palette)
    if tier in TIERS_FEW:
        return make_few(palette, tier)
    if tier == 10:
        return make_small_pile(palette)
    if tier == 20:
        return make_medium_pile(palette)
    if tier == 50:
        return make_large_pile(palette)
    raise ValueError(f"unsupported tier {tier}")


def main() -> None:
    out_root = "assets/overworld_objects"
    coin_types = [
        ("copper_coin", PALETTES["copper"]),
        ("silver_coin", PALETTES["silver"]),
        ("gold_coin",   PALETTES["gold"]),
    ]
    tiers = TIERS_FEW + [10, 20, 50]

    for type_id, palette in coin_types:
        out_dir = os.path.join(out_root, type_id)
        os.makedirs(out_dir, exist_ok=True)
        for tier in tiers:
            img = render_tier(palette, tier)
            path = os.path.join(out_dir, f"{tier}.png")
            img.save(path)
            print(f"Saved {path}")


if __name__ == "__main__":
    main()
