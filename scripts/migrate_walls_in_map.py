"""
One-off rewriter for `assets/maps/overworld.yaml`. Replaces every legacy
`wall` and `side_wall` placement with one of the new directional ids
(`wall_n` / `wall_s` / `wall_e` / `wall_w`) or, on a building's corner tile,
the matching `wall_corner_*`.

Strategy: for each legacy wall tile, look at its 4 cardinal neighbours
(restricted to the same legacy cluster, found via 4-connected flood-fill):

    has_s & has_w & !has_n & !has_e → wall_corner_ne
    has_s & has_e & !has_n & !has_w → wall_corner_nw
    has_n & has_w & !has_s & !has_e → wall_corner_se
    has_n & has_e & !has_s & !has_w → wall_corner_sw
    has_e & has_w & !has_n & !has_s → horizontal: wall_s if y == y_min(cluster) else wall_n
    has_n & has_s & !has_e & !has_w → vertical:   wall_w if x == x_min(cluster) else wall_e

Tiles that don't match any pattern (T-joints, lone tiles, 1×1 clusters,
ambiguous strip endpoints) fall back to the closest direction implied by
their original legacy id (`wall` → `wall_s`, `side_wall` → `wall_w`) and
are reported on stderr so the user can hand-fix them in the editor.

Run from the repo root:

    ~/.venv/mud/bin/python scripts/migrate_walls_in_map.py
"""

from __future__ import annotations

import sys
from collections import defaultdict
from pathlib import Path

import yaml


MAP_PATHS = [
    Path("assets/maps/overworld.yaml"),
    Path("assets/maps/dupa4.yaml"),
]
LEGACY_TYPES = {"wall", "side_wall"}
NEW_TYPES = ("wall_n", "wall_s", "wall_e", "wall_w",
             "wall_corner_ne", "wall_corner_nw",
             "wall_corner_se", "wall_corner_sw")


def main() -> int:
    for path in MAP_PATHS:
        if not path.exists():
            print(f"skip {path}: not found", file=sys.stderr)
            continue
        migrate_one(path)
    return 0


def migrate_one(map_path: Path) -> None:
    raw = map_path.read_text()
    doc = yaml.safe_load(raw)
    objects = doc.get("objects") or []

    # ── 1. Pull every legacy wall placement out ──────────────────────────
    # Tag each tile with its original legacy id so we can fall back on it
    # when neighbour-based classification is ambiguous.
    legacy_tiles: list[tuple[int, int, int, str]] = []  # (z, x, y, orig_type)
    for obj in objects:
        if obj["type"] in LEGACY_TYPES:
            for p in obj.get("placement", []):
                legacy_tiles.append((p.get("z", 0), p["x"], p["y"], obj["type"]))
    # Drop the legacy blocks entirely; new ones get rebuilt below.
    objects = [obj for obj in objects if obj["type"] not in LEGACY_TYPES]

    # ── 2. Flood-fill into clusters by floor ─────────────────────────────
    by_z_set: dict[int, set[tuple[int, int]]] = defaultdict(set)
    orig_type: dict[tuple[int, int, int], str] = {}
    for z, x, y, t in legacy_tiles:
        by_z_set[z].add((x, y))
        orig_type[(z, x, y)] = t

    classified: dict[str, list[tuple[int, int, int]]] = defaultdict(list)
    unmatched: list[tuple[int, int, int, str]] = []

    for z, tiles in by_z_set.items():
        clusters = _flood_clusters(tiles)
        for cluster in clusters:
            cluster_set = set(cluster)
            xs = [p[0] for p in cluster]
            ys = [p[1] for p in cluster]
            x_min, x_max = min(xs), max(xs)
            y_min, y_max = min(ys), max(ys)
            for (x, y) in cluster:
                has_n = (x, y + 1) in cluster_set
                has_s = (x, y - 1) in cluster_set
                has_e = (x + 1, y) in cluster_set
                has_w = (x - 1, y) in cluster_set
                kind: str | None = None
                # Corners: exactly two perpendicular neighbours present, the
                # other two are the building's outside.
                if has_s and has_w and not has_n and not has_e:
                    kind = "wall_corner_ne"
                elif has_s and has_e and not has_n and not has_w:
                    kind = "wall_corner_nw"
                elif has_n and has_w and not has_s and not has_e:
                    kind = "wall_corner_se"
                elif has_n and has_e and not has_s and not has_w:
                    kind = "wall_corner_sw"
                # Straight walls: collinear neighbours; the cluster's bbox
                # edge tells us which side of the building we're on.
                elif has_e and has_w and not has_n and not has_s:
                    kind = "wall_s" if y == y_min else "wall_n"
                elif has_n and has_s and not has_e and not has_w:
                    kind = "wall_w" if x == x_min else "wall_e"
                if kind is None:
                    # Strip endpoints, T-joints, door-adjacent tiles. Fall
                    # back to which cluster-bbox edge(s) this tile sits on —
                    # gives a sane direction for almost everything.
                    on_n = y == y_max
                    on_s = y == y_min
                    on_e = x == x_max
                    on_w = x == x_min
                    if on_n and on_e:
                        kind = "wall_corner_ne"
                    elif on_n and on_w:
                        kind = "wall_corner_nw"
                    elif on_s and on_e:
                        kind = "wall_corner_se"
                    elif on_s and on_w:
                        kind = "wall_corner_sw"
                    elif on_n:
                        kind = "wall_n"
                    elif on_s:
                        kind = "wall_s"
                    elif on_e:
                        kind = "wall_e"
                    elif on_w:
                        kind = "wall_w"
                    else:
                        legacy = orig_type.get((z, x, y), "wall")
                        kind = "wall_s" if legacy == "wall" else "wall_w"
                        unmatched.append((z, x, y, legacy))
                classified[kind].append((z, x, y))

    if unmatched:
        print(
            f"WARN: {len(unmatched)} ambiguous tile(s) (T-joints / lone tiles "
            f"/ strip endpoints) defaulted to wall_s or wall_w by legacy type. "
            f"Hand-fix in the editor if a direction is wrong. First 20:",
            file=sys.stderr,
        )
        for entry in unmatched[:20]:
            print(f"    z={entry[0]} (x, y)=({entry[1]}, {entry[2]})  legacy={entry[3]}",
                  file=sys.stderr)

    # ── 3. Merge into any pre-existing blocks (corners already authored) ─
    existing: dict[str, dict] = {obj["type"]: obj for obj in objects
                                 if obj["type"] in NEW_TYPES}
    for type_id, placements in classified.items():
        seen: set[tuple[int, int, int]] = set()
        target = existing.get(type_id)
        if target is None:
            target = {"type": type_id, "placement": []}
            objects.append(target)
            existing[type_id] = target
        # De-dupe against anything already present.
        for p in target["placement"]:
            seen.add((p.get("z", 0), p["x"], p["y"]))
        for (z, x, y) in placements:
            if (z, x, y) in seen:
                continue
            entry = {"x": x, "y": y}
            if z != 0:
                entry["z"] = z
            target["placement"].append(entry)
            seen.add((z, x, y))
        target["placement"].sort(key=lambda p: (p.get("z", 0), p["y"], p["x"]))

    # ── 4. Re-emit. Keep the top-level structure stable. ────────────────
    objects.sort(key=lambda o: o["type"])
    doc["objects"] = objects
    map_path.write_text(yaml.safe_dump(doc, sort_keys=False, width=120))
    print(
        f"[{map_path}] Migrated {len(legacy_tiles)} legacy tiles -> "
        + ", ".join(f"{k}: {len(v)}" for k, v in sorted(classified.items()))
    )


def _flood_clusters(tiles: set[tuple[int, int]]) -> list[list[tuple[int, int]]]:
    """4-connected flood-fill grouping of `tiles`."""
    seen: set[tuple[int, int]] = set()
    out: list[list[tuple[int, int]]] = []
    for start in tiles:
        if start in seen:
            continue
        stack = [start]
        cluster: list[tuple[int, int]] = []
        while stack:
            (x, y) = stack.pop()
            if (x, y) in seen or (x, y) not in tiles:
                continue
            seen.add((x, y))
            cluster.append((x, y))
            for (dx, dy) in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                stack.append((x + dx, y + dy))
        out.append(cluster)
    return out


if __name__ == "__main__":
    raise SystemExit(main())
