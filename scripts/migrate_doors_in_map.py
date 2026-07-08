"""
One-off rewriter: replace legacy flat `wooden_door` placements with the
directional wall-slab doors (`wooden_door_n` / `_s` / `_e` / `_w`, generated
by scripts/gen_door_set.py) wherever the door's side can be inferred from
the adjacent wall objects.

Strategy: for each `wooden_door` at (z, x, y), look at the four cardinal
neighbours among the same map's wall objects:

    east/west neighbour is wall_n or a *_ne / *_nw corner → wooden_door_n
    east/west neighbour is wall_s or a *_se / *_sw corner → wooden_door_s
    north/south neighbour is wall_e or a *_ne / *_se corner → wooden_door_e
    north/south neighbour is wall_w or a *_nw / *_sw corner → wooden_door_w

Doors whose neighbours give no vote or conflicting votes are left as the
legacy `wooden_door` and reported on stderr for a hand-fix in the editor.

Unlike scripts/migrate_walls_in_map.py this does NOT re-dump the YAML —
several of these maps are hand-authored with comments, so we surgically
rewrite only the matched `type: wooden_door` lines and leave everything
else (ids, properties, lever wiring, comments, formatting) byte-for-byte
untouched.

Run from the repo root:

    python3 scripts/migrate_doors_in_map.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import yaml

MAP_PATHS = [
    Path("assets/maps/overworld.yaml"),
    Path("assets/maps/island.yaml"),
    Path("assets/maps/proving_grounds.yaml"),
    Path("assets/maps/starter_cellar.yaml"),
]

# Which side a wall type votes for, split by whether it is a horizontal
# (east/west) or vertical (north/south) neighbour of the door. Corners vote
# with the arm that faces the door.
HORIZ_VOTES = {
    "wall_n": "n",
    "wall_corner_ne": "n",
    "wall_corner_nw": "n",
    "wall_s": "s",
    "wall_corner_se": "s",
    "wall_corner_sw": "s",
}
VERT_VOTES = {
    "wall_e": "e",
    "wall_corner_ne": "e",
    "wall_corner_se": "e",
    "wall_w": "w",
    "wall_corner_nw": "w",
    "wall_corner_sw": "w",
}
WALL_TYPES = set(HORIZ_VOTES) | set(VERT_VOTES)


def placements_of(obj: dict) -> list[tuple[int, int, int]]:
    """Normalize an object's placement (single dict or list) to (z, x, y)s."""
    placement = obj.get("placement")
    if placement is None:
        return []
    entries = placement if isinstance(placement, list) else [placement]
    return [(p.get("z", 0), p["x"], p["y"]) for p in entries]


def classify(door: tuple[int, int, int], walls: dict[tuple[int, int, int], str]) -> str | None:
    """Return 'n'/'s'/'e'/'w' when the neighbouring walls agree on a side."""
    z, x, y = door
    votes = set()
    for nx in (x - 1, x + 1):
        t = walls.get((z, nx, y))
        if t in HORIZ_VOTES:
            votes.add(HORIZ_VOTES[t])
    for ny in (y - 1, y + 1):
        t = walls.get((z, x, ny))
        if t in VERT_VOTES:
            votes.add(VERT_VOTES[t])
    if len(votes) == 1:
        return votes.pop()
    return None


def door_blocks_in_text(raw: str) -> list[tuple[int, tuple[int, int, int] | None]]:
    """Find each `type: wooden_door` line and the (z, x, y) of the placement
    inside the same object block. Returns (line_index, placement) pairs.

    An object block ends at the next line whose indent is <= the `- ` list
    marker's indent and which starts a new list item or key.
    """
    lines = raw.split("\n")
    out: list[tuple[int, tuple[int, int, int] | None]] = []
    type_re = re.compile(r"^(\s*(?:-\s+)?)type:\s*wooden_door\s*$")
    inline_re = re.compile(
        r"placement:\s*\{\s*x:\s*(-?\d+)\s*,\s*y:\s*(-?\d+)\s*(?:,\s*z:\s*(-?\d+)\s*)?\}"
    )
    for i, line in enumerate(lines):
        if not type_re.match(line):
            continue
        indent = len(line) - len(line.lstrip())
        placement: tuple[int, int, int] | None = None
        # Scan the object block around the type line (an `id:`/`placement:`
        # can precede or follow `type:`); stop at the next sibling list item.
        j = i + 1
        block: list[str] = []
        # Look back to the start of this list item.
        k = i
        while k >= 0:
            stripped = lines[k].lstrip()
            block.insert(0, lines[k])
            if stripped.startswith("- ") and (len(lines[k]) - len(stripped)) <= indent:
                break
            k -= 1
        while j < len(lines):
            nxt = lines[j]
            stripped = nxt.lstrip()
            if stripped and (len(nxt) - len(stripped)) < indent:
                break
            if stripped.startswith("- ") and (len(nxt) - len(stripped)) < indent:
                break
            if stripped.startswith("- ") and (len(nxt) - len(stripped)) == indent - 2:
                break
            block.append(nxt)
            j += 1
        text = "\n".join(block)
        m = inline_re.search(text)
        if m:
            placement = (int(m.group(3) or 0), int(m.group(1)), int(m.group(2)))
        else:
            # List-form (`- x:`) or block-form (`x:` under `placement:`)
            # placement; optional trailing `z:`.
            lm = re.search(
                r"placement:\s*\n\s*(?:-\s*)?x:\s*(-?\d+)\s*\n\s*y:\s*(-?\d+)(?:\s*\n\s*z:\s*(-?\d+))?",
                text,
            )
            if lm:
                placement = (int(lm.group(3) or 0), int(lm.group(1)), int(lm.group(2)))
        out.append((i, placement))
    return out


def tile_grid_walls(doc: dict) -> dict[tuple[int, int, int], str]:
    """Walls authored in the `tiles:` glyph grid (row 0 = world y 0). The
    grid is always the ground floor (z=0)."""
    legend = doc.get("legend") or {}
    tiles = doc.get("tiles")
    walls: dict[tuple[int, int, int], str] = {}
    if not isinstance(tiles, str):
        return walls
    for y, row in enumerate(tiles.rstrip("\n").split("\n")):
        for x, ch in enumerate(row):
            t = legend.get(ch)
            if t in WALL_TYPES:
                walls[(0, x, y)] = t
    return walls


def migrate_one(path: Path) -> None:
    raw = path.read_text()
    doc = yaml.safe_load(raw)
    objects = doc.get("objects") or []

    walls: dict[tuple[int, int, int], str] = tile_grid_walls(doc)
    door_positions: list[tuple[int, int, int]] = []
    for obj in objects:
        t = obj.get("type")
        if t in WALL_TYPES:
            for pos in placements_of(obj):
                walls[pos] = t
        elif t == "wooden_door":
            door_positions.extend(placements_of(obj))

    if not door_positions:
        print(f"[{path}] no wooden_door placements")
        return

    lines = raw.split("\n")
    migrated = 0
    skipped: list[tuple[int, int, int]] = []
    for (line_idx, placement) in door_blocks_in_text(raw):
        if placement is None:
            print(f"[{path}] WARN: could not read placement near line {line_idx + 1}",
                  file=sys.stderr)
            continue
        side = classify(placement, walls)
        if side is None:
            skipped.append(placement)
            continue
        lines[line_idx] = lines[line_idx].replace(
            "type: wooden_door", f"type: wooden_door_{side}"
        )
        migrated += 1

    if migrated:
        path.write_text("\n".join(lines))
    print(f"[{path}] migrated {migrated}/{len(door_positions)} door(s)")
    for (z, x, y) in skipped:
        print(
            f"[{path}] WARN: door at z={z} ({x}, {y}) has no/conflicting wall "
            f"neighbours — left as legacy wooden_door, hand-fix in the editor",
            file=sys.stderr,
        )


def main() -> int:
    for path in MAP_PATHS:
        if not path.exists():
            print(f"skip {path}: not found", file=sys.stderr)
            continue
        migrate_one(path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
