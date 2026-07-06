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

---

## HUD panels rebuild every frame — never gate presentation on `ClientGameState::is_changed()`

**Symptom**: The character sheet, skills panel, recipe book (and the minimap) do a full `despawn_related::<Children>()` + rebuild every single frame, even while the player stands still. Shows up as constant per-system time in the diagnostics overlay and UI flicker / input churn.

**Root cause**: `ClientGameState` is one monolithic resource. The client fold `apply_event_to_state` (`src/game/projection.rs`) `DerefMut`s the *whole* resource whenever **any** `GameEvent` is applied, and events fire almost every frame in normal play (NPCs roaming → `WorldObjectUpserted`, vitals regen → `PlayerVitalsChanged`, the world clock → `WorldTimeChanged` + a 10s heartbeat). So `ClientGameState::is_changed()` is `true` nearly every frame and is **useless as a redraw gate** — a panel that only renders skills rebuilds every time an NPC takes a step. (Note: merely taking `ResMut` does *not* dirty it; only the `DerefMut` does.)

**Fix**: Gate each presentation system on the data it actually renders, using one of three patterns:
- **Snapshot `Local`** (for systems that rebuild child entities): build a tiny `#[derive(Clone, PartialEq)]` view-model of exactly the fields rendered, keep it in `Local<Option<T>>`, rebuild only when it differs. See `CharacterSheetSnapshot` / `SkillsPanelSnapshot` / `RecipeBookSnapshot`.
- **Compare-then-write** (for systems that only update a field): read the current value, write only if different — avoids dirtying Bevy change detection / re-layout. See `sync_quickbar_visuals`, the HP-bar / border writes in `sync_nearby_npcs_panel`, `sync_minimap_zoom_labels`.
- **Generation counter** (for consumers of *large* collections where snapshotting is too costly): the `ClientStateRevisions` resource (`src/game/resources.rs`) carries per-domain `u64`s bumped by `apply_game_events_to_client_state`; compare against a `Local<u64>`. See `mirror_client_world_objects_into_registry` and the `MinimapSignature` gate in `update_minimap_images`.

**Gotcha to remember**: `ClientStateRevisions` is bumped only in the client fold system (`apply_game_events_to_client_state`), never in `apply_event_to_state` — the latter is shared with the server's per-peer baseline advance and must stay pure. Editor / asset-viewer `is_changed()` gates on `editor_state` / `viewer_state` / buffers are fine; those resources change only on explicit user action.

---

## Full-scale work every frame with no change gate (2026-07 audit round)

**Symptom**: Missed frames in release, ~30fps in debug, growing worse as maps grew (256×256 island). Same bug *class* as the HUD-rebuild issue above, re-introduced by ~139 commits of new features — each new system did full-scan work per frame because nothing gated it.

**The big five found by the audit** (all fixed, keep them fixed):
- `recompute_indoor_tile_map` / `recompute_floor_mask_map` scanned every tile of every loaded floor map (65k+/floor) / every world object, every frame. Now gated with `run_if(indoor_map_inputs_changed / floor_mask_inputs_changed)` on `ClientStateRevisions` counters.
- `build_floor_render_cells` ran `quick_hash` over the full floor String-array per visible floor per frame just to detect change. Now: a `built_for` entry is *invariantly current* — entries are evicted in a re-validate pass that only runs when `revisions.floor_maps` bumps, so the steady state does zero hashing. Corollary: never insert into `built_for` without the current grid hash.
- `compute_events_for_peer` diffed the full ~3.7k-tile interest window and built a `ClientWorldObjectState` (two String clones) per in-range object, per peer per frame. Now: `FloorDiffCache` memo (revision + peer tile) skips the tile diff; world objects compare field-by-field against the baseline *before* the struct is materialized (exhaustive destructure — adding a field to the struct breaks the compare at compile time, on purpose).
- `update_roaming_npcs` rebuilt blocker/LoS/combatant/occupancy indices O(entities) every frame even when no NPC was due to step. Now a timer pre-pass early-returns first.
- `update_fog_overlay` walked the entire discovered-tiles set (up to 65k) and `get_mut`-dirtied the fog material (full GPU uniform re-upload) every frame. Now gated on (space, window origin, `revisions.discovered`); iterates whichever is smaller, window or set.

**New revision domains** in `ClientStateRevisions`: `floor_maps` (grid edits ONLY — `map_tiles` also bumps on fog discovery, which fires on nearly every step, so floor-render systems must NOT gate on `map_tiles`), `discovered`, `log`, `inventory`. Server-side, `FloorMaps::revision()` bumps on every mutable access.

**Frame-spike fixes**: floor cells for out-of-range z are now kept alive (parked at z=-10000 by `sync_floor_render_transforms`) instead of despawned+respawned on stairs (~66k entities/frame on the island); autosave saves one player per frame from a queue instead of all players in one frame.

**Gotchas discovered this round**:
- Deref-coercing a `Mut<T>` into a `&mut T` at a call site marks it changed even if the callee never writes — helper closures for compare-then-write must take `&mut Mut<T>`.
- Calling any `&mut self` method on a `ResMut` resource (e.g. `DockedPanelState::open_nearby_npcs` every frame in the auto-open path, `panel_mut` in the title sync) DerefMuts the resource and permanently defeats every `resource_changed::<T>` gate downstream. Check with the immutable getter first.
- `sprite.texture_atlas.as_mut()` dirties the whole `Sprite` (re-extract) — read `as_ref()` first and only take the `&mut` when the atlas index actually advances (`advance_animation_timers`).
- Unconditional `node.width = percent(...)` on UI bars forces relayout every frame; the value is almost always identical (`sync_vital_bars` et al.).
- `SystemTimer` is a no-op unless `diagnostics::enable_system_timers()` ran (done by `DiagnosticsPlugin`); the headless server no longer pays a Mutex per instrumented call.
- Debug builds: `[profile.dev] opt-level = 1` + `[profile.dev.package."*"] opt-level = 3` in `Cargo.toml` — do not remove; opt-level 0 own-code was a large chunk of the 30fps.
