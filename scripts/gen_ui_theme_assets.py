"""
Generates the UI theme assets used by movable windows and themed widgets.

Outputs to assets/ui/theme/:
  panel_frame.png   32x32, 8px 9-slice border, FULL-COLOR (rendered with
                    Color::WHITE tint by spawn_movable_window).
  title_bar.png     32x16, 4px 9-slice border, FULL-COLOR (same).
  button_frame.png  24x24, 4px 9-slice border, tint-neutral (near-white)
                    so the per-state palette tinting in
                    apply_themed_button_tint still drives the look.
  slot_frame.png    40x40, 2px 9-slice border, tint-neutral.
  divider.png       8x2, tint-neutral hairline.
  close_icon.png    14x14, full-color brass "X" on transparent background.
  close_button.png  18x18, circular brass medallion with the X embedded,
                    on transparent background. Used as a self-contained
                    close button (no rectangular frame underneath).
  dock_button.png   18x18, brass medallion with a "dock-into-sidebar"
                    arrow (left-pointing into a vertical bar). Used on
                    floating windows to re-dock them to the right side.
  undock_button.png 18x18, brass medallion with a "pop-out" arrow
                    (diagonal arrow exiting a frame). Used on docked
                    panels to float them.
  resize_corner.png 14x14, full-color brass corner ornament for the
                    bottom-right resize handle (matches the panel's
                    bottom-right corner styling with added grip lines).
  resize_grip.png   16x10, horizontal brass grip strip used by the docked
                    panels' bottom resize-edge handle. Tileable along
                    the panel's full width.

Re-run after changes:
    python3 scripts/gen_ui_theme_assets.py
"""

from PIL import Image
import math
import os

OUT_DIR = "assets/ui/theme"

# Full-color brass / wood palette. Baked directly into panel_frame /
# title_bar / close_icon (rendered without runtime tinting).
WOOD_DARK = (28, 24, 20, 255)
WOOD_GRAIN = (40, 32, 24, 255)
WOOD_LIGHT = (60, 44, 30, 255)
BRASS_HI = (240, 200, 100, 255)
BRASS_MID = (180, 140, 56, 255)
BRASS_LO = (110, 80, 32, 255)
BRASS_DARK = (60, 42, 16, 255)
OUTLINE = (10, 8, 6, 255)
CLEAR = (0, 0, 0, 0)

# Tint-neutral palette for button_frame / slot_frame / divider. Pixels are
# near-white so the runtime tint multiplies cleanly to the target color.
NEUTRAL_FILL = (235, 235, 235, 255)
NEUTRAL_HI = (255, 255, 255, 255)
NEUTRAL_LO = (175, 175, 175, 255)
NEUTRAL_EDGE = (115, 115, 115, 255)


# ---------- primitives -----------------------------------------------------


def new_img(w, h, bg=CLEAR):
    return Image.new("RGBA", (w, h), bg)


def px(img, x, y, c):
    w, h = img.size
    if 0 <= x < w and 0 <= y < h:
        img.putpixel((x, y), c)


def hline(img, x, y, n, c):
    for i in range(n):
        px(img, x + i, y, c)


def vline(img, x, y, n, c):
    for i in range(n):
        px(img, x, y + i, c)


def rect_fill(img, x, y, w, h, c):
    for j in range(h):
        for i in range(w):
            px(img, x + i, y + j, c)


def rect_outline(img, x, y, w, h, c):
    hline(img, x, y, w, c)
    hline(img, x, y + h - 1, w, c)
    vline(img, x, y, h, c)
    vline(img, x + w - 1, y, h, c)


# ---------- corner stamps --------------------------------------------------


def brass_corner_8():
    """8x8 ornate top-left corner: outline, brass plate with light bevel,
    inner shadow, and a centered rivet."""
    c = new_img(8, 8, OUTLINE)
    rect_fill(c, 1, 1, 6, 6, BRASS_MID)
    # Light bevel along the two outward-facing inner edges (top + left).
    hline(c, 1, 1, 6, BRASS_HI)
    vline(c, 1, 1, 6, BRASS_HI)
    # Shadow along the two inward-facing edges (right + bottom).
    hline(c, 1, 6, 6, BRASS_LO)
    vline(c, 6, 1, 6, BRASS_LO)
    # Diagonal glint inside the bevel.
    px(c, 2, 2, BRASS_HI)
    # Rivet (2x2 dark with single highlight).
    rect_fill(c, 4, 4, 2, 2, BRASS_DARK)
    px(c, 4, 4, BRASS_HI)
    return c


def brass_corner_4():
    """4x4 corner cap — dark outline + brass interior + single glint."""
    c = new_img(4, 4, OUTLINE)
    rect_fill(c, 1, 1, 2, 2, BRASS_MID)
    px(c, 1, 1, BRASS_HI)
    return c


def neutral_corner_4():
    """4x4 tint-neutral bevel corner."""
    c = new_img(4, 4, NEUTRAL_EDGE)
    rect_fill(c, 1, 1, 2, 2, NEUTRAL_FILL)
    px(c, 1, 1, NEUTRAL_HI)
    return c


def neutral_corner_2():
    """2x2 tint-neutral inset corner."""
    c = new_img(2, 2, NEUTRAL_EDGE)
    px(c, 1, 1, NEUTRAL_FILL)
    return c


def paste_4_corners(img, corner):
    """Stamp `corner` into all four corners of `img`, mirrored so each
    corner's bevel points inward."""
    cw, ch = corner.size
    iw, ih = img.size
    tl = corner
    tr = corner.transpose(Image.FLIP_LEFT_RIGHT)
    bl = corner.transpose(Image.FLIP_TOP_BOTTOM)
    br = corner.transpose(Image.ROTATE_180)
    img.paste(tl, (0, 0), tl)
    img.paste(tr, (iw - cw, 0), tr)
    img.paste(bl, (0, ih - ch), bl)
    img.paste(br, (iw - cw, ih - ch), br)


# ---------- assets ---------------------------------------------------------


def gen_panel_frame():
    """32x32 panel with dark wood interior, brass perimeter, and 8x8 corner
    caps. The 9-slice border is 8px so corners stay at native size and the
    edge stripes (in the 8..24 sides region) stretch cleanly."""
    img = new_img(32, 32, WOOD_DARK)

    # Subtle marbled noise inside the slicer's 16x16 center region
    # (x=8..24, y=8..24). The slicer is configured with `Tile` for the
    # center, so this 16x16 tile repeats at native pixel size — no
    # nearest-neighbor stretching, no visible checker artifacts.
    #
    # Recipe: deterministic per-pixel hash noise (the "stipple"), plus a
    # smooth diagonal sinusoid (the "marble flow"). Two thresholds give
    # three tones: WOOD_DARK base, faint WOOD_GRAIN vein, sparse
    # WOOD_LIGHT highlight. All values are computed in the wrapped 16x16
    # tile space so the tile is seamlessly repeatable.
    for v in range(16):
        for u in range(16):
            h = ((u * 73856093) ^ (v * 19349663)) & 0xFFFF
            noise = h / 65535.0  # 0..1
            # Diagonal flow: a single sine wave with period 16 along the
            # (u+v) diagonal, so the marbled pattern reads as flowing
            # across the panel rather than as uniform stipple.
            flow = 0.5 + 0.5 * math.sin(2 * math.pi * (u + v) / 16.0)
            val = 0.55 * noise + 0.45 * flow
            if val > 0.86:
                px(img, 8 + u, 8 + v, WOOD_LIGHT)
            elif val > 0.62:
                px(img, 8 + u, 8 + v, WOOD_GRAIN)
            # else: leave as WOOD_DARK from the canvas fill.

    # Outer outline (perimeter).
    rect_outline(img, 0, 0, 32, 32, OUTLINE)
    # Brass perimeter strip one pixel inside the outline. The slicer
    # stretches this line along the four edge "sides" regions.
    rect_outline(img, 1, 1, 30, 30, BRASS_MID)
    # Bright top/left highlight strip, dark bottom/right shadow strip
    # — these all live in the 8..24 sides region so they stretch as solid
    # lines across the edges of any sized window.
    hline(img, 8, 2, 16, BRASS_HI)
    hline(img, 8, 29, 16, BRASS_LO)
    vline(img, 2, 8, 16, BRASS_HI)
    vline(img, 29, 8, 16, BRASS_LO)
    # Inner shadow line just inside the brass strip (separates frame from
    # wood interior).
    rect_outline(img, 3, 3, 26, 26, BRASS_DARK)

    paste_4_corners(img, brass_corner_8())
    img.save(f"{OUT_DIR}/panel_frame.png")


def gen_title_bar():
    """32x16 title bar — warm wood with a gold top stripe, dark bottom
    underline, and 4x4 brass corner caps."""
    img = new_img(32, 16, WOOD_LIGHT)
    # Center of the title bar is left flat — it will be Tile-mode in the
    # slicer (so it'd visibly repeat any feature) and the gold/dark stripes
    # in the edge region carry the visual interest.

    # Outer outline.
    rect_outline(img, 0, 0, 32, 16, OUTLINE)
    # Brass top edge highlight and bottom shadow — sit inside the 4..28
    # "sides" range so they stretch across the bar's full width.
    hline(img, 4, 1, 24, BRASS_HI)
    hline(img, 4, 2, 24, BRASS_MID)
    hline(img, 4, 13, 24, BRASS_LO)
    hline(img, 4, 14, 24, BRASS_DARK)
    # Side gold accents.
    vline(img, 1, 4, 8, BRASS_MID)
    vline(img, 30, 4, 8, BRASS_MID)

    paste_4_corners(img, brass_corner_4())
    img.save(f"{OUT_DIR}/title_bar.png")


def gen_button_frame():
    """24x24 tint-neutral button frame. Most pixels are white-ish so the
    button's per-state palette tint controls the look; only the corners
    and edges carry a subtle bevel."""
    img = new_img(24, 24, NEUTRAL_FILL)

    rect_outline(img, 0, 0, 24, 24, NEUTRAL_EDGE)
    # Top highlight, bottom shadow within the slicer "sides" range (4..20).
    hline(img, 4, 1, 16, NEUTRAL_HI)
    hline(img, 4, 22, 16, NEUTRAL_LO)
    vline(img, 1, 4, 16, NEUTRAL_HI)
    vline(img, 22, 4, 16, NEUTRAL_LO)

    paste_4_corners(img, neutral_corner_4())
    img.save(f"{OUT_DIR}/button_frame.png")


def gen_slot_frame():
    """40x40 inventory slot — recessed inset with a 2px slicer border so
    edges read as a clean 1-pixel line at any slot size."""
    img = new_img(40, 40, NEUTRAL_FILL)
    rect_outline(img, 0, 0, 40, 40, NEUTRAL_EDGE)
    rect_outline(img, 1, 1, 38, 38, NEUTRAL_LO)
    paste_4_corners(img, neutral_corner_2())
    img.save(f"{OUT_DIR}/slot_frame.png")


def gen_divider():
    """8x2 hairline — pure white so the caller's tint controls the color."""
    img = new_img(8, 2, NEUTRAL_HI)
    img.save(f"{OUT_DIR}/divider.png")


def gen_close_button():
    """18x18 circular brass medallion with the X already embedded. Used as a
    self-contained close button — no rectangular button_frame underneath."""
    img = new_img(18, 18, CLEAR)
    _draw_medallion(img)
    # Inset X (4-px long diagonals around the disc center).
    for i in range(5):
        ax, ay = 6 + i, 6 + i
        bx, by = 11 - i, 6 + i
        px(img, ax, ay, BRASS_HI)
        px(img, bx, by, BRASS_HI)
        px(img, ax, ay - 1, BRASS_MID)
        px(img, bx, by - 1, BRASS_MID)
    img.save(f"{OUT_DIR}/close_button.png")


def _draw_medallion(img):
    """Stamp an 18x18 brass medallion (background ring + dark face). Caller
    overlays an icon on top of the dark face. Same recipe as
    `gen_close_button`; factored out so the dock/undock buttons match."""
    cx, cy = 8.5, 8.5
    for y in range(18):
        for x in range(18):
            dx = x - cx
            dy = y - cy
            r = (dx * dx + dy * dy) ** 0.5
            if r > 8.5:
                continue
            if r > 7.5:
                px(img, x, y, OUTLINE)
            elif r > 6.5:
                if dx + dy < -1:
                    px(img, x, y, BRASS_HI)
                elif dx + dy < 1:
                    px(img, x, y, BRASS_MID)
                else:
                    px(img, x, y, BRASS_LO)
            elif r > 4.5:
                px(img, x, y, BRASS_MID)
            elif r > 3.5:
                px(img, x, y, BRASS_DARK)
            else:
                px(img, x, y, WOOD_DARK)


def gen_dock_button():
    """18x18 brass medallion with an arrow pointing right into a vertical
    bar — visual metaphor for "send this window into the sidebar dock"."""
    img = new_img(18, 18, CLEAR)
    _draw_medallion(img)
    # Arrow shaft (3 pixels horizontal, leftward bias).
    for x in (6, 7, 8):
        px(img, x, 8, BRASS_HI)
        px(img, x, 9, BRASS_MID)
    # Arrowhead (right-pointing chevron).
    px(img, 9, 7, BRASS_HI)
    px(img, 10, 8, BRASS_HI)
    px(img, 10, 9, BRASS_HI)
    px(img, 9, 10, BRASS_HI)
    # Vertical "dock" bar to the right of the arrow.
    for y in (6, 7, 8, 9, 10, 11):
        px(img, 12, y, BRASS_HI)
    img.save(f"{OUT_DIR}/dock_button.png")


def gen_undock_button():
    """18x18 brass medallion with a diagonal arrow exiting a frame in the
    top-right — visual metaphor for "pop this docked panel out into a
    floating window"."""
    img = new_img(18, 18, CLEAR)
    _draw_medallion(img)
    # Small frame in the bottom-left quadrant (3x3 hollow square).
    for x in (5, 6, 7):
        px(img, x, 12, BRASS_HI)
        px(img, x, 10, BRASS_HI)
    for y in (10, 11, 12):
        px(img, 5, y, BRASS_HI)
        px(img, 7, y, BRASS_HI)
    # Diagonal arrow exiting toward the top-right (4-pixel diagonal).
    for i in range(4):
        px(img, 8 + i, 9 - i, BRASS_HI)
    # Arrowhead: two pixels marking the tip of the diagonal.
    px(img, 12, 5, BRASS_HI)
    px(img, 11, 5, BRASS_HI)
    px(img, 12, 6, BRASS_HI)
    img.save(f"{OUT_DIR}/undock_button.png")


def gen_resize_grip():
    """16x10 horizontal brass grip used by docked-panel resize handles
    (drag the bottom edge to resize the panel vertically).

    Dark wood border top + bottom, brass middle band with three pairs of
    raised pinstripe dashes acting as a "drag-this-edge" affordance. Tile
    horizontally across the panel's full width.
    """
    img = new_img(16, 10, WOOD_DARK)
    # Outer outline top + bottom, no left/right (tiles horizontally).
    hline(img, 0, 0, 16, OUTLINE)
    hline(img, 0, 9, 16, OUTLINE)
    # Brass band background spanning the middle 6 rows.
    for y in (2, 3, 4, 5, 6, 7):
        for x in range(16):
            px(img, x, y, BRASS_LO if y in (2, 7) else BRASS_MID)
    # Top + bottom shadow rows just inside the outline.
    hline(img, 0, 1, 16, BRASS_DARK)
    hline(img, 0, 8, 16, BRASS_DARK)
    # Three pairs of pinstripe grip dashes (highlight + shadow). Centered
    # vertically. Spaced at x = 2, 8, 14 for visual rhythm.
    for cx in (2, 8, 14):
        # Two-pixel-wide stub.
        for ox in (0, 1):
            px(img, cx + ox, 3, BRASS_HI)
            px(img, cx + ox, 4, BRASS_MID)
            px(img, cx + ox, 5, BRASS_LO)
            px(img, cx + ox, 6, BRASS_DARK)
    img.save(f"{OUT_DIR}/resize_grip.png")


def gen_resize_corner():
    """18x18 brass corner ornament oriented for the bottom-right of a
    window. A brass wedge filling the lower-right triangle, with three
    parallel diagonal grip ridges so the user reads it as a drag handle.
    Larger than the panel's 8x8 corner caps so it's comfortably grabbable."""
    img = new_img(18, 18, CLEAR)
    # Outer outline along the bottom + right edges.
    hline(img, 0, 17, 18, OUTLINE)
    vline(img, 17, 0, 18, OUTLINE)
    # Inner diagonal outline along the wedge's hypotenuse.
    for i in range(10):
        px(img, i, 9 - i, OUTLINE)
    # Brass wedge fill (everything strictly inside the triangle and not on
    # an outline pixel).
    for y in range(17):
        for x in range(17):
            if x + y <= 9:
                continue  # transparent or outline above the diagonal
            d = x + y
            if d <= 12:
                px(img, x, y, BRASS_HI)
            elif d <= 18:
                px(img, x, y, BRASS_MID)
            elif d <= 24:
                px(img, x, y, BRASS_LO)
            else:
                px(img, x, y, BRASS_DARK)
    # Three parallel grip ridges near the inner diagonal. Each ridge is a
    # 1-px dark line with a 1-px bright highlight one row above it,
    # creating an embossed pinstripe look.
    for ridge in (12, 14, 16):
        # The dark stroke.
        for x in range(ridge - 14, ridge + 1):
            y = ridge - x
            if 0 <= x < 17 and 0 <= y < 17:
                px(img, x, y, BRASS_DARK)
        # The bright highlight one row above (still inside the wedge).
        for x in range(ridge - 14, ridge + 1):
            y = ridge - x - 1
            if 0 <= x < 17 and 0 <= y < 17 and x + y > 9:
                px(img, x, y, BRASS_HI)
    img.save(f"{OUT_DIR}/resize_corner.png")


def gen_close_icon():
    """14x14 brass 'X' on transparent background. Two 2-px diagonals with
    a 1-px bright bevel above and a 1-px dark shadow below each stroke."""
    img = new_img(14, 14, CLEAR)
    for i in range(8):
        # Diagonal A: top-left -> bottom-right, from (3,3) to (10,10).
        ax, ay = 3 + i, 3 + i
        # Diagonal B: top-right -> bottom-left, from (10,3) to (3,10).
        bx, by = 10 - i, 3 + i
        # Body stroke.
        px(img, ax, ay, BRASS_MID)
        px(img, bx, by, BRASS_MID)
        # Highlight one pixel above each stroke.
        px(img, ax, ay - 1, BRASS_HI)
        px(img, bx, by - 1, BRASS_HI)
        # Shadow one pixel below each stroke.
        px(img, ax, ay + 1, BRASS_LO)
        px(img, bx, by + 1, BRASS_LO)
    # Central glint where the two strokes cross.
    px(img, 6, 6, BRASS_HI)
    px(img, 7, 6, BRASS_HI)
    px(img, 6, 7, BRASS_HI)
    px(img, 7, 7, BRASS_HI)
    img.save(f"{OUT_DIR}/close_icon.png")


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    gen_panel_frame()
    gen_title_bar()
    gen_button_frame()
    gen_slot_frame()
    gen_divider()
    gen_close_icon()
    gen_close_button()
    gen_dock_button()
    gen_undock_button()
    gen_resize_grip()
    gen_resize_corner()
    print(f"Generated theme assets in {OUT_DIR}/")


if __name__ == "__main__":
    main()
