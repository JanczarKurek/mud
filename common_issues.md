# Common Issues and Root Causes

## Only player sprite visible in offline (EmbeddedClient) mode

**Symptom**: When running in embedded/offline mode, only the player sprite renders. Ground tiles and world objects are absent.

**Root cause**: `spawn_ground_tiles_for_current_space` used `world_config.is_changed()` to guard against per-frame re-spawning. Bevy's change detection for systems that have never run before has ambiguous `last_run` tick semantics — on first entry to `InGame` state, the system may not see `WorldConfig` as changed even though it was freshly written.

**Root cause (confirmed)**: `collect_game_events_from_authority` uses `player_query.single()` to drive all client-state events (player position, space, world objects). When it fails (wrong entity count), `current_space` stays `None` and `sync_client_world_projection` early-returns forever. The failure mode: when the TCP server runs and saves its state after all clients disconnect, it writes `players: []`. Offline mode loads this snapshot, sets `snapshot_status.loaded = true`, `spawn_embedded_player_authoritative` returns early (snapshot was loaded), and leaves zero player entities in the ECS.

**Fix**: Added `players_restored` flag to `WorldSnapshotStatus`. `spawn_embedded_player_authoritative` now only skips spawning if the snapshot both loaded AND had player entities. An empty-players snapshot falls through and spawns the local player.

**Secondary architectural fix**: `GameServerPlugin` registered `apply_game_events_to_client_state` with `.run_if(simulation_active)` while `GameClientPlugin` registered it unconditionally. `WorldClientPlugin` uses `.after(apply_game_events_to_client_state)` — this ordering constraint must resolve identically in both modes. Fixed by removing the `run_if` from the server-side registration.

**Fix 1**: `GameServerPlugin` now registers `apply_game_events_to_client_state` unconditionally (identical to `GameClientPlugin`). The server-only simulation systems (`process_game_commands`, `collect_game_events_from_authority`) remain gated by `run_if(simulation_active)`. When simulation is inactive the events buffer is empty so the apply pass is a no-op.

**Fix 2**: Replaced `is_changed()` in `spawn_ground_tiles_for_current_space` with explicit config tracking via `GroundTileConfig` resource. This makes tile spawning independent of Bevy's change detection tick initialization.

**Files changed**: `src/game/mod.rs`, `src/world/resources.rs`, `src/world/setup.rs`, `src/world/mod.rs`

---

## Mob movement only worked for some NPC types (e.g. only goblins moved)

**Symptom**: After deleting a save file and starting fresh, most NPCs stood still.

**Root cause**: Anonymous YAML map object entries (using `placement: [...]` list) cannot carry a `behavior:` field — they don't get individual IDs. Only NPCs defined as explicit objects (with `id:` and `behavior:`) actually got `RoamingBehavior` / `HostileBehavior` components attached.

**Fix**: Convert anonymous mob entries in `assets/maps/overworld.yaml` to explicit entries with stable IDs and `behavior:` blocks.

---

## Jagged player movement in TCP/online mode (snaps then lerps)

**Symptom**: Player movement first snaps to the new tile, then the smooth lerp plays in reverse.

**Root cause**: `sync_tile_transforms` ran without ordering relative to `detect_player_movement`. On frames where the player moved, `tick_view_scroll` set `view_scroll.current` to the full tile offset, but `sync_tile_transforms` had already positioned entities using the old (zero) scroll value, causing a one-frame snap.

**Fix**: Added `.after(detect_player_movement)` ordering to `sync_tile_transforms` in `src/world/mod.rs`.

---

## Player renders on top of large NPC sprites at same tile

**Symptom**: When the player walks to the same tile as a large NPC (e.g. cyclops), the player character appears in front instead of behind it.

**Root cause**: The `y_sort_z` function assigns the same z value to the player and any NPC at the same `tile_y`. With identical z, Bevy's render order is undefined and the player entity often wins.

**Fix**: In `sync_player_z` (`src/world/systems.rs`), subtract 0.005 (half-tile sort step) from the computed z. This makes the player sort as if they are half a tile further back, so same-row NPCs and obstacles always render in front of the player.

---

## Stale XDG cache overrides local map with anonymous (no-behavior) NPC entries

**Symptom**: NPCs that have `behavior:` blocks in `assets/maps/overworld.yaml` are stationary; other NPCs from the same file (whose entries existed before the cache was written) behave normally.

**Root cause**: `AssetResolver::scan_dirs` puts the XDG cache (`~/.local/share/mud2/assets/`) after bundled assets so the cache wins. If the map editor saves a map, `ExplicitOutput` in `src/editor/serializer.rs` previously had no `behavior` field, dropping all NPC behaviors. The stale cached YAML (with anonymous entries) then overrides the correct local YAML on every launch.

**Fix 1**: Added `behavior: Option<MapBehavior>` to `ExplicitOutput` in `src/editor/serializer.rs`, populated from `ObjectRegistry::behavior()`. Also added `behaviors: HashMap<u64, MapBehavior>` to `ObjectRegistry`, populated in `from_space_definitions`.

**Fix 2**: Copy the corrected local YAML to the XDG cache: `cp assets/maps/overworld.yaml ~/.local/share/mud2/assets/maps/overworld.yaml`.

---

## Remote player movement appears jagged

**Symptom**: Other players' sprites snap to position rather than smoothly sliding.

**Root cause**: `sync_remote_player_projection` updated `TilePosition` but did not insert `VisualOffset` / `JustMoved` components the way `sync_client_world_projection` did for world objects.

**Fix**: Added the same `VisualOffset` + `JustMoved` insertion block to `sync_remote_player_projection` in `src/world/systems.rs` (guarded by `dx.abs() <= 1 && dy.abs() <= 1` to skip teleports).

---

## NPC freezes on an upper floor — attacks only when the player is directly adjacent

**Symptom**: A hostile NPC (e.g. fire elemental) chases the player up stairs to the second floor, then stops. It attacks when the player stands right next to it, but stands still and does nothing the moment the player steps one tile farther away (distance 2) on the same floor.

**Root cause**: A line-of-sight bug, not pathfinding. The "adjacent works, distance-2 fails" threshold is the signature of an LoS gate — `has_line_of_sight` short-circuits to `true` for any ray ≤ 1 tile. Painted upper floors that occlude (the normal case — `wooden_floor`, `cave_floor` set both `occludes_floor_above` and `walkable_surface`) inserted their LoS occluder at `surface_z = floor_idx * 2`, which is *exactly the z where entities stand on that floor* (floor 1 = z=2). So any horizontal ray between two entities on the same upper floor passed through an occluding tile at z=2 and read as blocked. Combined with an index mismatch — `tick_alert` re-detected with the movement index (no occluder) while the pursue `lost_los` gate used the LoS index (occluder) — the NPC entered a detect→abort freeze loop instead of just failing to aggro.

**Fix**: In `apply_floor_layer` (`src/world/spatial.rs`), insert the floor occluder at `support_z` (= `surface_z - 1`, the between-floor half-block) instead of `surface_z`. Vertical/cross-floor rays still pass through the odd between-floor z and stay blocked; horizontal same-floor rays at the even surface z no longer hit it. Also aligned `tick_alert` (`src/npc/systems.rs`) to re-detect with `los_blockers`, matching `tick_wander` and the `lost_los` gate.

**Gotcha to remember**: entities stand on floor *N* at the **even** z `N*2`; the floor *slab/ceiling* belongs at the **odd** between-floor z `N*2 - 1`. Never put a movement/LoS blocker that represents a floor on the even surface z, or you block the entities standing on it. Regression tests: `world::spatial::tests::{floor_occluder_sits_below_the_walking_surface, same_floor_horizontal_los_is_clear_above_occluding_floor, vertical_los_through_occluding_floor_is_blocked}` and `npc::systems::tests::los_npc_pursues_across_occluding_upper_floor`.
