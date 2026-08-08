"""
Generates the player sprite sets — the base `player` definition plus the four
per-class variants (`player_fighter`, `player_wizard`, `player_cleric`,
`player_vagabond`) — on the shared `char_rig` renderer (same oblique
projection as the wall set; see docs/sprite_style.md).

Every set shares the SAME sheet contract (the runtime layer-atlas sharing and
the character-create preview depend on it):

  frame 128×96 px, 4 columns × 8 rows = 512 × 768 px
  Row 0: idle_s   Row 1: walk_s   Row 2: idle_n   Row 3: walk_n
  Row 4: idle_e   Row 5: walk_e   Row 6: idle_w   Row 7: walk_w

Outputs per definition, under assets/overworld_objects/<def_id>/:
  sheet.png            full-color base sheet
  layers/hair.png      hair-region pixels, white-tone palette (tintable)
  layers/torso.png     torso-region pixels, white-tone palette (tintable)
  layers/trousers.png  trousers-region pixels, white-tone palette (tintable)
  sprite_large.png     single south-facing idle frame (static fallback)

Occlusion correctness: char_rig paints ALL boxes in one globally depth-sorted
painter pass, and layer sheets render the same pass with non-region boxes
painted as fully-transparent ERASERS — so a layer only keeps pixels that are
actually visible in the base render (the old per-region painter drew the
tunic's top cap over the head; `verify()` below guards against regressions).

Class silhouettes (all keep the recolorable hair/torso/trousers regions):
  fighter   broad torso, steel breastplate band + pauldrons
  wizard    floor-length robe + a tall pointed hat on the *hair* slot
  cleric    floor-length robe on the trousers slot + a tabard on the *torso*
            slot (they must differ, or the tabard vanishes into the gown)
  vagabond  slim build, hooded cloak (hood/cape = torso region), hair rim

Robed classes draw no boots: the hem runs to the ground, both because a full
robe hides the feet and because a boot box cannot sort behind the skirt here
(see `_robe_parts`).
"""

import os

from PIL import Image

from wall_perspective import TILE_PX, BG, project
from char_rig import (
    anchor_for,
    assemble,
    render_frame,
    save,
    standard_rows,
    IDLE_FRAMES,
    WALK_FRAMES,
)

# ── Frame & sheet geometry (shared-grid invariant — do not change per class) ──
#
# 128 WIDE, 96 tall. Height in this projection travels up-*left* at 36 px per
# floor, so canvas width — not height — is what limits how tall a hat or helm
# can be: at 96 wide the anchor sits at x=24 and nothing above fz≈1.25 fits,
# which is not enough for a pointed wizard hat. 128 moves the anchor to x=40
# and buys ~0.4 more floors of headroom. The character art is unchanged in tile
# space; the extra width is transparent margin, and `project(0.5, 0, 0)` still
# lands on the frame's bottom-center as `Anchor::BOTTOM_CENTER` requires.
FRAME_W = 128
FRAME_H = 96
COLS = 4
ROWS = 8

OUT_ROOT = "assets/overworld_objects"


# ── Palette ───────────────────────────────────────────────────────────────────
# (front, east-side shadow, lit top cap) triples, matching the wall set's
# 3-tone treatment. Base-sheet colors only; the recolor layers are rendered
# in char_rig.LAYER_TRIPLE white tones and tinted at runtime.

SKIN = ((220, 170, 120, 255), (180, 130, 85, 255), (240, 195, 150, 255))
HAIR = ((220, 180, 35, 255), (170, 130, 20, 255), (250, 215, 80, 255))
TUNIC = ((145, 55, 165, 255), (100, 30, 120, 255), (175, 90, 200, 255))
PANTS = ((55, 70, 105, 255), (35, 48, 75, 255), (80, 98, 140, 255))
BOOT = ((72, 44, 18, 255), (45, 28, 10, 255), (95, 62, 28, 255))
BELT = ((130, 85, 25, 255), (90, 55, 15, 255), (170, 120, 45, 255))

STEEL = ((150, 155, 170, 255), (105, 110, 125, 255), (195, 200, 215, 255))
GOLD = ((205, 165, 60, 255), (150, 115, 35, 255), (240, 205, 105, 255))

# The wizard hat is a stack of shrinking boxes, and this projection shows every
# box's top cap — with the usual wide tonal spread those caps read as bright
# concentric rings rather than one cone. Both the base sheet and (via
# `layer_colors`) the tintable layer use a compressed spread so the hat reads
# as a single felt silhouette that still has some form.
HAT_FELT = ((208, 172, 40, 255), (188, 154, 32, 255), (222, 188, 58, 255))
HAT_LAYER = ((220, 220, 220, 255), (198, 198, 198, 255), (233, 233, 233, 255))

EYE_WHITE = (240, 240, 240, 255)
EYE_PUPIL = (20, 20, 30, 255)
MOUTH = (110, 60, 45, 255)


# ── Body geometry (canonical SOUTH facing, symmetric about (0.5, 0.5)) ────────
# Stacked fz bands (floors). Head lean is 36 px/floor up-LEFT; the 96-wide
# canvas absorbs it up to fz ≈ (24 + fx0*48) / 36 for a box's west edge fx0 —
# verify() bounds-checks every frame, hat included.
BOOT_TOP = 0.08
PANTS_TOP = 0.46
BELT_TOP = 0.52
TORSO_TOP = 0.86
NECK_TOP = 0.91
HEAD_TOP = 1.08
HAIR_TOP = 1.10
ARM_BOTTOM = 0.52
SLEEVE_BOTTOM = 0.70

LEFT_LEG = (0.34, 0.45, 0.40, 0.60)
RIGHT_LEG = (0.55, 0.66, 0.40, 0.60)
TORSO_FP = (0.32, 0.68, 0.38, 0.62)
NECK_FP = (0.46, 0.54, 0.43, 0.57)
HEAD_FP = (0.36, 0.64, 0.36, 0.64)
LEFT_ARM = (0.28, 0.34, 0.42, 0.58)
RIGHT_ARM = (0.66, 0.72, 0.42, 0.58)


def _slim(fp, k):
    """Scale a footprint's fx extent about the tile centre 0.5."""
    fx0, fx1, fy0, fy1 = fp
    return (0.5 + (fx0 - 0.5) * k, 0.5 + (fx1 - 0.5) * k, fy0, fy1)


def _widen(fp, pad):
    fx0, fx1, fy0, fy1 = fp
    return (fx0 - pad, fx1 + pad, fy0, fy1)


# ── Shared part groups ────────────────────────────────────────────────────────

def _boots(slim=1.0):
    return [
        dict(fp=_slim(LEFT_LEG, slim), fz=(0.0, BOOT_TOP), colors=BOOT,
             swing_key="l_foot_swing", dz_key="l_foot_dz"),
        dict(fp=_slim(RIGHT_LEG, slim), fz=(0.0, BOOT_TOP), colors=BOOT,
             swing_key="r_foot_swing", dz_key="r_foot_dz"),
    ]


def _pant_legs(slim=1.0):
    return [
        dict(fp=_slim(LEFT_LEG, slim), fz=(BOOT_TOP, PANTS_TOP), colors=PANTS,
             swing_key="l_foot_swing", dz_key="l_foot_dz", region="trousers"),
        dict(fp=_slim(RIGHT_LEG, slim), fz=(BOOT_TOP, PANTS_TOP), colors=PANTS,
             swing_key="r_foot_swing", dz_key="r_foot_dz", region="trousers"),
    ]


def _arms(sleeve_bottom=SLEEVE_BOTTOM, sleeve_top=TORSO_TOP, slim=1.0,
          sleeve_region="torso"):
    """Forearm (skin) below, sleeve above. `sleeve_region` picks which slider
    tints the sleeve — robed classes point it at the robe's own region so the
    sleeves match the gown rather than a separate shirt."""
    la, ra = _slim(LEFT_ARM, slim), _slim(RIGHT_ARM, slim)
    sleeve_colors = TUNIC if sleeve_region == "torso" else PANTS
    return [
        dict(fp=la, fz=(ARM_BOTTOM, sleeve_bottom), colors=SKIN,
             swing_key="l_arm_swing", dz_key="body_dz"),
        dict(fp=ra, fz=(ARM_BOTTOM, sleeve_bottom), colors=SKIN,
             swing_key="r_arm_swing", dz_key="body_dz"),
        dict(fp=la, fz=(sleeve_bottom, sleeve_top), colors=sleeve_colors,
             swing_key="l_arm_swing", dz_key="body_dz", region=sleeve_region),
        dict(fp=ra, fz=(sleeve_bottom, sleeve_top), colors=sleeve_colors,
             swing_key="r_arm_swing", dz_key="body_dz", region=sleeve_region),
    ]


def _neck_head(slim=1.0):
    head_fp = _slim(HEAD_FP, slim)
    parts = [
        dict(fp=_slim(NECK_FP, slim), fz=(TORSO_TOP, NECK_TOP), colors=SKIN,
             dz_key="body_dz"),
        dict(fp=head_fp, fz=(NECK_TOP, HEAD_TOP), colors=SKIN,
             dz_key="body_dz", face_part=True),
    ]
    face = dict(fp=head_fp, fz=(NECK_TOP, HEAD_TOP), dz_key="body_dz",
                style="human", skin=SKIN[0], eye_white=EYE_WHITE,
                eye_pupil=EYE_PUPIL, mouth=MOUTH)
    return parts, face


def _hair_cap(slim=1.0, fz=(HEAD_TOP, HAIR_TOP)):
    return dict(fp=_slim(HEAD_FP, slim), fz=fz, colors=HAIR,
                dz_key="body_dz", fy_shift_key="hair_dy", region="hair")


def _belt(fp=TORSO_FP, fz=(PANTS_TOP, BELT_TOP)):
    return dict(fp=fp, fz=fz, colors=BELT, dz_key="body_dz")


# ── Class silhouettes ─────────────────────────────────────────────────────────

def style_base():
    """The original all-purpose villager silhouette (fallback definition)."""
    parts = _boots() + _pant_legs()
    parts.append(_belt())
    parts.append(dict(fp=TORSO_FP, fz=(BELT_TOP, TORSO_TOP), colors=TUNIC,
                      dz_key="body_dz", region="torso"))
    parts += _arms()
    neck_head, face = _neck_head()
    parts += neck_head
    parts.append(_hair_cap())
    return parts, face


def style_fighter():
    """Broad torso; steel breastplate band + pauldrons over the tunic."""
    torso = _widen(TORSO_FP, 0.03)
    parts = _boots() + _pant_legs()
    parts.append(_belt(fp=torso))
    # Torso split into disjoint fz bands: tunic / breastplate / tunic, so the
    # steel band needs no overlapping-box tricks.
    parts.append(dict(fp=torso, fz=(BELT_TOP, 0.56), colors=TUNIC,
                      dz_key="body_dz", region="torso"))
    parts.append(dict(fp=torso, fz=(0.56, 0.70), colors=STEEL,
                      dz_key="body_dz"))
    parts.append(dict(fp=torso, fz=(0.70, TORSO_TOP), colors=TUNIC,
                      dz_key="body_dz", region="torso"))
    # Shorter sleeves leave room for the pauldrons capping each arm.
    parts += _arms(sleeve_bottom=SLEEVE_BOTTOM, sleeve_top=0.80)
    for arm_fp, swing in ((LEFT_ARM, "l_arm_swing"), (RIGHT_ARM, "r_arm_swing")):
        parts.append(dict(fp=_widen(arm_fp, 0.01), fz=(0.80, 0.90),
                          colors=STEEL, swing_key=swing, dz_key="body_dz"))
    neck_head, face = _neck_head()
    parts += neck_head
    parts.append(_hair_cap())
    return parts, face


def _robe_parts(sleeve_bottom, *, body_region="torso", skirt_region="trousers"):
    """Shared wizard/cleric robe: floor-length skirt, sash, robe body, sleeves.

    **No boots.** The hem runs to the ground because a full robe hides the
    feet — and because boot boxes genuinely cannot sort behind the skirt here:
    the painter key falls through to the fx bucket, where the right boot
    (fx≈0.605 → bucket 6) lands after the skirt (0.50 → bucket 5), so its top
    cap punched through the gown.

    `body_region` / `skirt_region` pick which appearance slider tints each
    half. The cleric points both at one slot so its tabard can own a
    *different* slot — a tabard tinted the same as the robe under it would be
    invisible, which defeats the garment.
    """
    body_colors = TUNIC if body_region == "torso" else PANTS
    skirt_colors = TUNIC if skirt_region == "torso" else PANTS
    parts = [
        dict(fp=(0.33, 0.67, 0.37, 0.63), fz=(0.0, BELT_TOP),
             colors=skirt_colors, dz_key="body_dz", region=skirt_region),
        _belt(fz=(BELT_TOP, 0.58)),
        dict(fp=TORSO_FP, fz=(0.58, TORSO_TOP), colors=body_colors,
             dz_key="body_dz", region=body_region),
    ]
    parts += _arms(sleeve_bottom=sleeve_bottom, sleeve_region=body_region)
    return parts


def style_wizard():
    """Floor-length robe + a tall pointed hat that the hair slider tints.

    The hat takes the `hair` region and replaces the hair cap outright: it is
    the wizard's head item, so the head slider colors the hat instead of hair
    nobody can see under it.
    """
    parts = _robe_parts(sleeve_bottom=0.60)
    neck_head, face = _neck_head()
    parts += neck_head
    # Flared brim + a long tapering spire. The brim starts below HEAD_TOP so
    # the hat sits down over the skull (no hair cap needed) while clearing the
    # eye row at fz ≈ 1.00.
    #
    # Canvas budget: the head leans up-left 36 px per floor from the anchor at
    # x = FRAME_W//2 - 24, so every box must satisfy
    # `(min_edge - 0.01) * 48 + (FRAME_W//2 - 24) >= 36 * (fz_top + 0.025) + 1`,
    # where min_edge is the smallest of fx0 / 1-fx1 / fy0 / 1-fy1 (facing
    # rotations swap the axes), 0.01 covers the `hair_dy` sway and 0.025 the
    # walk-frame body bob. verify() enforces it.
    #
    # Only three boxes: every box shows its lit top cap in this top-down-ish
    # projection, so a finely-stepped cone renders as concentric rings ("a
    # maze, not a hat"). A big taper across few steps reads as brim → spire →
    # point instead.
    # The brim is wider than the head, so it must sit *above* HEAD_TOP (1.08):
    # a flared brim any lower overhangs the head's front face, and that face is
    # only ~4 px tall in this projection — the brim's shadowed bottom seam
    # lands straight on the eye row. verify() catches exactly that.
    for fp, fz in (
        ((0.35, 0.65, 0.35, 0.65), (1.09, 1.13)),
        ((0.42, 0.58, 0.42, 0.58), (1.13, 1.34)),
        ((0.47, 0.53, 0.47, 0.53), (1.34, 1.58)),
    ):
        parts.append(dict(fp=fp, fz=fz, colors=HAT_FELT, dz_key="body_dz",
                          fy_shift_key="hair_dy", region="hair",
                          layer_colors=HAT_LAYER))
    return parts, face


def style_cleric():
    """Robe + a tabard the torso slider tints + gold circlet.

    The robe (body, skirt and sleeves) collapses onto the `trousers` slot so
    the tabard can own `torso` — tinting both from one slider would paint the
    tabard the same color as the gown behind it and erase the garment.
    """
    parts = _robe_parts(
        sleeve_bottom=SLEEVE_BOTTOM,
        body_region="trousers",
        skirt_region="trousers",
    )
    # Tabard: thin slab hanging in front of the robe (the rig's apron pattern),
    # from the chest to below the sash. Its fy sits south of the robe, so the
    # painter's pass puts it in front without any z fudging.
    parts.append(dict(fp=(0.41, 0.59, 0.348, 0.378), fz=(0.14, 0.80),
                      colors=TUNIC, dz_key="body_dz", region="torso"))
    neck_head, face = _neck_head()
    parts += neck_head
    # Circlet: thin gold band slightly proud of the head at the hair base.
    # (The projected face front is only ~4 px tall, so any band below fz≈1.05
    # lands straight on the eye row.)
    parts.append(dict(fp=_widen(HEAD_FP, 0.015), fz=(1.06, 1.085),
                      colors=GOLD, dz_key="body_dz"))
    parts.append(_hair_cap())
    return parts, face


def style_vagabond():
    """Slim build under a hooded cloak; hair reads as a front fringe."""
    slim = 0.88
    parts = _boots(slim) + _pant_legs(slim)
    torso = _slim(TORSO_FP, slim)
    parts.append(_belt(fp=torso))
    parts.append(dict(fp=torso, fz=(BELT_TOP, 0.80), colors=TUNIC,
                      dz_key="body_dz", region="torso"))
    # Sleeves stop where the shoulder cape starts (disjoint fz bands).
    parts += _arms(sleeve_bottom=SLEEVE_BOTTOM, sleeve_top=0.80, slim=slim)
    # Shoulder cape: wider than the torso, covers arms + torso top.
    parts.append(dict(fp=_widen(torso, 0.04), fz=(0.80, TORSO_TOP),
                      colors=TUNIC, dz_key="body_dz", region="torso"))
    neck_head, face = _neck_head(slim)
    parts += neck_head
    # Hood: cowl panels hugging the head's back and sides up to temple height,
    # plus a dome over the crown. The face stays open to the south; east/west
    # facings legitimately show hood panel instead of profile. Canvas-bounds
    # constraint (36 px/floor up-left lean, worst rotation uses the smallest
    # of fx0 / 1-fx1 / fy0 / 1-fy1): min_edge*48 + 24 >= 36*(fz1+0.025) + 1.
    head = _slim(HEAD_FP, slim)
    hx0, hx1, hy0, hy1 = head
    hood = TUNIC
    parts.append(dict(fp=(hx0 - 0.02, hx1 + 0.02, hy1 - 0.03, hy1 + 0.03),
                      fz=(TORSO_TOP, 1.06), colors=hood, dz_key="body_dz",
                      region="torso"))  # back panel
    parts.append(dict(fp=(hx0 - 0.03, hx0 + 0.02, hy0, hy1 + 0.03),
                      fz=(TORSO_TOP, 1.06), colors=hood, dz_key="body_dz",
                      region="torso"))  # west panel
    parts.append(dict(fp=(hx1 - 0.02, hx1 + 0.03, hy0, hy1 + 0.03),
                      fz=(TORSO_TOP, 1.06), colors=hood, dz_key="body_dz",
                      region="torso"))  # east panel
    # Hair: a rim peeking out between the cowl top (1.06) and the crown dome,
    # visible from every facing. (A front fringe over the face doesn't work —
    # the head's front face is only ~4 px tall in this projection, so bangs
    # would paint straight over the eyes.)
    parts.append(dict(fp=head, fz=(1.06, HEAD_TOP), colors=HAIR,
                      dz_key="body_dz", region="hair"))
    parts.append(dict(fp=(0.40, 0.60, 0.40, 0.62), fz=(HEAD_TOP, 1.12),
                      colors=hood, dz_key="body_dz",
                      fy_shift_key="hair_dy", region="torso"))  # crown dome
    return parts, face


CLASS_STYLES = {
    "player": style_base,
    "player_fighter": style_fighter,
    "player_wizard": style_wizard,
    "player_cleric": style_cleric,
    "player_vagabond": style_vagabond,
}

LAYER_KEYS = ("hair", "torso", "trousers")


def build_cfg(style_fn):
    parts, face = style_fn()
    return dict(frame_w=FRAME_W, frame_h=FRAME_H, parts=parts, face=face)


# ── Verification ──────────────────────────────────────────────────────────────

def _frames_of(rows):
    for facing, frames in rows:
        for frame in frames:
            yield facing, frame


def _sample_head_front(cfg, facing, frame, def_id):
    """Screen-sample points strictly inside the head's camera-visible face
    (south face for facing 's', east face for facing 'e' — mirroring
    char_rig.paint_face)."""
    head = next(p for p in cfg["parts"] if p.get("face_part"))
    from char_rig import rotate_xy

    rx0, rx1, ry0, ry1 = rotate_xy(head["fp"], facing)
    dz = frame.get("body_dz", 0.0)
    fz0, fz1 = head["fz"][0] + dz, head["fz"][1] + dz
    anchor = anchor_for(FRAME_W, FRAME_H)
    pts = []
    steps = 10
    for i in range(2, steps - 1):
        for j in range(2, steps - 1):
            # Inset from the box edges so edge strokes don't false-positive.
            fz = fz0 + (fz1 - fz0) * j / steps
            if facing == "s":
                fx = rx0 + (rx1 - rx0) * i / steps
                pts.append(project(fx, ry0, fz, anchor))
            else:
                fy = ry0 + (ry1 - ry0) * i / steps
                pts.append(project(rx1, fy, fz, anchor))
    return pts


def verify():
    """Regression guards:
    1. every frame of every class stays inside the 96×96 canvas (no opaque
       pixel on the frame border — catches wizard-hat overflow);
    2. the torso layer never covers the head's front face on classes whose
       face is uncovered (the original tunic-over-head bug);
    3. the face is actually visible (skin pixels on the head front) there.
    """
    rows = standard_rows(IDLE_FRAMES, WALK_FRAMES)
    failures = []
    for def_id, style_fn in CLASS_STYLES.items():
        cfg = build_cfg(style_fn)
        for facing, frame in _frames_of(rows):
            img = render_frame(cfg, facing, frame)
            for x in range(FRAME_W):
                if img.getpixel((x, 0))[3] or img.getpixel((x, FRAME_H - 1))[3]:
                    failures.append(f"{def_id}/{facing}: opaque pixel on top/bottom border")
                    break
            for y in range(FRAME_H):
                if img.getpixel((0, y))[3] or img.getpixel((FRAME_W - 1, y))[3]:
                    failures.append(f"{def_id}/{facing}: opaque pixel on left/right border")
                    break

        if def_id == "player_vagabond":
            continue  # hood legitimately covers the head on some facings
        for facing in ("s", "e"):
            frame = IDLE_FRAMES[0]
            pts = _sample_head_front(cfg, facing, frame, def_id)
            torso = render_frame(cfg, facing, frame, layer_region="torso")
            base = render_frame(cfg, facing, frame)
            covered = sum(1 for p in pts if torso.getpixel(p)[3] != 0)
            if covered:
                failures.append(
                    f"{def_id}/{facing}: torso layer covers {covered} head-front samples"
                )
            # Face visibility: the eye whites must survive whatever accessories
            # (circlet, hat brim…) the class stacks around the head.
            xs = [p[0] for p in pts]
            ys = [p[1] for p in pts]
            eye_px = sum(
                1
                for x in range(min(xs) - 2, max(xs) + 3)
                for y in range(min(ys) - 2, max(ys) + 3)
                if 0 <= x < FRAME_W and 0 <= y < FRAME_H
                and base.getpixel((x, y)) == EYE_WHITE
            )
            if eye_px < 4:
                failures.append(
                    f"{def_id}/{facing}: eyes not visible on head front "
                    f"({eye_px} eye-white px)"
                )
    if failures:
        raise SystemExit("verify() FAILED:\n  " + "\n  ".join(failures))
    print(f"verify() OK — {len(CLASS_STYLES)} classes, {ROWS * COLS} frames each")


# ── Output ────────────────────────────────────────────────────────────────────

def main():
    rows = standard_rows(IDLE_FRAMES, WALK_FRAMES)
    for def_id, style_fn in CLASS_STYLES.items():
        cfg = build_cfg(style_fn)
        out_dir = os.path.join(OUT_ROOT, def_id)
        sheet = assemble(cfg, rows=rows, cols=COLS)
        assert sheet.size == (FRAME_W * COLS, FRAME_H * ROWS), sheet.size
        save(sheet, os.path.join(out_dir, "sheet.png"))
        for key in LAYER_KEYS:
            save(assemble(cfg, rows=rows, cols=COLS, layer_region=key),
                 os.path.join(out_dir, "layers", f"{key}.png"))
        save(render_frame(cfg, "s", IDLE_FRAMES[0]),
             os.path.join(out_dir, "sprite_large.png"))
    verify()


if __name__ == "__main__":
    main()
