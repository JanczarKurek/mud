#!/usr/bin/env python3
"""Generate the `tiles:` block for `assets/maps/overworld.yaml` (180 x 130).

This script **prints to stdout only** — it never touches the map file. Paste the
output over the `tiles: |` block in `assets/maps/overworld.yaml`, or redirect it
to a scratch file and splice it in. (Its predecessor, `gen_overworld_v2_map.py`,
wrote the map in place and silently reverted eight commits of hand authoring;
don't reintroduce that.)

Orientation: emitted row 0 is world **y=0, the SOUTH edge** — the same order the
YAML wants. Everything inside this file is authored NORTH-UP (first line of a
stamp is the highest y) because that is how the map reads on screen; `stamp()`
flips it. Coordinates are always (x, y) in world space, y growing north.

Layering order, later wins: base meadow -> named regions -> the Ember ->
structure stamps -> road/plaza corridors -> spawn-area thinning. Roads are cut
last on purpose: cutting the south road through the cemetery hedge is what
opens the graveyard gate.

Determinism: every scatter draws from its own seeded `random.Random`, so reruns
are byte-identical. Nothing here reads the clock.

Run it from the repo root, inside the nix shell:

    nix-shell --run "python3 scripts/gen_overworld_grid.py"
"""

import random
import re
import sys

W, H = 180, 130

# Legend chars this script emits. Must stay a subset of `legend:` in the map.
#   P pine  T oak  W water  f fence  h hedge  o stone  # cave_wall
#   * crystal_cluster  , flowers  B berry_bush  x tombstone
#   S/N/E/V walls  a/b/c/d wall corners
EMPTY = "."

grid = [[EMPTY] * W for _ in range(H)]


def fill(x0, y0, x1, y1, ch):
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            grid[y][x] = ch


def clear(x0, y0, x1, y1):
    fill(x0, y0, x1, y1, EMPTY)


def scatter(x0, y0, x1, y1, weights, density, seed):
    """Sprinkle weighted chars over a rect. `weights` is {char: relative weight}."""
    rng = random.Random(seed)
    chars = list(weights)
    cum = list(weights.values())
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            if rng.random() < density:
                grid[y][x] = rng.choices(chars, weights=cum)[0]


def thin(x0, y0, x1, y1, density, seed):
    """Clear a rect, then re-sprinkle it lightly. Used to keep spawn areas walkable."""
    clear(x0, y0, x1, y1)
    scatter(x0, y0, x1, y1, {"T": 3, ",": 3, "o": 2, "B": 2}, density, seed)


def stamp(art, x0, y0):
    """Paste a NORTH-UP block of text with its SW corner at (x0, y0)."""
    rows = [r for r in art.strip("\n").split("\n")]
    width = len(rows[0])
    assert all(len(r) == width for r in rows), "stamp rows must be equal length"
    for i, row in enumerate(reversed(rows)):
        y = y0 + i
        for j, ch in enumerate(row):
            grid[y][x0 + j] = ch


# --------------------------------------------------------------------------
# Terrain
# --------------------------------------------------------------------------

FOREST = {"P": 60, "T": 34, "B": 6}
ROCK = {"#": 50, "o": 36, "*": 4, "P": 10}
MEADOW = {"T": 26, ",": 34, "B": 14, "o": 14, "P": 12}
MOOR = {"o": 50, "P": 20, "T": 20, ",": 10}

# Base: a thin dusting everywhere so no region reads as a bald rectangle.
scatter(0, 0, W - 1, H - 1, MEADOW, 0.020, seed=1001)

# Borders. Forest walls the valley on three sides; the east is the cliff face
# of the Cinder Hills massif (the old 70x50 map did the same with one column).
scatter(0, 0, W - 1, 2, FOREST, 0.45, seed=1002)          # south
scatter(0, H - 3, W - 1, H - 1, FOREST, 0.45, seed=1003)  # north
scatter(0, 0, 2, H - 1, FOREST, 0.45, seed=1004)          # west
fill(W - 1, 0, W - 1, H - 1, "#")                         # east cliff face

# Cinder Hills (east). Kept sparse so the fire-elemental glade and the cyclops
# lair stay pathable for A* — dense rock here strands the spawns.
scatter(126, 3, W - 2, 126, ROCK, 0.11, seed=1010)

# Thornwood (north) and the deeper ogre wood in its north-west corner.
scatter(3, 96, 125, 126, FOREST, 0.20, seed=1020)
scatter(3, 96, 36, 126, FOREST, 0.24, seed=1021)
# Where the wood meets the hills, over the goblin camp.
scatter(126, 96, W - 2, 126, {"P": 40, "T": 20, "#": 25, "o": 15}, 0.16, seed=1022)

# Emberbrook Fields (west of the Ember) and the pasture that runs north of them.
scatter(3, 36, 52, 72, {"T": 40, ",": 30, "B": 20, "P": 10}, 0.05, seed=1030)
scatter(34, 64, 58, 94, MEADOW, 0.05, seed=1031)

# The south-west moor: the Hollow Bell trailhead country.
scatter(3, 4, 50, 34, MOOR, 0.07, seed=1040)

# The three open approaches to the village. These are the buffers that keep the
# Watch and the wilderness apart, so they stay deliberately sparse.
scatter(58, 4, 125, 52, MEADOW, 0.035, seed=1050)   # south, toward the Old Yard
scatter(40, 76, 125, 95, MEADOW, 0.040, seed=1051)  # north, toward Thornwood
scatter(104, 53, 125, 95, MEADOW, 0.040, seed=1052) # east, toward the hills

# The Ember. Three columns the full height of the map; the bridge gap is cut
# with the roads below.
fill(55, 0, 57, H - 1, "W")

# --------------------------------------------------------------------------
# Structures
#
# The village, cemetery and orchard blocks are lifted verbatim from the 70x50
# map so the hand-authored geometry survives the move. Village pieces are the
# old coordinates + (55, 40); the Old Yard is + (57, 6).
# --------------------------------------------------------------------------

CEMETERY = """
hhhhhhhhhh..hhhhhhhhh
h...................h
h....x..x..x........h
h.............cN.NNdh
h.x..x..x.....V....Eh
h.............V....Eh
h.x..x........V....Eh
h.............aSSSSbh
hhhhhhhhhhhhhhhhhhhhh
"""
stamp(CEMETERY, 79, 8)  # hedge x79..99 y8..16, crypt x93..98 y9..13

STOREHOUSE = """
cNN.NNd
V.....E
V.....E
V.....E
aSSSSSb
"""
stamp(STOREHOUSE, 79, 56)

GENERAL_STORE = """
cNNNNNd
V.....E
V.....E
V.....E
aSS.SSb
"""
stamp(GENERAL_STORE, 82, 68)

TAVERN = """
cNNNNNNd
V......E
V......E
V......E
V......E
V......E
aS.SSSSb
"""
stamp(TAVERN, 93, 66)

ORCHARD = """
fffffffffff
f.........f
f.T.TT..T.f
f.....T....
f...T..B...
f.T..TT.TTf
f.........f
fffffffffff
"""
stamp(ORCHARD, 14, 72)

# Roadside farmstead on the north approach — a landmark that makes the walk to
# Thornwood read as distance rather than emptiness. Gate faces the road.
FARMSTEAD = """
fffffffffff
f.........f
f.........f
f..........
f.........f
f...T.....f
fffffffffff
"""
stamp(FARMSTEAD, 76, 77)

# Ruined watchtower on the south approach to the Old Yard.
WATCHTOWER = """
cNNd
V..E
V..E
aS.b
"""
stamp(WATCHTOWER, 84, 25)  # x84..87, hard against the south road at x88

# The Thornwood boys' stockade.
STOCKADE = """
fffffffffffffffffffff
f...................f
f...................f
f...................f
f...................f
f...................f
f...................f
f...................f
f...................f
f...................f
f...................f
f...................f
ffffffffff.ffffffffff
"""
stamp(STOCKADE, 144, 103)

# --------------------------------------------------------------------------
# Spawn areas — thinned so `pick_spawn_tile` finds free ground and A* has room.
# Keys mirror the `spawn_groups[].area.bounds` in the map YAML. Runs before the
# corridors so a spawn rect overlapping a road can't re-sprinkle it shut;
# `village_guards` is absent on purpose — the village bowl is cleared below.
# --------------------------------------------------------------------------

SPAWN_AREAS = {
    "field_rats": (12, 44, 34, 62),
    "pasture_sheep": (38, 72, 50, 82),
    "pasture_wolves": (28, 84, 44, 92),
    "thornwood_goblins": (66, 104, 114, 116),
    "camp_goblins": (146, 105, 162, 113),
    "camp_mage": (150, 107, 158, 111),
    # `yard_skeletons` (80,9 -> 91,15) is deliberately absent: the CEMETERY
    # stamp is already open ground dotted with graves, and thinning it would
    # scrub out the crypt walls that sit inside the same rect.
    "glade_elemental": (142, 56, 154, 74),
    "hill_cyclops": (152, 12, 170, 24),
    "wood_ogre": (10, 106, 26, 120),
}
for i, (name, (x0, y0, x1, y1)) in enumerate(sorted(SPAWN_AREAS.items())):
    thin(x0, y0, x1, y1, 0.05, seed=2000 + i)

# --------------------------------------------------------------------------
# Corridors — cut last so they win over terrain, stamps and scatter alike.
# --------------------------------------------------------------------------

clear(76, 54, 103, 75)   # the village bowl; buildings are re-stamped below
clear(54, 64, 58, 66)    # Ember Bridge (cuts the river)
clear(59, 64, 75, 66)    # west road: bridge -> village
clear(104, 63, 119, 66)  # east road: village -> Proving Grounds arch
clear(88, 76, 92, 104)   # north road: village -> Thornwood
clear(88, 16, 92, 54)    # south road: village -> Old Yard (cuts the hedge gate)
clear(153, 96, 155, 103) # camp track: the stockade gate out into the wood
clear(27, 108, 48, 109)  # game trail east out of the ogre den
clear(36, 64, 54, 66)    # west-bank road: moor spur -> bridge
clear(34, 24, 36, 66)    # moor track running south down the west bank
clear(16, 22, 36, 24)    # trailhead spur -> the Hollow Bell cage lift

# Re-stamp the village buildings: `clear(76, 54, ...)` above wiped them.
stamp(STOREHOUSE, 79, 56)
stamp(GENERAL_STORE, 82, 68)
stamp(TAVERN, 93, 66)

# Portal tiles must never be blocked. Clear the tile itself only — the cellar
# hatch sits inside the tavern, and a 3x3 clear there eats the building's NE
# corner. The other three already stand on cleared road/plaza corridors; only
# the sinkhole needs elbow room carved out of the Cinder Hills scatter.
for px, py in [(99, 71), (150, 50), (119, 65), (16, 22)]:
    clear(px, py, px, py)
clear(148, 48, 152, 52)  # sinkhole clearing

# `tests/combat_scoping.rs` teleports a peer to (2, 2); keep it out of a tree.
clear(1, 1, 3, 3)

# Every hand-placed object in the map's `objects:` block needs bare ground: a
# scattered pine landing on the same tile as a signpost stacks two colliders
# and quietly walls the tile off. Read the placements straight out of the YAML
# (plain-text regex — PyYAML is not in shell.nix) so objects added later are
# handled by the next regeneration without anyone having to remember this.
# The pattern only matches two-key `{ x: N, y: N }` mappings, so `floors:`
# rects (which carry w/h) and patrol waypoints (which carry `dwell`) are
# untouched; the slice starts at `objects:` so the tile grid itself is never
# scanned.
_map_text = open("assets/maps/overworld.yaml").read()
_objects = _map_text[_map_text.index("\nobjects:"):]
for _mx, _my in re.findall(r"\{ x: (\d+), y: (\d+) \}", _objects):
    clear(int(_mx), int(_my), int(_mx), int(_my))

# --------------------------------------------------------------------------
# Checks
# --------------------------------------------------------------------------

rows = ["".join(r) for r in grid]
assert len(rows) == H, f"{len(rows)} rows, expected {H}"
for y, row in enumerate(rows):
    assert len(row) == W, f"row y={y} is {len(row)} chars, expected {W}"

# The plaza centre is the player spawn (`find_spawn_location` uses w/2, h/2).
assert rows[65][90] == EMPTY, "spawn tile (90,65) must be free"

# The plaza is clipped around the tavern (x93.., y66..) and the general store
# (y68..) so its cobblestone rect stays disjoint from theirs — `floors:` is a
# HashMap, so overlapping rects resolve in nondeterministic order (ISSUES.md).
MUST_BE_CLEAR = [
    ("plaza", 85, 62, 95, 65),
    ("plaza apron", 85, 66, 92, 67),
    ("west road", 59, 64, 75, 66),
    ("bridge", 54, 64, 58, 66),
    ("east road", 104, 63, 119, 66),
    ("north road", 88, 76, 92, 104),
    ("south road", 88, 16, 92, 54),
    ("west-bank road", 36, 64, 54, 66),
    ("moor track", 34, 24, 36, 66),
    ("trailhead spur", 16, 22, 36, 24),
]
for name, x0, y0, x1, y1 in MUST_BE_CLEAR:
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            assert grid[y][x] == EMPTY, f"{name} blocked at ({x},{y}) by {grid[y][x]!r}"

for px, py in [(99, 71), (150, 50), (119, 65), (16, 22)]:
    assert grid[py][px] == EMPTY, f"portal tile ({px},{py}) is blocked"

# Stamp integrity. A corridor cut or a portal clear that overruns a building
# silently eats its walls, which is invisible in the emitted text — check the
# four corners and the doorway of every structure instead.
STRUCTURES = {
    #  name:            (sw, se, nw, ne)          door
    "storehouse":       ((79, 56), (85, 56), (79, 60), (85, 60), (82, 60)),
    "general store":    ((82, 68), (88, 68), (82, 72), (88, 72), (85, 68)),
    "tavern":           ((93, 66), (100, 66), (93, 72), (100, 72), (95, 66)),
    "watchtower":       ((84, 25), (87, 25), (84, 28), (87, 28), (86, 25)),
    "crypt":            ((93, 9), (98, 9), (93, 13), (98, 13), (95, 13)),
}
for name, (sw, se, nw, ne, door) in STRUCTURES.items():
    for (cx, cy), want in ((sw, "a"), (se, "b"), (nw, "c"), (ne, "d")):
        assert grid[cy][cx] == want, (
            f"{name} corner ({cx},{cy}) is {grid[cy][cx]!r}, expected {want!r}"
        )
    dx, dy = door
    assert grid[dy][dx] == EMPTY, f"{name} doorway ({dx},{dy}) is blocked"

for hx, hy in [(79, 8), (99, 8), (79, 16), (99, 16)]:
    assert grid[hy][hx] == "h", f"Old Yard hedge corner ({hx},{hy}) was clobbered"

LEGEND = set("PTWfhox#*,BSNEVabcd") | {EMPTY}
for y, row in enumerate(rows):
    for x, ch in enumerate(row):
        assert ch in LEGEND, f"unmapped char {ch!r} at ({x},{y})"

# Reachability. Every region has to be walkable *from the plaza*, or a road cut
# in the wrong place quietly strands a whole zone behind the river or a hedge.
# Walkable glyphs: bare ground, flowers (`extends: pickup`) and tombstones
# (`colliding: false`). Everything else in the legend blocks.
WALKABLE = {EMPTY, ",", "x"}


def reachable_from(sx, sy):
    seen = [[False] * W for _ in range(H)]
    seen[sy][sx] = True
    stack = [(sx, sy)]
    while stack:
        x, y = stack.pop()
        for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if 0 <= nx < W and 0 <= ny < H and not seen[ny][nx]:
                if grid[ny][nx] in WALKABLE:
                    seen[ny][nx] = True
                    stack.append((nx, ny))
    return seen


walkable_from_plaza = reachable_from(90, 65)

for px, py in [(99, 71), (150, 50), (119, 65), (16, 22)]:
    assert walkable_from_plaza[py][px], f"portal ({px},{py}) is unreachable from the plaza"

REACH_TARGETS = dict(SPAWN_AREAS)
REACH_TARGETS["yard_skeletons"] = (80, 9, 91, 15)
for name, (x0, y0, x1, y1) in sorted(REACH_TARGETS.items()):
    tiles = [(x, y) for y in range(y0, y1 + 1) for x in range(x0, x1 + 1)]
    open_tiles = [t for t in tiles if grid[t[1]][t[0]] in WALKABLE]
    live = [t for t in open_tiles if walkable_from_plaza[t[1]][t[0]]]
    assert open_tiles, f"{name} has no open tile at all"
    ratio = len(live) / len(open_tiles)
    assert ratio > 0.8, (
        f"{name}: only {ratio:.0%} of its open tiles connect to the plaza"
    )

occupied = sum(1 for r in rows for c in r if c != EMPTY)
print(
    f"# {W}x{H} = {W * H} tiles, {occupied} legend objects "
    f"({occupied / (W * H):.1%} occupancy)",
    file=sys.stderr,
)

for row in rows:  # row 0 first == y=0 == the SOUTH edge
    print("  " + row)
