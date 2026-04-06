# YAML Formats

This document describes the YAML formats currently used by the project.

It should be updated whenever the schema or intended meaning of these files changes.

## 1. Map Layout YAML

Path:
- `assets/maps/*.yaml`

Current example:
- `assets/maps/overworld.yaml`

Purpose:
- Describes the tile dimensions of a map.
- Defines the default fill object type for every tile.
- Defines explicit object instances with stable numeric IDs.
- Allows objects to exist on the map, inside containers, or nowhere.

Top-level fields:

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

### `objects`
- Type: list of object entries
- Meaning: all authored map objects, using either explicit instances or compact anonymous placement groups

Two object entry forms are currently valid:

### Explicit object instance
- Use this when the object needs a stable ID.
- Required for objects referenced from `contents`.
- Appropriate for containers and stateful objects.

Fields:

### `id`
- Type: integer
- Meaning: stable numeric object instance ID within the map

### `type`
- Type: string
- Meaning: object definition ID for the instance
- This should match a directory name under `assets/overworld_objects/`

### `placement`
- Type: mapping
- Optional: yes
- Meaning: where the object exists in the world, if it is currently placed on the map

### `contents`
- Type: list of integers
- Optional: yes
- Default: empty list
- Meaning: IDs of other objects stored inside this object
- Intended for container objects such as barrels

### Anonymous placement group
- Use this when you just want to place many objects of the same type and do not need to refer to them elsewhere in the map file.
- Runtime object IDs are generated automatically during map loading.

Fields:

### `type`
- Type: string
- Meaning: object definition ID for all spawned instances in the group

### `placement`
- Type: list of tile coordinate mappings
- Meaning: list of world placements for generated object instances

Placement fields:

### `x`
- Type: integer
- Meaning: tile x coordinate

### `y`
- Type: integer
- Meaning: tile y coordinate

Example:

Explicit instance example:

```yaml
- id: 300
  type: barrel
  placement: { x: 20, y: 13 }
  contents: [600, 601]
- id: 600
  type: apple
- id: 601
  type: potion
```

Anonymous placement group example:

```yaml
- type: tree
  placement:
    - { x: 6, y: 7 }
    - { x: 7, y: 7 }
    - { x: 8, y: 8 }
```

Notes:
- Each object may exist in at most one place:
  - placed in the world via `placement`
  - inside exactly one container via another object's `contents`
  - or nowhere
- Objects with no `placement` and no parent container are valid and simply start unspawned.
- Anonymous placement groups cannot be referenced by `contents` because they do not declare stable map IDs.
- Anonymous placement groups are expanded into generated object instances during map loading.
- Container contents are ordered by the list order in `contents`.
- The map loader validates duplicate IDs, missing content references, self-containment, and multi-location conflicts.
- Rendering order is controlled by object metadata `render.z_index`, not by object order in the map file.

## 2. Overworld Object Metadata YAML

Path:
- `assets/overworld_objects/<object_id>/metadata.yaml`

Purpose:
- Defines object type behavior and rendering metadata.
- The directory name acts as the object ID used in map files and runtime data.

Top-level fields:

### `name`
- Type: string
- Meaning: display name of the object

### `description`
- Type: string
- Meaning: human-readable description of the object

### `colliding`
- Type: boolean
- Meaning: whether the object blocks movement

### `collectible`
- Type: boolean
- Optional: yes
- Default: `false`
- Meaning: whether the object can be picked up and dragged into inventory/container slots

### `equipment_slot`
- Type: string or `null`
- Optional: yes
- Meaning: if present, the collectible is recognized as equippable gear for that paperdoll slot
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

Example:

```yaml
name: Barrel
description: A heavy wooden barrel that can be opened as a simple container.
colliding: true
collectible: false
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
- `collectible`, `equipment_slot`, and `container_capacity` can coexist if needed.
- If `sprite_path` is omitted or `null`, the object falls back to colored debug rendering.
- The current runtime uses these fields directly for world spawning, collision, pickup behavior, and container creation.
