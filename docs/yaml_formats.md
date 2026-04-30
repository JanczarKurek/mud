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

### `fill_object_type`
- Type: string
- Meaning: object definition ID that fills every tile before explicit object instances are applied
- This should match a directory name under `assets/overworld_objects/`

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
- Type: mapping of state-name → `{sprite_path?, animation?, colliding?}`
- Optional: yes
- Meaning: per-state visual + collider overrides. Each state may override `sprite_path` (static), `animation` (atlas — same shape as `render.animation`), and/or `colliding`. Unset fields inherit from the base definition.

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
  - `untargeted`
  - `targeted`

### `range_tiles`
- Type: integer
- Optional: yes
- Default: `0`
- Meaning: maximum Chebyshev distance for targeted spells

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

Example:

```yaml
name: Spark Bolt
incantation: Exori Vis
mana_cost: 12.0
targeting: targeted
range_tiles: 5
effects:
  damage: 18.0
```
