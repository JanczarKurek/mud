"""
Generates the Grey Ledger clerk sprites (assets/modules/grey_ledger/):

  overworld_objects/assessor_quill/sheet.png   - 4 cols x 8 rows (384x768)
  overworld_objects/assessor_quill/sprite.png  - static south idle (96x96)
  overworld_objects/underclerk_moss/sheet.png  - 4 cols x 8 rows (384x768)
  overworld_objects/underclerk_moss/sprite.png - static south idle (96x96)

Assessor Quill: a tall grey heron clerk on stilt legs — long south-jutting
beak, dark crest, ink-stained wing tips, a silver pince-nez painted on the
south face. Art tops out ~1.17 floors.

Underclerk Moss: a small round brown vole — ball body in a green waistcoat,
neckless head, tiny ears with a pair of spectacles pushed up between them,
thin tail. Art tops out ~0.68 floors.

Both are custom part stacks on scripts/char_rig.py (shared 4-facing oblique
box rig, see docs/sprite_style.md): 96x96 frames, anchor at
(W//2 - TILE_PX//2, H-1), facings are true rotations about the tile centre so
the beak/snout really point the way the character walks. Deterministic — no
randomness anywhere.
"""

from char_rig import (
    IDLE_FRAMES,
    WALK_FRAMES,
    _px,
    assemble,
    render_frame,
    save,
    scale_color,
    triple,
)
from wall_perspective import project

FRAME_W = 96
FRAME_H = 96
OUT_ROOT = "assets/modules/grey_ledger/overworld_objects"

# ── Palettes ─────────────────────────────────────────────────────────────────
# Quill (heron)
PLUME = triple((170, 174, 184, 255))     # body plumage
WING = triple((140, 145, 158, 255))      # folded wings, a shade darker
INK = triple((40, 42, 60, 255))          # ink-stained wing tips
LEG = triple((88, 74, 46, 255))          # wading-bird legs, dark so the beak pops
BEAK = triple((214, 166, 70, 255))       # long amber beak
CREST = triple((72, 76, 90, 255))        # dark slate crest + plume
SILVER = (202, 206, 216, 255)            # pince-nez wire

# Moss (vole)
FUR = triple((150, 118, 82, 255))
FUR_DARK = triple((106, 82, 56, 255))
BELLY = triple((196, 170, 132, 255))
COAT = triple((78, 100, 62, 255))        # waistcoat green
NOSE = (216, 122, 122, 255)

EYE_WHITE = (240, 240, 244, 255)
PUPIL = (34, 30, 28, 255)


# ── Assessor Quill: tall heron ───────────────────────────────────────────────

def quill_parts():
    parts = [
        # Stilt legs (thin, long — the heron silhouette).
        dict(fp=(0.39, 0.45, 0.44, 0.56), fz=(0.0, 0.42), colors=LEG,
             swing_key="l_foot_swing", dz_key="l_foot_dz"),
        dict(fp=(0.55, 0.61, 0.44, 0.56), fz=(0.0, 0.42), colors=LEG,
             swing_key="r_foot_swing", dz_key="r_foot_dz"),
        # Plump ovoid body.
        dict(fp=(0.34, 0.66, 0.36, 0.66), fz=(0.42, 0.72), colors=PLUME,
             dz_key="body_dz"),
        # Folded wings — ink-stained tips below, grey vanes above.
        dict(fp=(0.28, 0.36, 0.40, 0.62), fz=(0.44, 0.50), colors=INK,
             swing_key="l_arm_swing", dz_key="body_dz"),
        dict(fp=(0.64, 0.72, 0.40, 0.62), fz=(0.44, 0.50), colors=INK,
             swing_key="r_arm_swing", dz_key="body_dz"),
        dict(fp=(0.28, 0.36, 0.40, 0.62), fz=(0.50, 0.70), colors=WING,
             swing_key="l_arm_swing", dz_key="body_dz"),
        dict(fp=(0.64, 0.72, 0.40, 0.62), fz=(0.50, 0.70), colors=WING,
             swing_key="r_arm_swing", dz_key="body_dz"),
        # S-neck, simplified: lower segment, upper segment leaning south.
        dict(fp=(0.46, 0.54, 0.42, 0.54), fz=(0.72, 0.88), colors=PLUME,
             dz_key="body_dz"),
        dict(fp=(0.46, 0.54, 0.38, 0.50), fz=(0.88, 1.00), colors=PLUME,
             dz_key="body_dz"),
        # Head.
        dict(fp=(0.40, 0.60, 0.34, 0.58), fz=(1.00, 1.14), colors=PLUME,
             dz_key="body_dz"),
        # Long beak jutting south from the head front.
        dict(fp=(0.46, 0.54, 0.08, 0.36), fz=(1.03, 1.09), colors=BEAK,
             dz_key="body_dz"),
        # Dark crest cap with a plume trailing north.
        dict(fp=(0.42, 0.58, 0.38, 0.66), fz=(1.14, 1.17), colors=CREST,
             dz_key="body_dz", fy_shift_key="hair_dy"),
    ]
    face = dict(fp=(0.40, 0.60, 0.34, 0.58), fz=(1.00, 1.14),
                dz_key="body_dz", style="beast", skin=PLUME[0],
                eye_h=0.55, eye_span=(0.28, 0.72),
                eye_white=EYE_WHITE, eye_pupil=PUPIL)
    return parts, face


def quill_post_paint(img, facing, frame, anchor):
    """Silver pince-nez: two wire rings + bridge, south face only."""
    if facing != "s":
        return
    dz = frame.get("body_dz", 0.0)
    eye_fz = 1.00 + 0.55 * 0.14 + dz
    for t in (0.34, 0.66):
        cx, cy = project(0.40 + t * 0.20, 0.34, eye_fz, anchor)
        for dx, dy in ((-2, -1), (-2, 0), (2, -1), (2, 0),
                       (-1, -2), (0, -2), (-1, 1), (0, 1)):
            _px(img, cx + dx, cy + dy, SILVER)
    bx0, by = project(0.485, 0.34, eye_fz, anchor)
    bx1, _ = project(0.515, 0.34, eye_fz, anchor)
    for x in range(min(bx0, bx1), max(bx0, bx1) + 1):
        _px(img, x, by - 2, SILVER)


QUILL_CFG = dict(
    frame_w=FRAME_W,
    frame_h=FRAME_H,
    parts=quill_parts()[0],
    face=quill_parts()[1],
    post_paint=quill_post_paint,
)


# ── Underclerk Moss: small round vole ────────────────────────────────────────

def moss_parts():
    parts = [
        # Little feet.
        dict(fp=(0.38, 0.46, 0.42, 0.56), fz=(0.0, 0.05), colors=FUR_DARK,
             swing_key="l_foot_swing", dz_key="l_foot_dz"),
        dict(fp=(0.54, 0.62, 0.42, 0.56), fz=(0.0, 0.05), colors=FUR_DARK,
             swing_key="r_foot_swing", dz_key="r_foot_dz"),
        # Ball body.
        dict(fp=(0.32, 0.68, 0.34, 0.68), fz=(0.05, 0.42), colors=FUR,
             dz_key="body_dz"),
        # Waistcoat: a band proud of the body front.
        dict(fp=(0.35, 0.65, 0.325, 0.36), fz=(0.10, 0.36), colors=COAT,
             dz_key="body_dz"),
        # Belly patch above the waistcoat line.
        dict(fp=(0.42, 0.58, 0.33, 0.35), fz=(0.36, 0.41), colors=BELLY,
             dz_key="body_dz"),
        # Neckless head.
        dict(fp=(0.38, 0.62, 0.36, 0.64), fz=(0.42, 0.62), colors=FUR,
             dz_key="body_dz"),
        # Snout with the pink nose (nose painted in post).
        dict(fp=(0.45, 0.55, 0.28, 0.38), fz=(0.46, 0.53), colors=BELLY,
             dz_key="body_dz"),
        # Round ears.
        dict(fp=(0.39, 0.47, 0.42, 0.52), fz=(0.62, 0.69), colors=FUR_DARK,
             dz_key="body_dz"),
        dict(fp=(0.53, 0.61, 0.42, 0.52), fz=(0.62, 0.69), colors=FUR_DARK,
             dz_key="body_dz"),
        # Spectacles pushed up the fur: a silver wire across the head top.
        dict(fp=(0.42, 0.58, 0.44, 0.50), fz=(0.615, 0.63),
             colors=(SILVER, scale_color(SILVER, 0.7),
                     scale_color(SILVER, 1.1)),
             dz_key="body_dz"),
        # Thin tail trailing north.
        dict(fp=(0.48, 0.52, 0.70, 0.90), fz=(0.02, 0.07), colors=FUR_DARK,
             fy_shift_key="tail_dy"),
    ]
    face = dict(fp=(0.38, 0.62, 0.36, 0.64), fz=(0.42, 0.62),
                dz_key="body_dz", style="beast", skin=FUR[0],
                eye_h=0.62, eye_span=(0.26, 0.74),
                eye_white=EYE_WHITE, eye_pupil=PUPIL)
    return parts, face


def moss_post_paint(img, facing, frame, anchor):
    """Pink nose tip on the snout, south face only."""
    if facing != "s":
        return
    dz = frame.get("body_dz", 0.0)
    x, y = project(0.5, 0.28, 0.50 + dz, anchor)
    _px(img, x, y, NOSE)
    _px(img, x - 1, y, NOSE)


MOSS_CFG = dict(
    frame_w=FRAME_W,
    frame_h=FRAME_H,
    parts=moss_parts()[0],
    face=moss_parts()[1],
    post_paint=moss_post_paint,
)


# ── Output ───────────────────────────────────────────────────────────────────

def emit(name, cfg):
    sheet = assemble(cfg)
    save(sheet, f"{OUT_ROOT}/{name}/sheet.png")
    static = render_frame(cfg, "s", IDLE_FRAMES[0])
    save(static, f"{OUT_ROOT}/{name}/sprite.png")


if __name__ == "__main__":
    # Referenced only for the walk rows baked in by assemble()'s defaults.
    _ = WALK_FRAMES
    emit("assessor_quill", QUILL_CFG)
    emit("underclerk_moss", MOSS_CFG)
