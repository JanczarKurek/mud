# YAML Formats

This document describes the YAML formats currently used by the project.

It should be updated whenever the schema or intended meaning of these files changes.

## Schema files

Machine-readable JSON Schema files live under `assets/schemas/` and are wired to the
asset YAML paths in `.vscode/settings.json`. With the [redhat.vscode-yaml](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml)
extension installed, VS Code provides inline validation and autocomplete for all asset YAML files.

To regenerate schemas after changing any serde struct:

```bash
cargo run --bin gen_schemas --features gen-schemas
```

The generated files are committed alongside the source and should be kept in sync.

## 1. Map Layout YAML

Path:
- `assets/maps/*.yaml`

Current example:
- `assets/maps/overworld.yaml`
- `assets/maps/underworld.yaml`

Purpose:
- Describes one authored space definition.
- Defines the tile dimensions of that space.
- Defines the default fill object type for every tile.
- Defines object instances optionally tagged with a stable symbolic id (string) so other objects can reference them.
- Allows objects to exist on the map, inside containers (either inline-nested or by reference), or nowhere.
- Defines portal links between spaces.

Numeric runtime ids are assigned automatically by the loader; you never write them in YAML.

Top-level fields:

### `authored_id`
- Type: string
- Meaning: stable authored identifier for the space
- This is used by portal destinations and runtime space instancing

### `permanence`
- Type: string
- Allowed values:
  - `persistent`
  - `ephemeral`
- Meaning: default runtime lifetime policy for this space definition

### `width`
- Type: integer
- Meaning: map width in tiles

### `height`
- Type: integer
- Meaning: map height in tiles

### `fill_floor_type`
- Type: string
- Meaning: floor tileset ID that fills every tile of this space's ground floor before any explicit `floors` overlays or `tiles`/`objects` overrides are applied
- This must match a directory name under `assets/floors/` (see [Floor Tileset Metadata YAML](#4-floor-tileset-metadata-yaml))
- Set to the empty string (`''`) to leave tiles unfilled — useful for procedurally generated dungeons that paint their own floors

### `floors`
- Type: mapping from floor tileset id to a placement mapping
- Optional: yes
- Default: empty mapping (only `fill_floor_type` is painted)
- Meaning: per-floor-type overlays applied on top of `fill_floor_type` for the ground floor. Each key must match a directory under `assets/floors/`. The map loader paints the listed tiles/rectangles with that floor type; transition tilesets are looked up automatically wherever two adjacent tiles use different floor types (see [Floor Transition Metadata YAML](#5-floor-transition-metadata-yaml)).
- Each placement mapping has two optional sub-fields, both default-empty:
  - `placement`: list of `{ x, y }` coordinates — single tiles painted with this floor.
  - `rects`: list of `{ x, y, w, h, z? }` rectangles — axis-aligned blocks painted with this floor. `z` defaults to `0`; only `z = 0` (ground floor) is currently rendered.

Example:

```yaml
fill_floor_type: grass
floors:
  cobblestone:
    placement:
      - { x: 12, y: 18 }
      - { x: 12, y: 19 }
    rects:
      - { x: 5, y: 5, w: 4, h: 2 }
  dirt_path:
    placement:
      - { x: 8, y: 8 }
```

### `lighting`
- Type: mapping
- Optional: yes (defaults to outdoor-bright with day/night enabled)
- Meaning: per-space ambient lighting consumed by the client darkness
  overlay (`src/world/darkness.rs`). The overlay draws a single fullscreen
  quad whose color is the ambient tint and whose alpha is "how dark is
  this pixel". Light sources subtract from the alpha to carve visibility
  holes; they never add color. When the curve sets alpha to 0 (daylight),
  light sources are implicitly invisible.
- Subfields:
  - `indoor_ambient`: `[r, g, b]` u8 — base color for tiles whose `(x, y, z+1)`
    has an `occludes_floor_above` object (covered by a roof). Constant;
    not affected by the world clock. Alpha is derived from brightness.
    Default: `[55, 50, 60]`.
  - `outdoor_ambient`: `[r, g, b]` u8 — constant outdoor color used when
    `has_day_night: false`. Ignored when `has_day_night: true` (the curve
    drives both color and alpha in that case). Default: `[220, 220, 230]`.
  - `has_day_night`: bool — when true, outdoor lighting is driven by
    `outdoor_curve`. When false, outdoor uses the constant
    `outdoor_ambient` (caves, dungeons). Default: `true`.
  - `outdoor_curve`: list of keyframes — per-map day/night cycle.
    Optional; empty (the default) means "use the engine's built-in curve",
    which is bright at midday (alpha 0) with deep-blue navigable darkness
    at midnight. Each keyframe has:
    - `time`: f32 in `[0.0, 1.0]` — 0.0 is midnight, 0.5 is noon.
    - `color`: `[r, g, b]` u8 — ambient tint at this time.
    - `alpha`: f32 in `[0.0, 1.0]` — darkness overlay opacity. 0.0 means
      "completely transparent" (lights invisible — that's how daytime
      suppresses torches).
    Values are linearly interpolated; the curve is cyclic (the last
    keyframe wraps back to the first). Keyframes don't need to be sorted
    — they're sorted by `time` at load time.

Example (uniformly dim cave; no day/night):

```yaml
lighting:
  outdoor_ambient: [60, 55, 55]
  indoor_ambient: [50, 45, 45]
  has_day_night: false
```

Example (custom outdoor curve — warm bright noon, brief twilight):

```yaml
lighting:
  indoor_ambient: [55, 50, 60]
  has_day_night: true
  outdoor_curve:
    - { time: 0.0,  color: [40, 50, 100],  alpha: 0.7 }
    - { time: 0.25, color: [40, 50, 100],  alpha: 0.7 }
    - { time: 0.5,  color: [255, 250, 230], alpha: 0.0 }
    - { time: 0.75, color: [40, 50, 100],  alpha: 0.7 }
```

### `portals`
- Type: list of portal mappings
- Optional: yes
- Default: empty list
- Meaning: tile-triggered links to another authored space

Portal fields:

### `id`
- Type: string
- Meaning: stable portal identifier within the source space

### `source`
- Type: tile coordinate mapping
- Meaning: tile in this space that triggers the transition

### `destination_space_id`
- Type: string
- Meaning: authored ID of the destination space

### `destination_tile`
- Type: tile coordinate mapping
- Meaning: tile where the traveler appears in the destination space

### `destination_permanence`
- Type: string or omitted
- Optional: yes
- Meaning: optional runtime permanence override for the instantiated destination
- If omitted, the destination space definition's own `permanence` is used

### `objects`
- Type: list of object entries
- Meaning: all authored map objects, using either explicit instances or compact anonymous placement groups

Two object entry forms are currently valid:

### Explicit object instance
- Use this when the object needs custom `properties`, a `behavior`, container `contents`, or a symbolic id that another object refers to.
- The `id` field is *optional* — only declare it when something elsewhere in the file refers to this instance from a `contents` list.

Fields:

### `id`
- Type: string
- Optional: yes
- Default: omitted (runtime id auto-allocated, instance is anonymous)
- Meaning: symbolic name for this instance, used by `contents` lists in other objects to refer back to it. Must be unique within the file. Pure strings (`barrel_in_cellar`); never numeric.

### `type`
- Type: string
- Meaning: object definition ID for the instance
- This should match a directory name under `assets/overworld_objects/`

### `properties`
- Type: string-to-string mapping
- Optional: yes
- Default: empty mapping
- Meaning: per-instance values passed into object metadata templates
- Example use: a generic `scroll` item can set `spell_id: spark_bolt`

### `placement`
- Type: mapping
- Optional: yes
- Meaning: where the object exists in the world, if it is currently placed on the map.
- Inline children of another object's `contents` list **must not** declare `placement` — their location is "inside the parent" and is inferred automatically.

### `contents`
- Type: list of children — each entry is either a string (reference) or an inline object instance mapping
- Optional: yes
- Default: empty list
- Meaning: items stored inside this object. Intended for container objects such as barrels.
- A bare string entry (e.g. `- special_potion`) refers to another instance's symbolic `id`.
- An inline mapping (e.g. `- type: potion`) defines a child object on the spot. Inline children may themselves carry `properties`, nested `contents`, etc., but never `placement`.

### `behavior`
- Type: mapping or `null`
- Optional: yes
- Meaning: behavior assigned to this specific object instance
- Intended for authored NPCs and future mobs
- Current supported behavior kinds:
  - `roam`
  - `roam_and_chase`

### `facing`
- Type: string (one of `north`, `south`, `east`, `west`), or omitted
- Optional: yes
- Default: the object's `render.default_facing`, or `south` if none
- Meaning: initial facing direction for this specific instance, overriding the object definition's `default_facing`
- Useful for static props whose orientation is authored per placement (e.g. a signpost facing east)

### Anonymous placement group
- Use this when you just want to place many objects of the same type and do not need to refer to them elsewhere in the map file.
- Runtime object IDs are generated automatically during map loading.

Fields:

### `type`
- Type: string
- Meaning: object definition ID for all spawned instances in the group

### `properties`
- Type: string-to-string mapping
- Optional: yes
- Default: empty mapping
- Meaning: per-instance values copied into every generated object in the group

### `placement`
- Type: list of tile coordinate mappings
- Meaning: list of world placements for generated object instances

### `facing`
- Type: string (one of `north`, `south`, `east`, `west`), or omitted
- Optional: yes
- Default: the object's `render.default_facing`, or `south` if none
- Meaning: facing direction applied to every instance in this placement group

Placement fields:

### `x`
- Type: integer
- Meaning: tile x coordinate

### `y`
- Type: integer
- Meaning: tile y coordinate

Example:

Explicit instance example. Most explicit objects don't need an `id` — the cleanest way to fill a container is to nest the children inline:

```yaml
- type: barrel
  placement: { x: 20, y: 13 }
  contents:
    - type: apple
    - type: potion
    - type: scroll
      properties:
        spell_id: lesser_heal
- type: villager
  placement: { x: 8, y: 23 }
  behavior:
    kind: roam
    step_interval_seconds: 1.4
    bounds:
      min_x: 5
      min_y: 21
      max_x: 11
      max_y: 25
- type: goblin
  placement: { x: 18, y: 21 }
  behavior:
    kind: roam_and_chase
    step_interval_seconds: 0.9
    detect_distance_tiles: 5
    disengage_distance_tiles: 8
    bounds:
      min_x: 15
      min_y: 18
      max_x: 21
      max_y: 24
```

Use a symbolic `id` only when something else (e.g. a different object's `contents`, a future scripting hook) needs to refer back to this instance:

```yaml
- type: barrel
  placement: { x: 20, y: 13 }
  contents: [special_potion]
- id: special_potion
  type: potion
  properties:
    spell_id: lesser_heal
```

Anonymous placement group example:

```yaml
- type: tree
  placement:
    - { x: 6, y: 7 }
    - { x: 7, y: 7 }
    - { x: 8, y: 8 }
- type: scroll
  properties:
    spell_id: spark_bolt
  placement:
    - { x: 30, y: 12 }
```

### Compact tile grid format

Instead of listing every tile coordinate in anonymous placement groups, you can describe the map visually using the `legend` and `tiles` fields.

### `legend`
- Type: string-to-string mapping
- Optional: yes
- Default: empty
- Meaning: maps single-character keys to object type IDs
- Keys must be exactly one character each

### `tiles`
- Type: multi-line string (YAML literal block scalar `|`)
- Optional: yes
- Meaning: ASCII grid representation of the map, row-major with y=0 at the top row
- Each row must be exactly `width` characters wide
- The number of rows must be exactly `height`
- Characters present in `legend` produce anonymous object placements; all other characters are ignored (the `fill_object_type` applies to those cells)
- Grid-placed objects cannot carry `properties`; if you need properties on an anonymous group, use an explicit anonymous placement group in `objects:` instead

Both `tiles` (via `legend`) and `objects:` anonymous groups can be used in the same file. The `objects:` field is optional when using `tiles:` alone.

**Layering note:** A tile can have multiple objects — for instance, a wall sitting on top of a water tile. The grid represents one object per cell. To preserve a second object at the same position, add it as an explicit anonymous group in `objects:`.

Example:

```yaml
authored_id: starter_cellar
permanence: ephemeral
width: 12
height: 10
fill_object_type: grass

legend:
  "#": wall

tiles: |
  #####.######
  #..........#
  #..........#
  #..........#
  #..........#
  #..........#
  #..........#
  #..........#
  #..........#
  ############

portals:
  - id: cellar_exit
    source: { x: 6, y: 0 }
    destination_space_id: overworld
    destination_tile: { x: 6, y: 18 }

objects:
  - type: barrel
    placement: { x: 5, y: 4 }
    contents:
      - type: potion
```

Notes:
- Spaces with `persistent` permanence are loaded/shared world spaces.
- Spaces with `ephemeral` permanence may be instantiated on demand and despawned when empty.
- Portals are authored per space and connect a source tile to another authored space definition.
- Portal tiles can also hold normal non-colliding objects, which is how visible entrances/exits such as sinkholes or portal arches are authored.
- Each object may exist in at most one place:
  - placed in the world via `placement`
  - inside exactly one container via another object's `contents`
  - or nowhere
- Objects with no `placement` and no parent container are valid and simply start unspawned.
- Anonymous placement groups cannot be referenced by `contents` because they have no symbolic `id`.
- Anonymous placement groups are expanded into generated object instances during map loading.
- Container contents are ordered by the list order in `contents`.
- Behaviors are authored per explicit object instance, not in object metadata.
- The map loader validates duplicate symbolic ids, missing content references, self-containment, and multi-location conflicts. Numeric runtime ids are auto-allocated and never appear in YAML.
- Rendering order is controlled by object metadata `render.z_index`, not by object order in the map file.

### `spawn_groups`
- Type: list of spawn group mappings
- Optional: yes
- Default: empty list
- Meaning: dynamic NPC spawners. Each group caps the simultaneously alive members of one template at `max_count` and refills empty slots after a Poisson-distributed delay (mean `respawn_mean_seconds`). Members carry the group's `behavior` and persist across saves; on a server restart, surviving members are re-attached to their group and respawn cooldowns resume mid-flight.

Spawn group fields:

#### `id`
- Type: string
- Meaning: stable identifier unique within this space (e.g. `cellar_rats`). Persisted on each spawned member so the group can re-attach them after a save/load.

#### `template`
- Type: string
- Meaning: object definition id (e.g. `rat`). Must resolve via the overworld object metadata.

#### `max_count`
- Type: positive integer
- Meaning: maximum number of simultaneously alive members of this group.

#### `respawn_mean_seconds`
- Type: positive number
- Meaning: mean of the exponential distribution used to schedule each missing slot's respawn timer. Each empty slot ticks down independently; intervals are sampled as `-mean * ln(uniform(0, 1))`.

#### `area`
- Type: mapping
- Meaning: where members are allowed to spawn. Exactly one of `bounds` or `tiles` must be set.
- `area.bounds`: `{ min_x, min_y, max_x, max_y }` — inclusive rectangle. The spawn system samples uniformly within and rejects tiles already occupied by colliders, players, or other NPCs (up to 8 retries before deferring to the next frame).
- `area.tiles`: list of `{ x, y, z? }` mappings — explicit candidate tiles, sampled uniformly with the same occupancy rejection.

#### `behavior`
- Type: same `behavior` mapping accepted by explicit object instances (`roam` or `roam_and_chase`).
- Meaning: applied to every member spawned by this group. Bounds are independent of `area` — typically you'll want them to match (or be a superset) so members aren't constantly trying to walk back into the spawn region.

Example:

```yaml
spawn_groups:
  - id: cellar_rats
    template: rat
    max_count: 3
    respawn_mean_seconds: 30.0
    area:
      bounds: { min_x: 1, min_y: 1, max_x: 10, max_y: 8 }
    behavior:
      kind: roam_and_chase
      step_interval_seconds: 0.5
      detect_distance_tiles: 4
      disengage_distance_tiles: 6
      bounds: { min_x: 1, min_y: 1, max_x: 10, max_y: 8 }
```

## 2. Overworld Object Metadata YAML

Path:
- `assets/overworld_objects/<object_id>/metadata.yaml`
- reusable parents live under `assets/object_bases/*.yaml`

Purpose:
- Defines object type behavior and rendering metadata.
- The directory name acts as the object ID used in map files and runtime data.
- Supports single-parent inheritance through `extends`.

Top-level fields:

### `name`
- Type: string
- Meaning: display name of the object

### `description`
- Type: string **or** list of description entries
- Meaning: human-readable description shown when the player inspects the object

A plain string is the simplest form:

```yaml
description: A heavy wooden barrel.
```

For stackable items you can supply a list where each entry is either a plain string (always shown) or a mapping with a `text` field and an optional `stack_size` interval `[min, max]`. The first matching entry wins; use `null` for an open-ended bound.

```yaml
description:
  - text: A single red apple.
    stack_size: [1, 1]
  - text: A pair of apples.
    stack_size: [2, 2]
  - text: "{count_written} apples."
    stack_size: [3, ~]
```

The `text` value supports three count placeholders in addition to the normal `{properties.*}` templates:

| Placeholder | Example output for 12 |
|---|---|
| `{count}` | `12` |
| `{count_written}` | `twelve` |
| `{count_customary}` | `a dozen` |

`{count_customary}` uses built-in English customary names (singleton, pair, trio, dozen, baker's dozen, score, gross) and falls back to `{count_written}` when no customary name exists for the quantity.

### `extends`
- Type: string
- Optional: yes
- Meaning: parent object/base ID to inherit from before applying local overrides
- Parent IDs may refer to:
  - another object definition directory under `assets/overworld_objects/`
  - a base definition file under `assets/object_bases/`
- Inheritance is single-parent only
- Merge rules:
  - mappings are deep-merged
  - scalars are overridden by the child
  - lists are replaced by the child

### `colliding`
- Type: boolean
- Meaning: whether the object blocks movement

### `movable`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: whether the object can be dragged or repositioned in the game world

### `rotatable`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: whether the player can rotate this object in-place with the `Ctrl+Q`
  / `Ctrl+E` shortcuts when standing on an adjacent tile. Independent of
  `movable` — a static signpost can be rotatable but not movable. Rotation
  updates the object's `Facing` component; pair with `render.rotation_by_facing`
  for sprites that should visibly turn.

### `storable`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: whether the object can be placed into backpack, container, or equipment slots

### `equipment_slot`
- Type: string or `null`
- Optional: yes
- Meaning: if present, the storable item is recognized as equippable gear for that paperdoll slot
- Valid values:
  - `amulet`
  - `helmet`
  - `weapon`
  - `armor`
  - `shield`
  - `legs`
  - `backpack`
  - `ring`
  - `boots`

### `fillable_properties`
- Type: list of strings
- Optional: yes
- Default: empty list
- Meaning: names of per-instance properties that this object definition expects to receive
- Intended for generic item types that are specialized by instance state, such as scrolls carrying different spells

### `text_kind`
- Type: string (one of `book`, `tombstone`, `engraving`)
- Optional: yes
- Default: unset
- Meaning: marks this definition as a persistent-text artifact. Drives the right-click "Read" verb and the title of the reader-editor window.
  - `book` — read + write (write requires a `pen` in inventory). Per-instance `title` / `text` / `author_name` live in `properties`.
  - `tombstone` — read-only. Spawned on player death with auto-engraved `title` / `text`.
  - `engraving` — typically inferred from `engravable: true` on items; setting it explicitly on world objects is unusual.

### `engravable`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: when true, players carrying a `pen` can right-click → Read → Edit to set a one-time `properties["inscription"]` (≤ 32 chars). The handler also writes `properties["inscription_line"]` (a pre-formatted sentence), so descriptions can interpolate `{properties.inscription_line|}` and stay tidy when the item has not been inscribed.

### `stats`
- Type: mapping
- Optional: yes
- Default: empty mapping with zero bonuses
- Meaning: for equippable items, additive stat modifiers granted while equipped. For NPC definitions (objects that `extends: npc` and are spawned with a map `behavior`), these values are the NPC's **absolute base attributes** rather than modifiers. Per-attribute fallback: any field left at `0` falls back to the NPC default (strength/agility/constitution = 9, willpower/focus = 8, charisma = 7).

`stats` fields:

### `strength`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: modifies physical power, contributing to melee-oriented derived stats and carrying capacity

### `agility`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: modifies dexterity and speed-oriented character aptitude

### `constitution`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: modifies endurance, contributing heavily to maximum health

### `willpower`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: modifies resolve and magical endurance, contributing to maximum mana

### `charisma`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: modifies presence and social aptitude for future interaction systems

### `focus`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: modifies precision and spell control, contributing to maximum mana

### `max_health`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: increases or decreases the holder's maximum health

### `max_mana`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: increases or decreases the holder's maximum mana

### `storage_slots`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: increases or decreases available backpack storage slots

### `attack_profile`
- Type: mapping or `null`
- Optional: yes
- Meaning: for weapons and NPCs, how this entity attacks in melee or ranged combat
- Fields:
  - `kind`: `melee` or `ranged`
  - `hit_vfx` (optional): VFX definition id played on the target on a hit. Falls back to `"blood_splash"`.
  - `damage_type` (optional): one of `blunt`, `cut`, `pierce`, `fire`, `frost`, `earth`, `lightning`, `poison`, `acid`, `death`, `holy`, `arcane`. Defaults to `blunt` for melee and `pierce` for ranged. Shown in the combat log (e.g. `[Goblin hit Hero for 4 cut damage]`); no resistance math is applied yet.
  - `on_hit_effects` (optional): list of `MagicEffects` entries probabilistically applied to the target after a landed hit. Each entry is rolled independently. Fields per entry:
    - `kind`: any `EffectKind` (`burning`, `chill`, `poisoned`, `paralyze`, `slow`, `sleep`, `drunk`, etc.)
    - `magnitude`: float (effect-specific — for DOTs this is damage per second tick)
    - `seconds`: float duration
    - `chance` (optional, default `1.0`): probability in `[0, 1]` of applying this entry on a successful hit
    - `secondary_magnitude` (optional): second parameter used by some effects (e.g. `chill`'s slow multiplier)
    Example: a fire weapon that has a 35% chance to set the target on fire for 6 seconds:
    ```yaml
    attack_profile:
      kind: melee
      damage_type: fire
      on_hit_effects:
        - kind: burning
          chance: 0.35
          magnitude: 2.0
          seconds: 6.0
    ```

### `base_range_tiles`
- Type: integer
- Optional: yes
- Default: `4` when `attack_profile.kind` is `ranged` and this field is absent
- Meaning: maximum Chebyshev distance (in tiles) at which a ranged attack can engage

### `ammo_type`
- Type: string
- Optional: yes
- Meaning: object ID used as the projectile sprite for ranged NPC attacks

### `damage`
- Type: damage expression string
- Optional: yes
- Default: `1d6+strength/5` (melee default)
- Meaning: damage formula evaluated on each attack. The expression is a `+`-separated list of terms:
  - A dice term `NdM` (at most one per expression, e.g. `1d6`, `2d4`)
  - A stat term `<stat>`, `<stat>*<multiplier>`, or `<stat>/<divisor>` (`strength`, `agility`, `constitution`, `willpower`, `charisma`, `focus`, plus the abbreviations `str`/`agi`/`con`/`wil`/`cha`/`foc`)
  - A plain integer bonus
- Examples: `1d6+strength`, `2d4+agility`, `1d12+strength/2+5`
- Both weapons (when equipped by the player) and NPCs read this field.

### `hp`
- Type: damage expression string
- Optional: yes
- Default: unset — the NPC uses the derived HP formula `35 + constitution*6 + strength*2 + stats.max_health`
- Meaning: NPC maximum health formula. Uses the same expression syntax as `damage`. Rolled once per spawn using the NPC's own attributes, so dice terms produce per-instance variance.
- Examples: `1d8+30+constitution*3`, `2d20+80+constitution*6`, `50+constitution*5` (deterministic)
- Player HP is unaffected by this field.

### `armor`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: damage reduction for items equipped in defensive slots (`armor`, `helmet`, `legs`, `boots`). Values are summed across all equipped pieces. On every incoming hit that lands, the defender rolls a uniform integer in `0..=armor_total` and subtracts it from the damage. Final damage is floored at `1`.
- Only counted when worn in one of the four defensive slots above; setting `armor` on a weapon or ring has no effect.
- **Also applies to NPCs**: an NPC's own `armor` field becomes its `DefenseStats.armor` at spawn.

### `block`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: damage reduction specific to the `shield` slot. Block is now **chance-gated**: it only fires when the `block_chance` roll succeeds (see below). When it does, the defender rolls `0..=block` and subtracts before the armor roll. Combined with `armor` the post-hit order is: `damage = max(1, raw - block_roll - armor_roll)`.
- Only counted when equipped in the `shield` slot (for players) or set on an NPC definition.

### `block_chance`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: percentage chance (0-100) that a shield triggers its `block` mitigation on an incoming hit. Effective chance is `block_chance + AGI_mod * 2`, clamped to `[0, 95]`. Only meaningful in the `shield` slot for players or on an NPC's own definition.
- Example: `block_chance: 25` on a wooden shield → a fighter with AGI 14 (+2 mod) gets a 29% block rate.

### `dodge_bonus`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: flat bonus added to the wearer's dodge DC (the to-hit target attackers must beat). Counts on any equipment slot (boots, cloaks, rings, etc.), and stacks across pieces. Dodge DC = `10 + AGI_mod + sum(dodge_bonus)`.
- Example: `dodge_bonus: 1` on a pair of traveler boots → +1 DC for whoever wears them.

### `use_effects`
- Type: mapping
- Optional: yes
- Default: empty mapping with no effect
- Meaning: consumable on-use effects applied to the player when the item is used

`use_effects` fields:

### `restore_health`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: health restored immediately on use

### `restore_mana`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: mana restored immediately on use

### `regen_multiplier`
- Type: float
- Optional: yes
- Default: `1.0`
- Meaning: HP/MP regen rate multiplier applied while the buff is active. Values below 1.0 are clamped to 1.0 (no debuffs). Only takes effect if `regen_duration_seconds` is also positive.

### `regen_duration_seconds`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: how long (in seconds) the regen-rate buff persists after consumption. Re-eating extends the remaining duration; the multiplier snaps to `max(current, new)` so a stronger buff isn't diluted by a follow-up.

### `grants_item_modifier`
- Type: mapping ([`ItemModifier`](#item-modifiers-enchantments))
- Optional: yes
- Default: none
- Meaning: when the item is used, the player picks one of their own inventory/equipment items and this modifier is applied to it (e.g. a poison flask coating a weapon). Using the item enters an item-target cursor mode; the next slot click resolves the target. The source item is consumed only when the enchantment actually applies (a weaker modifier rejected by the anti-stack rule wastes nothing). Makes the item `usable`.

### `use_texts`
- Type: list of strings
- Optional: yes
- Default: empty list
- Meaning: possible narrator texts shown when the item is used; one is chosen per use
- If omitted or empty, the runtime falls back to `<Item name> used.`

### `use_on_texts`
- Type: list of strings
- Optional: yes
- Default: empty list
- Meaning: possible narrator texts shown when the item is used on a non-player target
- Supports simple placeholders:
  - `{target}` inserts the target's display name
  - `{item}` inserts the used item's display name
- If omitted or empty, the runtime falls back to `Used <Item name> on <Target name>.`

### `spell_id`
- Type: string or `null`
- Optional: yes
- Meaning: if present, the item casts the referenced spell when used
- Spell IDs map to YAML files under `assets/spells/`
- Targeted spells enter spell-target cursor mode; untargeted spells cast immediately
- This field may also be templated from instance properties, for example `"{properties.spell_id}"`

### `container_capacity`
- Type: integer
- Optional: yes
- Meaning: if present, the object becomes a container with that many slots
- Example: `8` for a 2x4 container grid

### `accepts_storable_containers`
- Type: bool
- Optional: yes
- Default: `true`
- Meaning: only meaningful on objects that have `container_capacity`. When
  `false`, the engine refuses to place a *storable container item* (a pouch)
  inside this container. Used by the `pouch` base to forbid pouch-in-pouch
  nesting while keeping chests/backpacks fully permissive.

### `weight`
- Type: float (kilograms)
- Optional: yes
- Default: `0.0`
- Meaning: per-instance carry weight. Stacked items count as
  `weight × quantity`. Container items (pouches) count themselves *plus* the
  recursive weight of their contents — picking up a pouch that holds 5
  arrows costs `pouch.weight + 5 × arrow.weight`. Players have a soft
  carry-cap (`MaxCarryWeight::soft_cap`, default `20 + STR × 2 kg`) and a
  hard cap (`soft × 1.5`). Pickups above the hard cap are rejected with a
  "Too heavy" chat message; above the soft cap the player is `Encumbered`
  and walks at half speed.

### `render`
- Type: mapping
- Meaning: visual configuration for the object

### `sound_paths`
- Type: list of strings
- Optional: yes
- Default: empty list
- Meaning: reserved list of audio asset paths associated with the object

### `max_stack_size`
- Type: integer
- Optional: yes
- Default: `1` (non-stackable); `consumable` base sets it to `100`
- Meaning: maximum number of identical items that can occupy a single inventory slot; set to `1` for equipment
- Example: `max_stack_size: 250`

### `inspect_range`
- Type: integer (tiles)
- Optional: yes
- Default: `3` when absent
- Meaning: base Chebyshev-distance from which a player can identify this object with the Inspect action. The server's effective range is `inspect_range + floor(focus / 5)`; beyond that the player sees only "You stand too far to see it clearly." Set higher (e.g. `6`) for large/bright landmarks (signs, fires, statues) or lower (e.g. `1`) for tiny items (coins, gems).
- Example: `inspect_range: 5`

### `stack_sprites`
- Type: list of mappings
- Optional: yes
- Default: empty (always use `render.sprite_path`)
- Meaning: per-quantity sprite overrides; each entry has `min_count` (inclusive) and `sprite_path`; the highest-matching tier wins; falls back to `render.sprite_path` if no tier matches
- Example:
  ```yaml
  stack_sprites:
    - min_count: 100
      sprite_path: overworld_objects/gold_coin/pile.png
    - min_count: 10
      sprite_path: overworld_objects/gold_coin/handful.png
  ```

`render` fields:

### `z_index`
- Type: float
- Meaning: render ordering depth relative to other world objects

### `debug_color`
- Type: 3-item integer list
- Meaning: fallback RGB color used if no sprite is configured
- Expected format: `[r, g, b]`

### `debug_size`
- Type: float
- Meaning: size multiplier used for sprite or debug-color rendering relative to the configured tile size

### `sprite_path`
- Type: string or `null`
- Optional: yes
- Meaning: Bevy asset path to the sprite image
- Path should be relative to `assets/`

### `sprite_width_tiles`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: sprite width in tile units for oversized sprites
- When both `sprite_width_tiles` and `sprite_height_tiles` are greater than 0, the sprite is rendered at this size instead of the `debug_size` square
- Enables Tibia-style oversized rendering where sprites extend beyond their grid tile

### `sprite_height_tiles`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: sprite height in tile units for oversized sprites
- Used together with `sprite_width_tiles` to define non-square sprite dimensions
- A tree occupying 1 tile but visually 2 tiles tall would use `sprite_height_tiles: 2.0`

### `y_sort`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: enables y-based depth sorting within the object's z_index layer
- When enabled, sprites are anchored at their bottom-center (foot position) and rendered with depth based on their vertical position: objects lower on screen render in front of objects higher on screen
- Recommended for obstacles, NPCs, and players to achieve correct occlusion with oversized sprites
- Ground tiles and flat pickups should leave this as `false`

### `default_facing`
- Type: string (one of `north`, `south`, `east`, `west`), or omitted
- Optional: yes
- Default: `south` (front-facing)
- Meaning: initial facing direction for this object type when no per-instance `facing` is set in the map YAML
- Used together with `rotation_by_facing` and/or directional animation clips to render oriented sprites
- Players and NPCs overwrite this on movement; static objects retain it

### `rotation_by_facing`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: when `true`, the sprite is rotated around its center via `Transform::rotation_z` to match the object's `Facing`
- Use this for single-sprite props (signposts, arrows, beds) that have no per-direction animation frames
- Rotated sprites use center anchoring — when this flag is `true`, the bottom-center y-sort shift is skipped so the sprite sits square on the tile after rotation. Design sprites for this flag as square tiles

### `block_size`
- Type: unsigned integer (`0`, `1`, or `2`)
- Optional: yes
- Default: `0`
- Meaning: physical size of the object in *half-block units*. `0` = flat (ground item, decal — does not participate in vertical stacking); `1` = half block (chest, low rock); `2` = full block (barrel, wall, stone step). Drives stack rendering, auto-climb, and bottom-anchored sprite alignment
- The world's `z` axis is in half-block units (a real floor = 2 z); a half-block step up adds `+1` to z, a full-block step adds `+2`
- Combined with `walkable_surface: true`, this lets the player auto-step up onto the object by `block_size` z units when walking into it — they snap onto its top and snap back down on stepping off
- Block-sized objects (`block_size > 0`) are rendered bottom-anchored (sprite footprint sits on the tile, art rises upward), unless `rotation_by_facing` is set — rotated sprites stay center-anchored
- Stacking: when an object is dropped on a tile already holding block-sized objects, it lands on top of the column (its feet at the stack top). The stack-top must be within `±1` z of the player and the resulting new top within `+2` z of the player's feet — caps how high you can build from where you're standing

### `floor_mask_rect`
- Type: array of 4 floats `[x0, y0, x1, y1]` or omitted
- Optional: yes
- Default: omitted (floor fills the whole tile as usual)
- Meaning: a sub-tile rectangle, in tile fractions with `x` = west→east and `y` = south→north (matching world axes; `0` = the tile's min edge, `1` = the max edge), that restricts where the floor is drawn on this object's tile. Floor renders **only** inside the rectangle.
- Used by the directional walls (`wall_s`, `wall_e`, corners, …) to keep floor on the interior side of the slab so it meets the wall flush, leaving the exterior strip free for other terrain. Example: `wall_s` (interior to the north, slab inset `0.25`) uses `[0.0, 0.25, 1.0, 1.0]` — floor is cut south of the slab line.
- The floor renderer applies the clip per-quadrant (the floor dual-grid renders at tile corners); a tile with no overlapping mask is unaffected. Implementation: `FloorMaskMap` + `clip_quadrant` in `src/world/floor_render.rs`.

### `hide_when_inside_facing`
- Type: string (`north`, `south`, `east`, `west`) or omitted
- Optional: yes
- Default: omitted (no fade)
- Meaning: marks this object as a building wall that should fade to a faint silhouette when the player is inside an enclosed area (the tile directly above the player has `occludes_floor_above: true`). Only `south` and `east` are honoured — these are the camera-facing walls that would otherwise obstruct the player view
- The wall remains technically present (it still blocks movement); only its sprite alpha is reduced

### `stack_order`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: tiebreaker when multiple block-sized objects sit at the same `z` in a column (overlapping flat decals, equally-sized props authored together). Lower values render at the bottom of the stack, higher values on top. When two objects share the same `stack_order`, the authoritative `object_id` (server-allocated) breaks the tie. Physical stacking sorts by `z` first; `stack_order` only matters for same-`z` ties
- Suggested values: barrel `10`, chest `20`, wall `50`

### `animation`
- Type: mapping or `null`
- Optional: yes
- Default: `null` (no animation; static `sprite_path` is used instead)
- Meaning: sprite-sheet animation configuration for the object
- When present, the object uses a texture atlas instead of a static image
- Objects without this field fall back to the static `sprite_path` with no behaviour change

`animation` fields:

### `sheet_path`
- Type: string
- Meaning: Bevy asset path to the sprite-sheet PNG, relative to `assets/`
- The sheet must be a uniform grid where every frame cell is the same pixel size

### `frame_width`
- Type: integer
- Meaning: width in pixels of a single animation frame cell

### `frame_height`
- Type: integer
- Meaning: height in pixels of a single animation frame cell

### `sheet_columns`
- Type: integer
- Meaning: number of frame columns in the sprite-sheet grid
- Used together with `sheet_rows` to register the texture atlas layout

### `sheet_rows`
- Type: integer
- Meaning: number of frame rows in the sprite-sheet grid

### `clips`
- Type: mapping from clip name string to clip definition
- Meaning: named animation clips that can be played on this object
- Well-known clip names used by the animation system:
  - `idle` — played when the entity is not moving (looping)
  - `walk` — played for one movement step, returns to `idle` when the step ends

Each clip definition has these fields:

### `row`
- Type: integer
- Meaning: zero-indexed row in the sprite-sheet grid where this clip's frames begin

### `start_col`
- Type: integer
- Meaning: zero-indexed column in the sprite-sheet grid where this clip's frames begin

### `frame_count`
- Type: integer
- Meaning: number of consecutive frames in this clip, starting from `(row, start_col)`

### `fps`
- Type: float
- Meaning: frames-per-second playback rate

### `looping`
- Type: boolean
- Optional: yes
- Default: `true`
- Meaning: whether the clip loops indefinitely or freezes on its last frame

Animated object example:

```yaml
extends: npc
name: Goblin
render:
  z_index: 1.0
  debug_color: [92, 156, 68]
  debug_size: 0.92
  y_sort: true
  animation:
    sheet_path: overworld_objects/goblin/sheet.png
    frame_width: 32
    frame_height: 48
    sheet_columns: 4
    sheet_rows: 2
    clips:
      idle:
        row: 0
        start_col: 0
        frame_count: 1
        fps: 1.0
        looping: true
      walk:
        row: 1
        start_col: 0
        frame_count: 4
        fps: 8.0
        looping: true
```

Notes:
- The atlas frame index for a given clip frame is `row * sheet_columns + start_col + frame_offset`.
- If the `idle` clip is omitted from `clips`, the animation system defaults to frame 0.
- If the `walk` clip is omitted, the system falls back to `idle` during movement.
- Smooth movement (viewport scroll for the player, per-entity offset for NPCs) is automatic when an entity has `animation` defined; no additional configuration is needed.

### `recolor_layers` (optional)

Per-region sprite layers stacked on top of the base sprite, each receiving an
independent per-character tint. Currently used by the player definition for
hair / torso / trousers customization at character creation. Each layer PNG
must share the same dimensions and frame grid as the parent's `animation`
sheet so frame indices stay locked together.

```yaml
render:
  animation:
    sheet_path: overworld_objects/player/sheet.png
    frame_width: 32
    frame_height: 48
    sheet_columns: 4
    sheet_rows: 2
    clips: { ... }
  recolor_layers:
    - key: hair       # tinted from PlayerAppearance.hair
      sheet_path: overworld_objects/player/layers/hair.png
    - key: torso      # tinted from PlayerAppearance.torso
      sheet_path: overworld_objects/player/layers/torso.png
    - key: trousers   # tinted from PlayerAppearance.trousers
      sheet_path: overworld_objects/player/layers/trousers.png
```

| Field | Type | Required | Description |
|---|---|---|---|
| `key` | string | yes | Region name. Recognized: `hair`, `torso`, `trousers` (`skin` is accepted but renders un-tinted; usually unnecessary since the base sheet already carries the skin pixels). Unknown keys are skipped with a warning. |
| `sheet_path` | string | yes | Bevy asset path to the layer sheet. Same dimensions as `animation.sheet_path`. |

Notes:
- Layers are tinted via Bevy's `Sprite::color` (multiplicative). Author each
  layer in a near-white neutral so the chosen RGB defines the visible color.
- Layers are stacked in declaration order with small Z offsets above the base
  sprite, so layer pixels overwrite the underlying base pixels in their
  region. Layer pixels must cover the exact same coordinates as the base sheet's
  region pixels — otherwise the original colors leak through the gaps.
- Only the player definition currently has consumer code wired up. Other
  objects may declare `recolor_layers` for forward-compat; nothing will spawn
  them until a system opts in.

Example:

```yaml
extends: movable_obstacle
name: Barrel
description: A heavy wooden barrel that can be opened as a simple container.
container_capacity: 8
render:
  z_index: 0.25
  debug_color: [134, 83, 42]
  debug_size: 0.62
  sprite_path: overworld_objects/barrel/sprite.png
sound_paths: []
```

Notes:
- The object ID is the folder name, not a field inside the YAML file.
- `movable`, `storable`, `equipment_slot`, `stats`, `use_effects`, `use_texts`, `use_on_texts`, and `container_capacity` can coexist if needed.
- `extends` is resolved before deserializing the final object definition.
- If `sprite_path` is omitted or `null`, the object falls back to colored debug rendering.
- The current runtime uses these fields directly for world spawning, collision, pickup behavior, and container creation.
- `name`, `description`, and `spell_id` support `{properties.<field>}` templating.
- `{properties.<field>.name}` resolves the property value as a spell ID and inserts that spell's display name.
- `description` additionally supports `{count}`, `{count_written}`, and `{count_customary}` placeholders that resolve to the current world-object or inventory stack quantity.
- `{properties.<field>|<fallback text>}` substitutes the fallback string when the property is missing or empty. The fallback runs through the rest of the template untouched, so `"{properties.inscription_line|}"` collapses to nothing for un-inscribed items.

Equippable item example:

```yaml
extends: equipment
name: Silver Ring
description: A tarnished silver ring with a faint blue sheen.
equipment_slot: ring
stats:
  willpower: 2
  focus: 1
render:
  z_index: 0.24
  debug_color: [170, 174, 196]
  debug_size: 0.42
  sprite_path: overworld_objects/silver_ring/sprite.png
sound_paths: []
```

Usable item example:

```yaml
extends: consumable
name: Potion
description: A small blue potion flask.
use_effects:
  restore_mana: 20
use_texts:
  - You drink the potion.
  - The potion tingles as your mana returns.
render:
  z_index: 0.24
  debug_color: [58, 109, 201]
  debug_size: 0.45
  sprite_path: overworld_objects/potion/sprite.png
sound_paths: []
```

Spell scroll example:

```yaml
extends: pickup
fillable_properties:
  - spell_id
name: Scroll of {properties.spell_id.name}
description: A charged scroll carrying {properties.spell_id.name}.
spell_id: "{properties.spell_id}"
render:
  debug_color: [224, 171, 108]
  debug_size: 0.45
  sprite_path: overworld_objects/scroll/sprite.png
```

### Object lighting (`light`)

Optional top-level mapping. When present, the object emits light in the world. The client's lighting system reads this metadata and attaches a `LightSource` ECS component to the projected entity. The base value can be overridden or suppressed per state via `states.<name>.light` / `states.<name>.clear_light: true`.

```yaml
# Always-on light (e.g. campfire):
light: { color: [255, 150, 70], radius: 5.5, intensity: 1.0 }
```

Per-state lighting (e.g. wall torch — only the `lit` state glows):

```yaml
states:
  unlit:
    sprite_path: overworld_objects/torch/unlit.png
    clear_light: true
  lit:
    sprite_path: overworld_objects/torch/lit_sheet.png
    light: { color: [255, 180, 90], radius: 4.5, intensity: 0.9 }
```

Subfields:

- `color` — `[r, g, b]` u8 sRGB. The hue radiated by this source.
- `radius` — float, tiles. Beyond this distance the contribution is zero. Falloff is quadratic (`(1 - d/r)^2`), Chebyshev-distance on the same z-floor.
- `intensity` — float, default `1.0`. Multiplier on `color`. Values above `1.0` over-drive brighter for cosmetic punch but are clamped at apply time.

### Stateful objects (`states` / `initial_state` / `interactions` / `wires_to`)

Optional. Lets a definition declare a small state machine the player can drive through the context menu (doors open/closed, torches lit/unlit, levers off/on). Any field omitted from a per-state override falls back to the base definition.

```yaml
states:
  closed:
    sprite_path: overworld_objects/wooden_door/closed.png
    colliding: true
  open:
    sprite_path: overworld_objects/wooden_door/open.png
    colliding: false
initial_state: closed
interactions:
  - verb: open
    label: Open
    from: [closed]
    to: open
  - verb: close
    label: Close
    from: [open]
    to: closed
```

#### `states`
- Type: mapping of state-name → `{sprite_path?, animation?, colliding?, light?, clear_light?}`
- Optional: yes
- Meaning: per-state visual + collider + lighting overrides. Each state may override `sprite_path` (static), `animation` (atlas — same shape as `render.animation`), `colliding`, and/or `light` (see below). `clear_light: true` suppresses any base `light` for that state (e.g. unlit torch). Unset fields inherit from the base definition.

#### `initial_state`
- Type: string
- Optional: yes (required when `states` is non-empty for new spawns to land in a known state)
- Meaning: state name a freshly spawned instance starts in. Persistence load overrides this from `properties["state"]` when present.

#### `interactions`
- Type: list of `{verb, label?, from?, to, side_effects?}`
- Optional: yes
- Meaning: verbs the player can invoke on this object via the context menu.
  - `verb` — short identifier (e.g. `open`, `light`, `pull`).
  - `label` — display string for the context menu; defaults to capitalised `verb`.
  - `from` — list of states this verb is available in. Empty/absent = always.
  - `to` — state to transition into.
  - `side_effects` — optional list of post-transition actions (see below).

#### `side_effects`
Each entry is tagged by `kind`:

- `kind: set_target_state` — `target` is a property template (e.g. `"{properties.target}"`); the resolved value must be a runtime u64. The targeted object is moved into `to` directly (validation against its own `from` is bypassed for cascades). Used by levers driving doors. Requires the source's definition to list the property key under `wires_to`.
- `kind: open_container_panel` — emits the same `OpenContainer` UI event as the player right-clicking a container; useful when an interaction should both transition state and pop a container view.

#### `wires_to`
- Type: list of property keys
- Optional: yes
- Meaning: at map load time, every property whose key appears in this list is rewritten from its authored map-id (e.g. `cellar_door`) to the runtime u64 of the matching object (as a decimal string). Missing targets panic at load. Cascades resolve `{properties.<key>}` against this rewritten value.

Lever wired to a door (map YAML):

```yaml
- id: cellar_door
  type: wooden_door
  placement: { x: 12, y: 8 }

- type: lever
  placement: { x: 4, y: 4 }
  properties:
    target: cellar_door
```

Chests get their `open` / `closed` visual purely from the *container-panel viewer count* — no `interactions:` block needed; just declare both `closed` and `open` states alongside `container_capacity`.

### Passive `on_stepped` triggers

A separate, **player-initiated-NOT-required** trigger pathway. Whenever an
entity (player or NPC) moves onto a tile holding an object with `on_stepped:`,
the listed effects fire automatically. Use this for environmental hazards —
fires that burn, traps that snap, slime puddles that slow.

```yaml
on_stepped:
  - from: [armed]                # optional; empty/absent = matches any state
    tick_seconds: 1.0            # optional; if set, the trigger ALSO re-fires
                                 # every N seconds while an entity remains on
                                 # the tile and the from-filter still matches.
    effects:
      - kind: apply_damage
        amount: "2d6+4"
      - kind: apply_effect
        effect: chill
        magnitude: 1.0
        seconds: 4.0
        secondary_magnitude: 2.0
      - kind: set_state
        state: sprung
```

Each entry is one trigger. Multiple triggers are evaluated in order; effects
inside a trigger run in declared order, so an `apply_damage` lands before any
`set_state` in the same trigger (you take the bite *then* the trap snaps).

#### Trigger fields

- `from` — list of `ObjectState` names this trigger fires in. Empty or absent
  = fires regardless of state (works on stateless objects too).
- `tick_seconds` *(optional)* — if set, the trigger fires on entry **and**
  every `tick_seconds` thereafter while an entity remains on the tile and the
  `from` filter still matches. Each tick re-runs the listed effects — so
  `apply_effect` stacks (a new `ActiveEffect` entry per tick), `apply_damage`
  rolls fresh damage, and `set_state` is idempotent once the state has
  flipped (after which the `from` filter naturally short-circuits further
  ticks on the same object). Omit for legacy one-shot-on-entry behavior. Use
  this for *zone hazards* — fire patches, acid pools, healing fountains —
  where damage should escalate while standing still and taper after leaving.
- `effects` — required list of effect entries.

#### Effect kinds

Tag each effect with `kind:`.

- `kind: apply_effect` — append a timed `MagicEffects` entry on the stepper
  (player or NPC; entities without `MagicEffects` are silently skipped). Same
  shape as a spell's `EffectSpec`:
  - `effect`  — `EffectKind` (`burning`, `chill`, `poisoned`, `slow`,
    `paralyze`, `sleep`, `bless`, `shield`, `haste`, `glimmer`, `drunk`)
  - `magnitude` — float
  - `seconds`  — float (≤ 0 is silently ignored)
  - `secondary_magnitude` — optional float (only read by `chill`)

  The stepping trap is the caster (`None` internally), so no XP is granted if
  a resulting DoT delivers a killing blow.

- `kind: apply_damage` — roll a `DamageExpr` and queue it through the normal
  damage pipeline as `DamageSource::Environment` (no XP credit). `amount`
  follows the same grammar as weapon `damage`: `<NdM>[+stat[*k|/k]][+bonus]`.
  Environmental traps have no attacker stats, so stat terms always evaluate to
  zero — keep it dice + bonus (e.g. `"2d6+4"`, `"1d10"`, `"15"`).

- `kind: set_state` — transition this object's `ObjectState` to `state`. The
  definition should declare matching `states:` (with per-state collider/sprite
  overrides) and usually a recovery `interactions:` verb (e.g. `rearm`). The
  transition cascade is the same one used by the player-driven interactions
  pipeline, so collider toggles, sprite swaps, and registry persistence all
  happen automatically.

A trap that snaps shut combines all three: see
`assets/overworld_objects/bear_trap/metadata.yaml`. A reusable hazard with no
state changes (e.g. a permanent patch of fire) uses just `apply_effect` — see
`assets/overworld_objects/blazing_fire/metadata.yaml`.

### Hidden trait

Hiddenness is **per-instance**, not per-type. To author a specific placed
object as hidden, set a `hidden_dc` entry in that instance's `properties:`
block in the map YAML:

```yaml
- id: bear_trap
  type: bear_trap
  placement: { x: 11, y: 11 }
  properties:
    hidden_dc: "15"
```

- `hidden_dc` — Perception DC the player must roll against. Higher = harder to
  spot. The value is a stringly-typed property (same convention as `state:`).

An object loaded with `hidden_dc` spawns with a server-side `Hidden` component
and never appears in any player's `world_objects` until that player detects
it. Detection is per-player and **sticky** — once spotted, the object stays
visible to that player across reloads (the `detected_by` set is persisted in
the world snapshot, as is the runtime DC).

Reveal happens two ways:

- **Passive perception** — the moment a player comes within `inspect_range - 1`
  Chebyshev tiles of the object (per-object, defaults to 2 tiles when the
  definition omits `inspect_range`), the server rolls a `Skill::Perception`
  check against the object's DC. After any roll, that (player, object) pair
  gets a 5-second cooldown before the next attempt. On success, a narrator
  line `"You spot a {name}!"` is pushed to the player's chat log and the
  object naturally pops into view on the next projection tick.
- **Auto-reveal on step** — if the object also declares `on_stepped:`, the
  player's foot reveals it on touch, regardless of the trigger's state filter.

`hidden_dc` + `on_stepped` is the canonical "authored trap" combination — see
the bear trap at `assets/maps/overworld.yaml`. `hidden_dc` alone (no
`on_stepped`) is a "secret stash" — passive perception is the only way to
spot it. Objects dropped or placed by players are never automatically hidden;
use the player-driven Hide action below.

### Player-hideable items (`can_hide`)

When `can_hide:` is present on the type definition, the right-click menu
surfaces a **Hide** action for the object whenever the actor has at least 1
rank of Thievery and the object is not already hidden. Triggering it runs a
Thievery check (DC 10, the object's `sneakiness` as a situational bonus). On
success, the object gains the `Hidden` component with `dc = check_total / 2`
and the placer is seeded into `detected_by` so they continue to see it. On
failure, a narrator line is emitted and the object stays visible.

```yaml
can_hide:
  sneakiness: 2
```

- `sneakiness` — integer modifier added to the placer's Thievery check total.
  Higher = inherently easier to conceal (small, drab, camouflaged). May be
  negative for bulky items. Defaults to `0` when omitted.

The halving of `check_total` keeps DCs from running into unreachable territory
at high levels — see `world::hide_action::HIDE_THRESHOLD_DC` for the threshold
constant and `world::hide_action::process_hide_commands` for the apply path.

### NPC Loot Tables

NPCs (objects that `extends: npc`) may include an optional `loot` section. When the NPC dies it spawns a corpse container at its tile. The corpse holds any rolled loot and disappears after `corpse_despawn_seconds`.

| Field | Type | Default | Description |
|---|---|---|---|
| `corpse_type_id` | string | `generic_corpse` | Object definition ID to use for the corpse container. Create a custom one to give it a unique sprite/description. |
| `corpse_despawn_seconds` | float | `60` | Time in seconds before the corpse (and uncollected loot) vanishes. |
| `drops` | list | `[]` | List of potential item drops (see below). |

Each entry in `drops`:

| Field | Type | Default | Description |
|---|---|---|---|
| `type_id` | string | required | Object definition ID of the item to drop. |
| `quantity` | int or `uniform(min, max)` | `1` | How many to place. A bare integer gives a fixed count; `uniform(5, 10)` rolls a random integer in `[5, 10]` inclusive. |
| `probability` | float | `1.0` | Chance (0.0–1.0) of this drop occurring. `1.0` = always, `0.5` = 50 % chance. |

Example (goblin):

```yaml
loot:
  corpse_despawn_seconds: 60
  drops:
    - type_id: gold_coin
      quantity: uniform(3, 8)
      probability: 1.0
    - type_id: apple
      quantity: 1
      probability: 0.4
    - type_id: leather_armor
      quantity: 1
      probability: 0.05
```

To use a custom corpse sprite, define a separate object (e.g. `goblin_corpse`) that `extends: corpse` with its own `render.sprite_path`, then set `corpse_type_id: goblin_corpse` in the loot block.

The `corpse` base type (in `assets/object_bases/corpse.yaml`) extends `static_world` and provides a 20-slot container. It defaults to the generic skull-and-bones sprite (`overworld_objects/generic_corpse/sprite.png`).

Map instance using the generic scroll:

```yaml
- type: scroll
  placement: { x: 30, y: 12 }
  properties:
    spell_id: spark_bolt
```

Base definition example:

```yaml
extends: static_world
movable: true
storable: true
render:
  z_index: 0.24
```

### Pouch base

`assets/object_bases/pouch.yaml` is a small, carryable container. It extends
`pickup` (so it inherits `storable: true`, `movable: true`,
`colliding: false`) and adds:

```yaml
extends: pickup
container_capacity: 4
accepts_storable_containers: false
```

A pouch is **the only kind of container that can itself sit in another
container's slot**. Backpack and chest extend `static_world` (which sets
`storable: false`) so they can never live inside a chest. The
`accepts_storable_containers: false` flag on the pouch base then forbids
pouches-in-pouches at placement time, capping nesting at depth 1.

Pouches preserve their contents through pickup/drop. Internally, the
container slots ride on the inventory stack as `contained_slots` while the
pouch is in inventory and round-trip back onto a fresh world entity when
the pouch is dropped.

Specific pouches in `assets/overworld_objects/`:

- `small_pouch` — 4 slots, `weight: 0.5`
- `herb_pouch` — 6 slots, `weight: 0.6` (`container_capacity: 6` overrides
  the base)

### `npc_behavior` (NPC templates only)

Intrinsic per-mob speed/aggro values copied onto each spawned NPC's
`RoamingBehavior` / `HostileBehavior` components. The presence of this block
is what marks a template as an NPC — the editor's Mobs palette and the spawn
factory both treat `Some(_)` here as the "this is a mob" signal.

```yaml
npc_behavior:
  step_interval_seconds: 1.0    # required; AI tick cadence (seconds)
  step_interval_jitter_seconds: 0.0   # optional; randomized extra delay
  idle_pause_chance: 0.3        # optional; per-step pause probability
  momentum_bias: 0.6            # optional; tendency to keep walking same way
  detect_distance_tiles: 7      # required; aggro acquisition range
  disengage_distance_tiles: 11  # required; leash radius
  alert_duration_seconds: 4.0   # optional; how long Alert state lingers
  requires_line_of_sight: true  # optional; Bresenham LoS gate for aggro
```

Roam bounds and the hostile/passive toggle live on the map's spawn group, not
here — see `spawn_groups` under section 1.

### `barks` (NPC templates only)

Lists of short utterances the NPC may emit as floating speech bubbles over
its sprite. `aggro` entries fire on the Wander → Pursue transition when the
NPC first sees a player; `mutter` entries fire on a low-probability per-tick
roll while wandering. Both lists are optional; an NPC with neither stays
silent. A per-NPC cooldown (8 seconds) limits any given mob to one bubble
at a time. ASCII-only — the default font can't render symbols/emoji.

```yaml
barks:
  aggro:
    - "Grraah!"
    - "Skin and bones!"
  mutter:
    - "*grumbles*"
    - "Hungry..."
```

### `spellcasting` (NPC templates only)

NPCs with this block become spell casters. During each combat turn the AI
walks `spells` in **declaration order** and casts the first entry whose
cooldown is ready and whose conditions all pass; if none match, the NPC
falls through to its physical `attack_profile`. First-match wins, so authors
order the list highest-priority first (heal > CC > nuke > filler).

```yaml
spellcasting:
  spells:
    - spell_id: goblin_heal
      cooldown_seconds: 25.0
      target: self
      conditions:
        - !self_hp_below_fraction 0.4
    - spell_id: sleep
      cooldown_seconds: 18.0
      target: target
      conditions:
        - !target_within_range 6
        - !target_without_effect sleep
        - !target_without_effect paralyze
    - spell_id: fireball_minor
      cooldown_seconds: 9.0
      target: target_tile
    - spell_id: magic_dart
      cooldown_seconds: 3.0
      target: target
      conditions:
        - !target_within_range 7
```

Per-entry fields:

- `spell_id` (required) — id of the spell file under `assets/spells/`.
- `cooldown_seconds` (required) — seconds between successful casts of this
  entry. The NPC's first turn always satisfies the cooldown.
- `target` (optional, default `target`) — one of `target` (single-target
  spell on the combat target), `target_tile` (AoE / tile-target spell
  centered on the target's tile), `self` (untargeted spell, e.g. self-heal).
- `conditions` (optional) — list of YAML-tagged predicates that all must
  pass for this entry to fire. Available conditions:
  - `!target_within_range <tiles>` — Chebyshev distance to target ≤ N.
  - `!target_visible <bool>` — accepted but currently a no-op (the AI tick
    already gates `CombatTarget` on Bresenham LoS).
  - `!target_hp_below_fraction <0..1>` — target HP / max ≤ fraction.
  - `!self_hp_below_fraction <0..1>` — caster HP / max ≤ fraction.
  - `!target_without_effect <effect_kind>` — target has **no** active
    effect of this kind (`sleep`, `paralyze`, `burning`, ...).
  - `!self_without_effect <effect_kind>` — caster has no active effect of
    this kind on itself.

NPC casts bypass the player's class/level gate and mana cost — design
constraints come from cooldowns and conditions only. The spell's authored
damage, AoE, buffs, and VFX all apply identically to player casts;
`buffs_target` lands on the combat target's `MagicEffects` and
`buffs_self` / `restore_health` land on the NPC. Tile-target AoE damage is
intentionally narrowed to the actual `target_entity` so friendly fire on
nearby NPCs doesn't surprise authors (per-tile VFX still play over the full
radius).

## 3. Spell Definition YAML

Path:
- `assets/spells/*.yaml`

Purpose:
- Defines castable spell data referenced by usable items such as scrolls.

Top-level fields:

### `name`
- Type: string
- Meaning: display name of the spell

### `incantation`
- Type: string
- Meaning: spoken words displayed in chat when the spell is cast

### `mana_cost`
- Type: float
- Meaning: mana spent by the caster

### `targeting`
- Type: string
- Meaning: whether the spell needs a selected target
- Valid values:
  - `untargeted` — casts on the caster's own tile / self.
  - `targeted` — player picks an *entity*; range is checked against the
    entity's tile. Used by single-target spells (`spark_bolt`, `frost_lance`).
  - `targeted_tile` — player picks a *tile* (entity optional); range is
    checked against the caster's own tile. Used by AoE spells and
    pattern-summon spells (`fireball`, `firewall`).
  - `targeted_item` — player picks one of their *own inventory/equipment
    items*; the only effect honored is `enchant_item`. Used by weapon-enchant
    spells (`flame_weapon`, `empower_weapon`).

### `range_tiles`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: maximum Chebyshev distance for `targeted` and `targeted_tile`
  spells.

### `class_access`
- Type: array of class enum values (`Fighter`, `Wizard`, `Cleric`, `Vagabond`)
- Optional: yes
- Default: empty list (any class may cast)
- Meaning: classes permitted to cast this spell directly. Bypassed by
  scroll-shaped items (any item carrying a `spell_id` field today is treated as
  a scroll). Use this to gate spells that will be added to a future
  memorized-spell-cast flow.

### `min_caster_level`
- Type: integer
- Optional: yes
- Default: `0` (anyone)
- Meaning: minimum caster level required. Always enforced — applies to both
  scroll casts and direct casts.

### `effects`
- Type: mapping
- Optional: yes
- Default: empty mapping
- Meaning: immediate spell effects

`effects` fields:

### `damage`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: instant damage dealt to the target

### `damage_type`
- Type: one of `blunt`, `cut`, `pierce`, `fire`, `frost`, `earth`, `lightning`, `poison`, `acid`, `death`, `holy`, `arcane`
- Optional: yes
- Default: `arcane` (only relevant when `damage > 0`)
- Meaning: damage type tag for the spell's damage. Shown in the cast log (e.g. `Cast Frost Lance on Goblin (frost damage).`); no resistance math is applied yet. Heal/buff spells with `damage: 0.0` can omit this field.

### `restore_health`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: health restored by the spell

### `restore_mana`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: mana restored by the spell

### `buffs_self`
- Type: array of `EffectSpec`
- Optional: yes
- Default: empty list
- Meaning: timed magical effects applied to the caster. Each entry is `{ kind,
  magnitude, seconds, secondary_magnitude? }`. Most kinds upsert on the
  caster's `MagicEffects` — re-applying refreshes duration and keeps the
  stronger magnitude (smaller magnitude for `haste` since lower = faster).
  The stacking kinds (`paralyze`, `chill`, `burning`, `poisoned`, `drunk`)
  always append a new independent entry instead. `secondary_magnitude` is
  optional and only consulted by `chill` (slow multiplier).

### `buffs_target`
- Type: array of `EffectSpec`
- Optional: yes
- Default: empty list
- Meaning: timed magical effects applied to the targeted NPC (ignored for
  `untargeted` spells). Same merge rules as `buffs_self`. `MagicEffects` is
  lazily attached to NPCs that don't already carry it.

### `clears_self`
- Type: array of `EffectKind`
- Optional: yes
- Default: empty list
- Meaning: effect kinds removed from the caster after the other effects
  apply. Drives Cleric "Restore" clearing `slow` / `sleep`.

### `spawns_object`
- Type: mapping `{ type_id: string, lifetime_seconds: float, pattern?: enum, attribute_to_caster?: bool }`
- Optional: yes
- Meaning: spawn one or more transient world objects at the cast location
  (caster's tile for `untargeted`, target tile for `targeted` / `targeted_tile`).
  Every spawned entity carries a `Ttl` (generic time-to-live) and despawns when
  it elapses. The referenced `type_id` must exist in `assets/overworld_objects/`.
- `pattern` (optional, default `single`):
  - `single`: spawn one entity at the center tile (legacy behavior).
  - `perpendicular_line_3`: spawn three entities in a straight line
    perpendicular to the caster→target axis, centered on the target tile.
    Tiebreakers: caster directly N/S of target → E↔W wall; caster directly
    E/W → N↔S wall; diagonal → cross-diagonal wall.
- `attribute_to_caster` (optional, default `false`): when `true`, every
  spawned entity gets a `HazardOwner(caster_id)` component so any damage or
  DoT it produces via `on_stepped` credits the caster via
  `DamageSource::OwnedByPlayer` (XP flows back to the placer on kill). Leave
  `false` for non-hazard summons (e.g. `magic_light`).

### `aoe`
- Type: mapping `{ radius_tiles: int, vfx_on_tile?: string }`
- Optional: yes
- Meaning: only meaningful for `targeted_tile` spells. After the regular
  spell effects resolve, the spell's `damage` value is dealt as a one-shot
  hit to every entity (NPC or player) within `radius_tiles` Chebyshev
  distance of the target tile. The caster is **not** excluded — friendly
  fire is on. `buffs_target` debuffs also fan out to every NPC in radius.
- `vfx_on_tile` (optional): VFX definition id played once on **every** tile
  in the AoE square, regardless of whether an entity occupies it. Use for
  explosion-style spells where the floor itself should flash (e.g.
  `fire_hit` for fireball). Distinct from `vfx_on_target_hit`, which only
  plays on entities that actually take damage.

### `vfx_on_cast`
- Type: string
- Optional: yes
- Default: `cast_flash`
- Meaning: VFX definition id (under `assets/vfx/`) played at the caster's tile
  when this spell is cast. Override per-spell to give specific spells unique
  cast looks (e.g. a frost spell can override with a blue variant).

### `vfx_on_target_hit`
- Type: string
- Optional: yes
- Default: `hit_flash` (damaging spells); set explicitly for healing or status spells (e.g. `heal_sparkle`)
- Meaning: VFX definition id played on the target object when a targeted spell
  resolves. Untargeted spells do not trigger this.

### `enchant_item`
- Type: mapping ([`ItemModifier`](#item-modifiers-enchantments))
- Optional: yes
- Default: none
- Meaning: only honored for `targeting: targeted_item` spells. Applies this
  modifier to the item the caster picks. Routed through the same anti-stack
  rule as item-granted modifiers. Mana is spent and any scroll consumed only
  when the enchantment actually applies.

`EffectKind` values (used in `buffs_self`, `buffs_target`, and `clears_self`):

| Kind | Magnitude semantics | Notes |
|---|---|---|
| `glimmer` | tile radius of the caster's halo | Client overrides the player's `LightSource` while active. |
| `haste` | step-interval multiplier (e.g. `0.7`) | Lower = faster. Self-buff. |
| `shield` | flat AC bonus | Tracked for Phase B combat math — currently a no-op vs incoming damage (auto-hit combat). |
| `bless` | flat to-hit bonus | Same Phase B status as `shield`. |
| `slow` | step-interval multiplier (e.g. `2.0`) | Higher = slower. Target-only. |
| `sleep` | unused (`0.0` ok) | Presence skips the NPC's AI tick; cleared on incoming damage. |
| `paralyze` | unused (`0.0` ok) | Blocks movement and spellcasting. Unlike `sleep`, damage does **not** clear it — only the timer expires it. Stacks. |
| `chill` | DOT (frost damage) per tick (`1s` cadence) | Pairs with `secondary_magnitude` to also slow NPC movement (multiplier, e.g. `1.5`). Both halves are optional — omit `secondary_magnitude` for pure DOT. Stacks. |
| `burning` | DOT (fire damage) per tick (`1s` cadence) | Stacks. |
| `poisoned` | DOT (poison damage) per tick (`1s` cadence) | Stacks. |
| `drunk` | deviation probability in `[0, 1]` | Each player move command has this chance to fumble ±45° to an adjacent direction (and pay a small cooldown penalty). NPCs ignore Drunk for now. Stacks (probabilities combine via the complement rule). |

Example (utility spell with a self-buff):

```yaml
name: Glimmer
incantation: Lux Minima
mana_cost: 2.0
targeting: untargeted
class_access: [Wizard, Cleric]
min_caster_level: 1
effects:
  buffs_self:
    - kind: glimmer
      magnitude: 4.0
      seconds: 600.0
```

Example (damage + debuff):

```yaml
name: Frost Lance
incantation: Frigus Hasta
mana_cost: 16.0
targeting: targeted
range_tiles: 6
class_access: [Wizard]
min_caster_level: 3
effects:
  damage: 7.0
  buffs_target:
    - kind: slow
      magnitude: 2.0
      seconds: 3.0
```

Example (object-spawning utility):

```yaml
name: Light
incantation: Lux
mana_cost: 2.0
targeting: untargeted
class_access: [Wizard, Cleric]
min_caster_level: 1
effects:
  spawns_object:
    type_id: magic_light
    lifetime_seconds: 1800.0
```

Example (minimal baseline form — back-compat with pre-batch spells):

```yaml
name: Spark Bolt
incantation: Exori Vis
mana_cost: 12.0
targeting: targeted
range_tiles: 5
class_access: [Wizard]
min_caster_level: 1
effects:
  damage: 18.0
```

## Item modifiers (enchantments)

An `ItemModifier` is a per-instance enchantment attached to a specific item
(not the shared definition). It is referenced by a spell's
[`enchant_item`](#enchant_item) and a consumable's
[`grants_item_modifier`](#grants_item_modifier). Modifiers live on the item
instance, persist with the character save, and (for the wielder-stat and
on-hit kinds) take effect while the item is equipped.

Fields:
- `type_ex` (string, required): exclusivity group. Anti-stack is scoped per
  item per `type_ex` — at most one modifier of a given `type_ex` survives on an
  item.
- `lvl` (int, required): rank within the `type_ex` group. Applying a **stronger**
  `lvl` overrides a weaker one; a **weaker** `lvl` is rejected (can't worsen a
  `+2` with a `+1`); an **equal** `lvl` refreshes the duration without stacking.
- `label` (string, optional): player-facing name shown in chat and the item
  tooltip (e.g. `"Flaming (+1d6 fire)"`).
- `effect` (mapping, required): internally tagged by `kind`:
  - `kind: bonus_damage` — extra damage dealt on every hit as its own
    `DamageEvent` (own number + element VFX). Fields: `dice` (optional `[count,
    sides]`, e.g. `[1, 6]`), `bonus` (optional int), `damage_type` (a
    `DamageType`, e.g. `fire`).
  - `kind: on_hit` — chance to apply a magical effect to the struck target.
    Fields: `chance` (float `0.0..=1.0`), `spec` (an `EffectSpec`: `{ kind,
    magnitude, seconds, secondary_magnitude? }`).
  - `kind: wielder_stats` — flat bonus to the wielder while the item is
    equipped. Fields: `attributes` (a full `AttributeSet` — all six of
    `strength`/`agility`/`constitution`/`willpower`/`charisma`/`focus`),
    `armor` (int), `dodge_bonus` (int).
- `duration` (mapping, required): internally tagged by `kind`:
  - `kind: permanent`
  - `kind: timed` with `remaining_seconds` (float). Counts down only while the
    item is equipped, in whole-second steps.
  - `kind: charges` with `remaining` (int). One charge is spent per **successful
    application** (e.g. each landed poison), and the modifier is removed at zero.

Example (a `targeted_item` enchant spell — `flame_weapon`):

```yaml
name: Flame Weapon
incantation: Ignis Acies
mana_cost: 14.0
targeting: targeted_item
effects:
  enchant_item:
    type_ex: weapon_elemental
    lvl: 1
    label: "Flaming (+1d6 fire)"
    effect:
      kind: bonus_damage
      dice: [1, 6]
      bonus: 0
      damage_type: fire
    duration:
      kind: timed
      remaining_seconds: 120.0
```

Example (a consumable that coats a weapon — `poison_flask`):

```yaml
use_effects:
  grants_item_modifier:
    type_ex: weapon_coating
    lvl: 1
    label: "Poisoned (20 hits)"
    effect:
      kind: on_hit
      chance: 1.0
      spec:
        kind: poisoned
        magnitude: 3.0
        seconds: 6.0
    duration:
      kind: charges
      remaining: 20
```

## 4. Floor Tileset Metadata YAML

Path:
- `assets/floors/<floor_id>/metadata.yaml`

Current examples:
- `assets/floors/grass/metadata.yaml`
- `assets/floors/cobblestone/metadata.yaml`
- `assets/floors/cave_floor/metadata.yaml`
- `assets/floors/dirt_path/metadata.yaml`
- `assets/floors/sand/metadata.yaml`

Purpose:
- Defines a ground-floor tileset (grass, cobblestone, cave floor, …).
- The directory name is the floor tileset id used by `fill_floor_type` and the map's `floors` overlay.
- A floor with no `atlas_path` falls back to the flat `debug_color`; this is the recommended starting point when authoring new floor types before the artwork is ready.

The `transitions/` subdirectory under `assets/floors/` is special: it holds [floor transition tilesets](#5-floor-transition-metadata-yaml), not regular floor types, and is skipped by this loader.

Top-level fields:

### `id`
- Type: string
- Optional: yes
- Default: the directory name
- Meaning: stable floor tileset identifier. If present, must equal the directory name; the loader panics on a mismatch. Leave empty (`id: ""`) or omit to let the loader fill it from the directory name.

### `name`
- Type: string
- Meaning: human-readable display name (e.g. used in the editor's floor palette).

### `priority`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: rendering precedence when two floor types meet at a corner. The lower-priority floor is the *base*; the higher-priority floor is painted on top via a transition atlas. Ties break alphabetically on `id`. Authors typically use `0` for the natural background (grass, cave floor) and larger values for crafted overlays (paths, cobblestone).

### `tile_size_px`
- Type: positive integer
- Optional: yes
- Default: `16`
- Meaning: pixel size of one floor tile in the atlas. Must be greater than zero. Every floor that participates in a transition pair must agree on `tile_size_px` — the transition loader asserts this.

### `atlas_path`
- Type: string or `null`
- Optional: yes
- Default: `null` (no atlas; the floor renders as its `debug_color`)
- Meaning: Bevy asset path to the floor's tileset PNG, relative to `assets/`. The atlas is laid out as a 4×4 authoring-layout block of 16 sub-tiles (one per corner-mask `0..=15`); additional 4-row blocks below hold optional variants (see `variants`). The four columns × four rows do **not** correspond to mask values in row-major order — the renderer maps each mask to its slot via the `MASK_TO_AUTHORING_INDEX` table in `src/world/floor_render.rs`. Mirror the visual convention used by the reference tilesets in `assets/floors/`. (Legacy native-layout PNGs can be converted with `python3 scripts/tile_permutor.py --inverse <src> <dst>`.)

### `debug_color`
- Type: 3-item integer list `[r, g, b]`
- Meaning: fallback sRGB color rendered when no `atlas_path` is configured, and shown in editor previews/minimaps.

### `occludes_floor_above`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: reserved for upper-storey floors; unused at `z = 0` in the current slice. Leave at default unless you're working on multi-storey support.

### `walkable_surface`
- Type: boolean
- Optional: yes
- Default: `true`
- Meaning: reserved; the ground floor is currently always walkable. Leave at default.

### `variants`
- Type: mapping from corner-mask integer (`1..=15`) to a list of positive integer weights
- Optional: yes
- Default: empty (every corner-mask has a single deterministic variant)
- Meaning: per-bitmask sprite variation. The corner mask encodes which corners of a 2×2 cell sample this floor type:
  - `NW = 1`, `NE = 2`, `SW = 4`, `SE = 8` (bitwise OR for combinations)
  - Variant 0 occupies rows `0..=3` of the atlas (the base block); variant `i` occupies rows `4*i..=4*i+3`.
- For each authored mask, supply one weight per variant (the loader requires `1..=15` keys, non-empty weight lists, and all weights `> 0`). Higher weights make a variant more likely; the renderer samples deterministically based on tile coordinates so the picture is stable across saves.
- Bitmasks omitted from this map fall back to a single variant (weight `[1]`).

### `ripple`
- Type: mapping or omitted
- Optional: yes
- Default: omitted (no overlay animation)
- Meaning: configures a sparse Poisson-scheduled overlay animation. When set, a client-side scheduler picks a random visible tile of this floor type every `Δt ~ Exp(λ_total)` and spawns a single transient sprite that plays the strip non-looping, then despawns. Under Poisson superposition `λ_total = rate_per_tile_per_second × visible_tile_count`, so larger ponds naturally produce proportionally more events with no per-map tuning. Used for water ripples — anything that wants occasional motion without paying for a per-tile timer. Implemented in `src/world/floor_animation.rs`.

`ripple` sub-fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `sheet_path` | string | yes | Bevy asset path (relative to `assets/`) to a horizontal strip of `frame_count` cells. |
| `frame_width` | positive integer | yes | Pixel width of one frame in the strip. |
| `frame_height` | positive integer | yes | Pixel height of one frame in the strip. |
| `frame_count` | positive integer | yes | Number of frames in the strip; played left-to-right, once. |
| `fps` | positive float | yes | Frames per second; total animation duration is `frame_count / fps`. |
| `rate_per_tile_per_second` | non-negative float | yes | Mean Poisson rate per visible tile of this floor type. `0.02` works well for water (one event every few seconds across a modest pond). |
| `z_offset` | float | no (default `0.00001`) | Z bump above the floor cell so the ripple draws on top of the floor sprite but below objects/players. |

Example (water with a 4-frame ripple strip):

```yaml
id: water
name: Water
priority: 5
tile_size_px: 16
atlas_path: floors/water/tileset.png
debug_color: [41, 97, 189]
ripple:
  sheet_path: floors/water/ripple.png
  frame_width: 16
  frame_height: 16
  frame_count: 4
  fps: 8
  rate_per_tile_per_second: 0.02
  z_offset: 0.0005
```

Example (`assets/floors/grass/metadata.yaml`):

```yaml
id: grass
name: Grass
priority: 0
tile_size_px: 16
atlas_path: floors/grass/tileset.png
debug_color: [47, 76, 43]
```

Example (no atlas, debug-colour-only):

```yaml
id: cobblestone
name: Cobblestone
priority: 30
tile_size_px: 16
debug_color: [148, 142, 134]
```

Example with variant weights (giving the fully-grass tile two equally-weighted shuffle variants and one rare flowery variant):

```yaml
id: grass
name: Grass
priority: 0
tile_size_px: 16
atlas_path: floors/grass/tileset.png
debug_color: [47, 76, 43]
variants:
  15: [3, 3, 1]   # NW|NE|SW|SE — the all-grass corner case
```

Notes:
- Validation is strict: directory name must match `id`, `tile_size_px > 0`, every `variants` key in `1..=15`, every weight list non-empty with `> 0` weights. Failures panic at load.
- Floor tilesets are loaded from every scan dir returned by `AssetResolver` (bundled `assets/` plus any synced asset cache for TCP clients).
- The transition atlas between two floors is authored separately under `assets/floors/transitions/`; defining a new floor type does not by itself produce a transition with any other floor.

### Flavors (runtime-derived)

- Every floor tileset automatically gains one or more **flavors** — programmatic treatments of its base art generated in memory at load (no YAML authoring, no extra files). The map editor's floor palette has a **Flavor** toggle that applies the selected flavor to whatever tileset the floor brush paints.
- A flavored floor is addressed by the derived id `"<base>#<flavor>"` (e.g. `cave_floor#flooring`). These ids are saved into map files like any other floor id and resolve back to their base definition's metadata at load; only the atlas image differs. Because `#` can never appear in an on-disk floor id (it must equal its directory name), derived ids never collide with authored ones.
- The first flavor, `flooring`, makes each rendered floor tile a solid square of the floor's interior texture that fills its grid cell exactly, so it meets the full-tile wall footprints flush (no overhang, no gap). Each authoring tile's mask-set quadrants are filled from the variant block's solid interior tile (mask `0xF`) — from its *opposite* quadrant, since the dual-grid draws each corner cell point-reflected — and unset quadrants are cleared. Implementation: `src/world/floor_flavors.rs` (`FloorFlavor`, `apply_flooring`, `generate_floor_flavor_atlases`).

## 5. Floor Transition Metadata YAML

Path:
- `assets/floors/transitions/<low>__<high>/metadata.yaml`

Current example:
- `assets/floors/transitions/grass__cobblestone/metadata.yaml`

Purpose:
- Describes how two adjacent floor types blend at a shared corner.
- The directory name encodes the canonical pair `<low>__<high>` (double underscore separator). `low` is the lower-priority floor (alphabetical id tiebreak on equal priority); `high` is the floor painted on top of `low`'s base. The loader asserts that the directory name and the metadata's `low`/`high` fields agree.
- The atlas paints `high`-side pixels with a feathered border onto a solid `low` base, indexed by the **high-side** corner bitmask (bits set where the high floor sits in a 2×2 corner cell).

Top-level fields:

### `low`
- Type: string
- Meaning: id of the lower-priority floor (the base painted underneath). Must exist as a regular floor under `assets/floors/`.

### `high`
- Type: string
- Meaning: id of the higher-priority floor (the overlay painted with feathered borders). Must exist as a regular floor under `assets/floors/`. Must differ from `low`.

### `tile_size_px`
- Type: positive integer
- Optional: yes
- Default: `16`
- Meaning: pixel size of one transition tile in the atlas. Must equal both endpoints' `tile_size_px` — the loader panics otherwise.

### `atlas_path`
- Type: string
- Meaning: Bevy asset path to the transition tileset PNG, relative to `assets/`. Same authoring 4×4 layout as a regular floor atlas (see the floor `atlas_path` section above); additional 4-row blocks are read as variants when `variants` lists them.

### `variants`
- Type: mapping from corner-mask integer (`1..=15`) to a list of positive integer weights
- Optional: yes
- Default: empty (single variant per mask)
- Meaning: identical to a floor tileset's `variants`, except keys index the **high-side** corner bitmask — bits set where the `high` floor sits in the 2×2 corner cell. Same validation rules: keys in `1..=15`, non-empty weight lists, all weights `> 0`.

Example (`assets/floors/transitions/grass__cobblestone/metadata.yaml`):

```yaml
low: grass
high: cobblestone
tile_size_px: 16
atlas_path: floors/transitions/grass__cobblestone/tileset.png
```

Notes:
- The directory name is *significant*: `grass__cobblestone/` means the pair is `(low=grass, high=cobblestone)`. Reversing the pair is a load-time error; the loader picks one canonical direction (priority asc, id alphabetical tiebreak) so each pair is authored exactly once.
- Transition lookup at runtime is order-insensitive (`transition_for("grass", "cobblestone")` and `transition_for("cobblestone", "grass")` both resolve to the same definition).
- A pair with no transition file falls back to a hard seam between the two floors.
- Like floor tilesets, transitions are loaded from every `AssetResolver` scan dir.

## 6. VFX Definition YAML

Path:
- `assets/vfx/<id>/metadata.yaml` (one directory per effect, sprite sheet sits next to the YAML — typically `sheet.png`)

Purpose:
- Declares a reusable visual effect (one-shot transient or sticky overlay) that
  any server system can address by id via `GameUiEvent::VfxSpawn` or that the
  client attaches automatically when a matching `EffectKind` is active on the
  local player.

Top-level fields:

### `animation`
- Type: `AnimationSheetDef` (same struct used by overworld objects)
- Meaning: sprite-sheet animation. The sheet **must contain a clip named
  `play`**. One-shot effects set `play.looping: false` so the frame cycler
  holds the final frame until `Ttl` despawns the entity; sticky overlays set
  `play.looping: true`.

### `duration_seconds`
- Type: float
- Optional: yes
- Default: `frame_count / fps` of the `play` clip (falls back to `0.5` if those are missing)
- Meaning: how long the one-shot effect lives before despawn. Ignored for
  sticky overlays (which have no `Ttl` and live as long as their backing
  `EffectKind` is active on the player).

### `scale`
- Type: float
- Optional: yes
- Default: `1.0`
- Meaning: multiplier on the rendered sprite size relative to the native
  `frame_width` × `frame_height`.

### `z_offset_pixels`
- Type: float
- Optional: yes
- Default: `0.0`
- Meaning: reserved for future use (rendering effects offset upward from the
  target's bottom-anchor). The current spawner renders centered on the
  target's tile.

### `looping`
- Type: bool
- Optional: yes
- Default: `false`
- Meaning: marks the effect as a sticky overlay (no `Ttl`, the `play` clip
  loops). Sticky overlays are *not* spawned via `VfxSpawn`; they are spawned
  by the client whenever the local player gains a matching `EffectKind`.

### Sticky-overlay mapping

The client attaches sticky overlays to the local player based on
`ClientGameState.active_effects`. The current `EffectKind` → VFX id map is:

| `EffectKind` | VFX definition id |
|---|---|
| `glimmer` | `glimmer_aura` |
| `haste` | `haste_streaks` |
| `shield` | `shield_bubble` |
| `bless` | `bless_aura` |
| `slow` | `slow_drag` |
| `sleep` | `sleep_zs` |

Definitions not in the map are silently ignored — adding a new `EffectKind`
requires a code change in `src/client_effects/vfx_attachment.rs::definition_id_for_effect`.

### Trigger sites for one-shot effects

| Trigger | Default id | Override field | File |
|---|---|---|---|
| Melee/ranged hit on target | `blood_splash` | `AttackProfileDef.hit_vfx` (under the attacker's overworld object metadata) | `src/combat/systems.rs` |
| Spell cast | `cast_flash` | `SpellEffects.vfx_on_cast` | `src/game/systems.rs` |
| Spell impact on target | `hit_flash` | `SpellEffects.vfx_on_target_hit` | `src/game/systems.rs` |
| NPC death | `death_poof` | (none yet) | `src/combat/systems.rs` |

Example (one-shot):

```yaml
animation:
  sheet_path: vfx/blood_splash/sheet.png
  frame_width: 48
  frame_height: 48
  sheet_columns: 6
  sheet_rows: 1
  clips:
    play:
      row: 0
      start_col: 0
      frame_count: 6
      fps: 16.0
      looping: false
duration_seconds: 0.4
```

Example (sticky overlay):

```yaml
animation:
  sheet_path: vfx/shield_bubble/sheet.png
  frame_width: 48
  frame_height: 48
  sheet_columns: 4
  sheet_rows: 1
  clips:
    play:
      row: 0
      start_col: 0
      frame_count: 4
      fps: 4.0
      looping: true
looping: true
```

Notes:
- VFX entities are presentation-only (`ViewPosition` + `WorldVisual`, no `SpaceResident` / `TilePosition`). They never affect simulation.
- `VfxSpawn` is a `GameUiEvent`, broadcast like `ProjectileFired`. Missing definition ids are skipped silently rather than crashing — useful when adding new triggers ahead of art.
- Definitions are reloaded on the same `OnEnter(ClientAppState::InGame)` pass that reloads object/spell/floor definitions, so editing a VFX YAML and re-entering the world picks it up without restarting.

### `AttackProfileDef.hit_vfx`

When an overworld object's metadata declares an `attack_profile:` block, it
may set an optional `hit_vfx` field to override the default `blood_splash`
played on hits landed by that attacker. Useful for elementals (e.g. a fire
imp could set `hit_vfx: ember_burst`) and for non-flesh creatures
(`stone_chunks`, `electric_arc`, …).

```yaml
attack_profile:
  kind: melee
  hit_vfx: ember_burst
```

## 7. Locks, keys, and skill-gated interactions

Stateful objects (doors, chests) can declare a `lock:` block plus
interactions whose `from: [locked]` requires either a skill check or a
matching inventory key. The state machine remains the same `from`/`to`
transition path — the gates are evaluated server-side before the
transition fires.

```yaml
states:
  locked: { sprite_path: ..., colliding: true }
  closed: { sprite_path: ... }
  open:   { sprite_path: ..., colliding: false }
initial_state: closed
lock:
  lock_id: 7
  pick_dc: 15
  force_dc: 18
interactions:
  - verb: pick_lock
    label: Pick Lock
    from: [locked]
    to: closed
    skill_gate: { skill: Thievery, dc: from_lock_pick }
  - verb: force_lock
    label: Force Lock
    from: [locked]
    to: closed
    skill_gate: { skill: Athletics, dc: from_lock_force }
  - verb: use_key
    label: Use Key
    from: [locked]
    to: closed
    key_gate: { source: from_lock }
  - verb: open
    from: [closed]
    to: open
  - verb: close
    from: [open]
    to: closed
```

- `skill_gate.dc` accepts `from_lock_pick`, `from_lock_force`, or a literal
  integer via `{ Fixed: 15 }`. Skill ranks come from the actor's
  `SkillSheet`; an `ability_mod` of the keyed attribute is added.
- `key_gate.source` accepts `from_lock` (reads `lock.lock_id`) or
  `{ Fixed: 12 }`. The server walks the actor's backpack + equipment
  slots for any item whose definition has a matching top-level
  `lock_id`. Define keys like:

  ```yaml
  extends: pickup
  name: Iron Key
  lock_id: 7
  ```

The context menu hides skill-gated verbs when the actor's rank in that
skill is 0, and hides `use_key` when no matching key is in the actor's
inventory.

## 8. Dialog custom commands

Yarn `.yarn` files may invoke project-specific custom commands beyond
`<<set>>` / `<<if>>` / variable storage:

| Command | Effect |
|---|---|
| `<<give_item "type_id" count>>` | Add `count` of `type_id` to the speaker's backpack. |
| `<<take_item "type_id" count>>` | Remove up to `count` of `type_id` from the speaker's backpack. |
| `<<give_recipe "recipe_id">>` | Mark `recipe_id` as learned. |
| `<<stash_set key value>>` / `<<stash_delete key>>` | Mutate the per-character stash. |
| `<<start_quest "quest_id">>` / `<<complete_quest "quest_id">>` | Quest state. |
| `<<skill_check Skill DC>>` | Roll `Skill` against `DC` for the speaker. Writes `$last_skill_check_success` (bool) and `$last_skill_check_total` (number) into the player's Yarn variable store. Next branch reads via `<<if $last_skill_check_success>>`. |

Library functions (read-only, callable from `<<if …>>` expressions):

| Function | Returns |
|---|---|
| `has_item("type_id", count)` | `true` when backpack has at least `count` of `type_id`. |
| `stash_has(key)` / `stash_get_str(key)` / `stash_get_num(key)` / `stash_get_bool(key)` | Stash readers. |
| `skill_rank("SkillName")` | Speaker's current rank in `SkillName` (or 0). Name is case-insensitive: `"Persuasion"`, `"thievery"`, etc. |

The set of allowed skills mirrors `docs/progression.md §5`: `Athletics`,
`Stealth`, `Perception`, `Lore`, `Spellcraft`, `Persuasion`, `Survival`,
`Heal`, `Thievery`, `Concentration`.

## 9. Starting Loadout YAML

Path:
- `assets/loadouts/starter.yaml`

Purpose:
- Defines the inventory a brand-new character is granted on first login. Loaded
  server-side into the `StartingLoadout` resource (`src/player/loadout.rs`) and
  applied by `StartingLoadout::apply_to` when a fresh character is seeded
  (embedded mode in `spawn_embedded_player_authoritative`, TCP in
  `handle_select_character`). The file stem **must** be `starter` — the loader
  panics at startup if no `starter.yaml` is found under any `loadouts` asset dir.

Top-level fields (both optional, default to empty):

### `equipment`
- Type: list of equipment entries
- Meaning: items the character starts wearing. Each entry:
  - `slot` (string, required): which slot to fill — one of `amulet`, `helmet`,
    `weapon`, `armor`, `shield`, `legs`, `backpack`, `ring`, `boots`, `ammo`
    (snake_case, matching `EquipmentSlot`).
  - `type_id` (string, required): the item type — a directory name under
    `assets/overworld_objects/`.
  - `quantity` (int, optional, default `1`): only meaningful for `slot: ammo`,
    where it becomes the ammo stack count. Ignored for single-item slots.
  - `properties` (map of string→string, optional): per-instance properties
    (e.g. a scroll's `spell_id`).
  - `modifiers` (list, optional): per-instance enchantments — see
    [Item modifiers](#item-modifiers-enchantments). Lets starting gear ship
    pre-buffed (e.g. a flaming starter sword) entirely from data.

### `backpack`
- Type: list of item entries
- Meaning: items dropped into the first free backpack slots, in order. Each
  entry takes `type_id`, `quantity` (default `1`), `properties`, and `modifiers`
  with the same meaning as an `equipment` entry (minus `slot`). Entries that
  find no free backpack slot are warned and dropped.

Example:

```yaml
equipment:
  - slot: weapon
    type_id: bow
  - slot: ammo
    type_id: arrow
    quantity: 20
backpack:
  - type_id: apple
    quantity: 3
  - type_id: bronze_sword
    modifiers:
      - type_ex: flaming
        lvl: 1
        label: "Flaming (+1d6 fire)"
        effect:
          kind: bonus_damage
          dice: [1, 6]
          damage_type: fire
        duration:
          kind: permanent
```
