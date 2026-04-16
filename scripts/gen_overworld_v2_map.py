"""
Generates assets/maps/overworld_v2.yaml
70×50 tile overworld showcasing all game features.

Zone layout (approximate):
  Village     x:1-21,  y:1-22   — buildings, villagers, well, campfire, signs
  Meadow      x:23-43, y:1-24   — farm paddock, flowers, portal to cellar
  Forest      x:45-68, y:1-24   — dense trees, hidden loot, forest goblin
  Lake        x:1-27,  y:27-48  — water + sand shore
  Central     x:28-44, y:25-48  — dirt road, dungeon entrance
  Goblin camp x:45-68, y:27-48  — 3 hostile goblins, loot barrel
"""

import textwrap
import os

W, H = 70, 50
OUT_PATH = "assets/maps/overworld.yaml"

# Explicit IDs are offset by 10000 so that anonymous tile objects
# (which get IDs from max_explicit+1 upward, i.e. 10000+N+1) never
# collide with starter_cellar.yaml's 1200-1202 or underworld.yaml's 2000-2007.

# ── Grid initialisation ────────────────────────────────────────────────────────
grid = [['.' for _ in range(W)] for _ in range(H)]

def set_cell(x, y, char):
    if 0 <= x < W and 0 <= y < H:
        grid[y][x] = char

def row_fill(y, x0, x1, char):
    for x in range(x0, x1 + 1):
        set_cell(x, y, char)

def col_fill(x, y0, y1, char):
    for y in range(y0, y1 + 1):
        set_cell(x, y, char)

def rect_fill(x, y, w, h, char):
    for dy in range(h):
        for dx in range(w):
            set_cell(x + dx, y + dy, char)

def rect_border(x, y, w, h, char, open_side=None, open_pos=None):
    """Draw hollow rectangle border. open_side: 't','b','l','r'; open_pos: coord along that side."""
    for dx in range(w):
        if not (open_side == 't' and x + dx == open_pos):
            set_cell(x + dx, y, char)
        if not (open_side == 'b' and x + dx == open_pos):
            set_cell(x + dx, y + h - 1, char)
    for dy in range(1, h - 1):
        if not (open_side == 'l' and y + dy == open_pos):
            set_cell(x, y + dy, char)
        if not (open_side == 'r' and y + dy == open_pos):
            set_cell(x + w - 1, y + dy, char)

# ─────────────────────────────────────────────────────────────────────────────
# ZONE A — VILLAGE
# ─────────────────────────────────────────────────────────────────────────────

# Cobblestone village square (behind/between buildings)
rect_fill(8, 3, 3, 15, 'c')   # street x:8-10 between inn and market
rect_fill(11, 7, 7, 6, 'c')   # market square x:11-17, y:7-12

# Inn (x:2-7, y:2-7) — 6 wide, 6 tall — entrance at bottom centre (x=4)
rect_border(2, 2, 6, 6, '#', open_side='b', open_pos=5)

# Market stall (x:11-17, y:2-6) — 7 wide, 5 tall — entrance at bottom (x=14)
rect_border(11, 2, 7, 5, '#', open_side='b', open_pos=14)

# Storage hut (x:2-7, y:13-18) — 6 wide, 6 tall — entrance at bottom (x=4)
rect_border(2, 13, 6, 6, '#', open_side='b', open_pos=4)

# Village flowers
for (fx, fy) in [(3,10),(4,10),(15,10),(16,11),(7,20),(8,20),(18,13),(19,14)]:
    if grid[fy][fx] == '.':
        set_cell(fx, fy, 'f')

# ─────────────────────────────────────────────────────────────────────────────
# ZONE B — MEADOW
# ─────────────────────────────────────────────────────────────────────────────

# Farm paddock fences (x:26-33, y:4-12) — gap entrance at bottom-right (x=33)
rect_border(26, 4, 8, 9, '=', open_side='b', open_pos=33)

# Meadow flowers
for (fx, fy) in [(24,5),(25,5),(38,4),(40,6),(25,15),(36,18),(38,20),(28,4),(30,8),(31,9),(39,13),(41,16)]:
    if grid[fy][fx] == '.':
        set_cell(fx, fy, 'f')

# Flowers inside farm paddock
for (fx, fy) in [(28,6),(29,7),(30,5),(31,8),(32,6)]:
    if grid[fy][fx] == '.':
        set_cell(fx, fy, 'f')

# ─────────────────────────────────────────────────────────────────────────────
# ZONE C — FOREST (dense trees with clearings)
# ─────────────────────────────────────────────────────────────────────────────

import random
rng = random.Random(42)  # deterministic

for y in range(1, 25):
    for x in range(45, 69):
        # Irregular tree placement (~78% density)
        if rng.random() < 0.78:
            set_cell(x, y, 'T')

# Clear corridors for navigability (horizontal path through forest)
for x in range(45, 69):
    set_cell(x, 12, '.')   # horizontal clearing at y=12
    set_cell(x, 13, '.')

# Clear small areas around hidden loot positions
for (cx, cy, r) in [(52,6,1),(60,14,1),(64,10,2),(58,20,1)]:
    for dy in range(-r, r+1):
        for dx in range(-r, r+1):
            set_cell(cx+dx, cy+dy, '.')

# Forest edge (column 45) stays sparse to look like tree-line
for y in range(1, 25):
    if rng.random() < 0.5:
        set_cell(45, y, 'T')

# ─────────────────────────────────────────────────────────────────────────────
# ZONE D — LAKE (water + sand shore)
# ─────────────────────────────────────────────────────────────────────────────

# Water body: x:4-24, y:30-46
rect_fill(4, 30, 21, 17, '~')

# Sand shore ring around water
# Top shore: y:28-29, x:2-26
rect_fill(2, 28, 25, 2, 'a')
# Bottom shore: y:47-48, x:2-26
rect_fill(2, 47, 25, 2, 'a')
# Left shore: y:30-46, x:2-3
rect_fill(2, 30, 2, 17, 'a')
# Right shore: y:30-46, x:25-26
rect_fill(25, 30, 2, 17, 'a')

# Rocky bits on shore
for (sx, sy) in [(2,29),(3,28),(5,27),(23,28),(26,30),(2,44),(25,45),(3,47)]:
    if 0 <= sx < W and 0 <= sy < H:
        set_cell(sx, sy, 's')

# Shore flowers (reeds)
for (fx, fy) in [(2,27),(4,27),(7,27),(13,27),(20,27),(25,27),(1,34),(1,39),(27,33),(27,40)]:
    if 0 <= fx < W and 0 <= fy < H and grid[fy][fx] == '.':
        set_cell(fx, fy, 'f')

# ─────────────────────────────────────────────────────────────────────────────
# ZONE E — CENTRAL APPROACH (dirt road + dungeon entrance)
# ─────────────────────────────────────────────────────────────────────────────

# Dirt road from village south exit (x:5, y:19) to dungeon (x:35, y:45)
# South from village: x:5, y:19-25
for y in range(19, 26):
    set_cell(5, y, 'd')
# East connector at y:26: x:5 to x:34
for x in range(5, 35):
    set_cell(x, 26, 'd')
# South to dungeon: x:35, y:27 to y:44
for y in range(27, 45):
    set_cell(35, y, 'd')

# Stones scattered around dungeon entrance
for (sx, sy) in [(33,44),(34,44),(36,44),(37,44),(33,46),(37,46),(32,47),(38,47)]:
    set_cell(sx, sy, 's')

# Scattered flowers in central zone
for (fx, fy) in [(30,27),(31,28),(38,27),(40,29),(29,35),(42,33),(28,40),(43,41)]:
    if grid[fy][fx] == '.':
        set_cell(fx, fy, 'f')

# ─────────────────────────────────────────────────────────────────────────────
# ZONE F — GOBLIN CAMP
# ─────────────────────────────────────────────────────────────────────────────

# Rough camp perimeter walls
camp_walls = [
    # North barrier with gap
    (46,28),(47,28),(48,28),(49,28),(50,28),
    (53,28),(54,28),(55,28),(56,28),(57,28),(58,28),
    (61,28),(62,28),(63,28),(64,28),(65,28),
    # West side
    (46,29),(46,30),(46,31),(46,32),(46,33),
    # East side
    (67,29),(67,30),(67,31),(67,32),(67,33),
    # South barrier with gap
    (47,48),(48,48),(49,48),
    (52,48),(53,48),(54,48),(55,48),(56,48),
    (59,48),(60,48),(61,48),(62,48),
]
for (wx, wy) in camp_walls:
    set_cell(wx, wy, '#')

# Debris stones around camp
for (sx, sy) in [(48,32),(50,30),(63,35),(65,43),(48,46),(60,43),(52,44)]:
    if grid[sy][sx] == '.':
        set_cell(sx, sy, 's')

# ─────────────────────────────────────────────────────────────────────────────
# Validate all rows are exactly W chars
# ─────────────────────────────────────────────────────────────────────────────
for y, row in enumerate(grid):
    assert len(row) == W, f"Row {y} has {len(row)} chars"

tile_lines = [''.join(row) for row in grid]
tiles_block = '\n'.join('  ' + line for line in tile_lines)

# ─────────────────────────────────────────────────────────────────────────────
# YAML ASSEMBLY
# ─────────────────────────────────────────────────────────────────────────────

yaml_content = f"""\
authored_id: overworld
permanence: persistent
width: 70
height: 50
fill_object_type: grass

portals:
  - id: starter_cellar_entrance
    source: {{x: 35, y: 20}}
    destination_space_id: starter_cellar
    destination_tile: {{x: 6, y: 1}}
    destination_permanence: ephemeral
  - id: underworld_sinkhole
    source: {{x: 35, y: 46}}
    destination_space_id: underworld
    destination_tile: {{x: 3, y: 14}}

legend:
  "#": wall
  "~": water
  "T": tree
  "=": fence
  "f": flowers
  "s": stone
  "c": cobblestone
  "d": dirt_path
  "a": sand

tiles: |
{tiles_block}

objects:
  # IDs are in the 10000+ range so that anonymous tile objects (which get
  # IDs from max_explicit+1 = 11000 upward) never collide with
  # starter_cellar (1200-1202) or underworld (2000-2007).

  # ── VILLAGE ZONE ────────────────────────────────────────────────────────

  # Inn barrel with potion + apple
  - id: 10100
    type: barrel
    placement: {{x: 4, y: 4}}
    contents: [10101, 10102]
  - id: 10101
    type: potion
  - id: 10102
    type: apple

  # Market — equipment on ground
  - id: 10110
    type: bronze_sword
    placement: {{x: 13, y: 3}}
  - id: 10111
    type: leather_armor
    placement: {{x: 15, y: 3}}

  # Storage hut — gear
  - id: 10120
    type: canvas_backpack
    placement: {{x: 4, y: 15}}
  - id: 10121
    type: leather_helmet
    placement: {{x: 6, y: 15}}

  # Village well
  - id: 10130
    type: well
    placement: {{x: 10, y: 9}}

  # Village campfire
  - id: 10131
    type: campfire
    placement: {{x: 10, y: 11}}

  # Village south sign
  - id: 10132
    type: sign_post
    placement: {{x: 5, y: 21}}
    properties:
      text: "South road leads to the old dungeon. Adventurers beware!"

  # Villagers
  - id: 10140
    type: villager
    placement: {{x: 9, y: 8}}
    behavior:
      kind: roam
      step_interval_seconds: 1.2
      bounds: {{min_x: 7, min_y: 7, max_x: 14, max_y: 13}}
  - id: 10141
    type: villager
    placement: {{x: 14, y: 12}}
    behavior:
      kind: roam
      step_interval_seconds: 1.0
      bounds: {{min_x: 11, min_y: 9, max_x: 18, max_y: 15}}

  # Player spawn
  - id: 10999
    type: player
    placement: {{x: 10, y: 18}}

  # ── MEADOW ZONE ─────────────────────────────────────────────────────────

  # Cellar portal sign
  - id: 10150
    type: sign_post
    placement: {{x: 34, y: 19}}
    properties:
      text: "Step here to enter the cellar."

  # Scattered meadow loot
  - id: 10151
    type: silver_ring
    placement: {{x: 38, y: 8}}
  - id: 10152
    type: potion
    placement: {{x: 36, y: 14}}
  - id: 10153
    type: scroll
    placement: {{x: 40, y: 16}}
    properties:
      spell_id: lesser_heal

  # ── FOREST ZONE ─────────────────────────────────────────────────────────

  - id: 10160
    type: apple
    placement: {{x: 52, y: 6}}
  - id: 10161
    type: apple
    placement: {{x: 60, y: 14}}
  - id: 10162
    type: copper_amulet
    placement: {{x: 64, y: 10}}
  - id: 10163
    type: leather_legs
    placement: {{x: 58, y: 20}}

  # Forest goblin
  - id: 10164
    type: goblin
    placement: {{x: 48, y: 10}}
    behavior:
      kind: roam_and_chase
      step_interval_seconds: 0.9
      detect_distance_tiles: 4
      disengage_distance_tiles: 6
      bounds: {{min_x: 46, min_y: 5, max_x: 55, max_y: 18}}

  # ── DUNGEON APPROACH ────────────────────────────────────────────────────

  # Sinkhole (portal trigger object)
  - id: 10170
    type: sinkhole
    placement: {{x: 35, y: 46}}

  # Portal arch visual
  - id: 10171
    type: portal_arch
    placement: {{x: 35, y: 44}}

  # Dungeon warning sign
  - id: 10172
    type: sign_post
    placement: {{x: 33, y: 43}}
    properties:
      text: "The sinkhole ahead leads deep underground. Return is not guaranteed."

  # Abandoned campfire near dungeon
  - id: 10173
    type: campfire
    placement: {{x: 32, y: 41}}

  # ── GOBLIN CAMP ─────────────────────────────────────────────────────────

  # Camp campfire
  - id: 10180
    type: campfire
    placement: {{x: 56, y: 38}}

  # Loot barrel
  - id: 10181
    type: barrel
    placement: {{x: 55, y: 36}}
    contents: [10182, 10183, 10184]
  - id: 10182
    type: potion
  - id: 10183
    type: scroll
    properties:
      spell_id: spark_bolt
  - id: 10184
    type: potion

  # Loot on ground
  - id: 10185
    type: bronze_sword
    placement: {{x: 60, y: 33}}
  - id: 10186
    type: leather_legs
    placement: {{x: 62, y: 40}}
  - id: 10187
    type: traveler_boots
    placement: {{x: 50, y: 42}}
  - id: 10188
    type: wooden_shield
    placement: {{x: 58, y: 42}}

  # Goblin NPCs
  - id: 10190
    type: goblin
    placement: {{x: 52, y: 34}}
    behavior:
      kind: roam_and_chase
      step_interval_seconds: 0.85
      detect_distance_tiles: 5
      disengage_distance_tiles: 8
      bounds: {{min_x: 46, min_y: 29, max_x: 60, max_y: 42}}
  - id: 10191
    type: goblin
    placement: {{x: 61, y: 38}}
    behavior:
      kind: roam_and_chase
      step_interval_seconds: 0.90
      detect_distance_tiles: 5
      disengage_distance_tiles: 7
      bounds: {{min_x: 50, min_y: 29, max_x: 67, max_y: 46}}
  - id: 10192
    type: goblin
    placement: {{x: 54, y: 44}}
    behavior:
      kind: roam_and_chase
      step_interval_seconds: 1.0
      detect_distance_tiles: 4
      disengage_distance_tiles: 6
      bounds: {{min_x: 46, min_y: 38, max_x: 64, max_y: 48}}
"""

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
with open(OUT_PATH, 'w') as f:
    f.write(yaml_content)

print(f"Saved {OUT_PATH}")
print(f"Grid size: {W}×{H}")
# Count object types placed via legend
from collections import Counter
counts = Counter(c for row in grid for c in row)
print("Legend char counts:", dict(sorted(counts.items())))
