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
- Debug builds: `[profile.dev] opt-level = 1` + `[profile.dev.package."*"] opt-level = 3` in `Cargo.toml` — do not remove either. Deps at O3 carry the Bevy/wgpu inner loops; own-code O1 is needed because the Bevy query/iterator generics monomorphized into the mud2 crate are hot too — re-tested 2026-07-07: O0 own-code is 25fps even on a mostly empty map, algorithmic fixes notwithstanding.
- Compile-time counterweight (2026-07-07): `[profile.dev] lto = "off"` — cargo's dev default is thin-*local* LTO, not "off", and disabling it cut the 115k-line crate's builds measurably (single-file incremental 28s→20s, full crate rebuild 2m46s/18.5m CPU→2m09s/11m CPU); fps impact expected nil (generics instantiate in the calling CGU) but verify with the F3 overlay after profile changes. Beware: flipping `lto` in any direction invalidates every dependency rlib → one ~15 min full dep rebuild at O3.
- Compile-time round 2 (2026-08-06): bevy now uses a **curated feature set** (`default_app` + `default_platform` + `2d_bevy_render` + `ui_bevy_render` — no 3d/audio/scene/picking; UI `Interaction` runs on `ui_focus_system`, which is not picking-gated) and `[profile.dev.build-override] opt-level = 3` runs proc-macros optimized. Single-file incremental 20s→15.7s; ~250 MB of never-called rlibs (bevy_pbr, bevy_light, bevy_animation-3d bits, bevy_gltf, bevy_picking, audio) dropped from the graph. If a new bevy API won't resolve, add its gating feature to the curated list in `Cargo.toml` — don't revert to default features. `viewer-hot-reload` is also off `default` now: yarn dialog + spritesheet hot-reload need `cargo run --features viewer-hot-reload`.
- Compile-time round 3 (2026-08-07): **workspace split** — game lib in `crates/mud2-lib` (lib target still named `mud2`), editor+viewers in `crates/mud2-editor`, root package `mud2-bins` holds only binaries. Measured dev incrementals: editor-file edit → `cargo build --bin mud2` **9.8s** (editor edits no longer rebuild the game lib); lib-file edit → server **~17.5s** / mud2 **~18s** (±2s vs pre-split — the dependent editor crate re-builds on lib changes, but the server binary never compiles editor code at all). Gotchas: `cargo run --bin X` works from the root because the bins live in the root package (moving them to a sub-crate breaks `--bin` resolution — cargo restricts it to the current package); unit tests run with the *crate dir* as CWD, so `crates/*/assets` symlinks keep relative asset paths working; `#[cfg(test)]` fixture constructors are invisible across crates (now unconditional `#[doc(hidden)] pub`).

---

## Wall / door sprite generation invariants (2026-07 warm-masonry art pass)

**Wall & directional-door art is 100% generator-owned.** `scripts/gen_wall_set.py` (8 wall pieces) and `scripts/gen_door_set.py` (`wooden_door_n/s/e/w`) emit both the PNGs **and** the `metadata.yaml`s from shared geometry in `scripts/wall_perspective.py`. Never hand-edit those metadata files — a regeneration silently clobbers hand edits. That already happened once: `wall_corner:` was hand-added to the four corner YAMLs after generation, and a later regen would have deleted it, breaking corner fade/tint (`is_camera_facing` / `interior_diagonal` in `src/world/systems.rs`). The field is now emitted by the generator; keep any new render field in the template, not in the files.

**Regression gates when touching the generators** (both are deliberate invariants, not accidents):
- Slab positions (`SLAB_N/S/E/W`), the emitted `floor_mask_rect`s, and each sprite's canvas size must not change — interior `Flooring`-flavor floors are clipped to those rects (`FloorMaskMap`, `src/world/floor_render.rs`) and the canvases are sized tight so sprites never bleed into neighbour tiles (cross-tile alpha occlusion). After an art-only change, `git diff assets/overworld_objects/wall_*` must show PNGs only.
- Regeneration must be byte-stable (run twice, `sha256sum`): per-stone variation comes from `hashlib` keyed on world-u block coords — never `random`. Keying on world coordinates (not arm-local) is also what makes masonry courses line up across neighbouring wall tiles and around corner arms.

**All of a door's state PNGs must share one canvas.** `build_object_visual_bundle` sizes every state's sprite from the *base* `render.sprite_width/height_tiles`; `sync_object_state_visuals` swaps only the `Sprite` on state change. `gen_door_set.py` guarantees this by computing the canvas from the same arm as the matching wall — closed.png, open.png, and the wall sprite are all the same size by construction.

**Legacy `wooden_door` is still a valid object** — freestanding doors (proving-grounds gates, the starter-cellar sandbox door, one overworld door) intentionally keep the flat sprite; only doors set into a wall run use the directional slabs (`scripts/migrate_doors_in_map.py` infers the side from adjacent walls — objects *and* `tiles:` glyph-grid walls — and leaves ambiguous doors alone). The building tool picks the side variant via `BuildingPreset::doors` (`DoorSlots`), falling back to `default_door` for presets without side variants; corners take no door once `doors` is configured.

---

## Character resumed into a dead ephemeral space id ("empty dungeon" / stranded on login)

**Symptom**: After disconnecting inside an ephemeral dungeon (proving grounds / starter cellar), the character logs back into a bare void or an inconsistent dungeon; in the 2026-08 playtest this surfaced as unexplained "That item is out of reach." refusals. World saves also accumulated orphan `floor_maps` entries (keys `(7,0)`, `(8,0)`, `(9,0)` with `spaces` only listing 0–6).

**Root causes** (three cooperating defects in the ephemeral-space lifecycle):
1. Character saves persist the *runtime* space id (`space_id: 9`) even for ephemeral instances. On the next login the resume path trusted any non-origin saved position, so the character was placed into a space id that no longer exists — and which a later `allocate_space_id` (reset to `max persisted id + 1` on snapshot load) can hand to a *different* future instance.
2. `cleanup_empty_ephemeral_spaces` despawned the entities and removed the space, but never removed the space's `FloorMaps` grids — leaked grids then persisted into the world snapshot as orphans keyed by reusable ids.
3. Ephemeral re-instantiation reuses *authored* object ids (`spawn_overworld_object_instance`), which is fine while ids are unique among live entities — but any lifecycle leak above turns that into cross-instance aliasing.

**Fix**: `needs_spawn_location` (shared by the embedded and TCP select-character paths) now also respawns when the saved space id is not live in `SpaceManager`; `cleanup_empty_ephemeral_spaces` calls `FloorMaps::remove_space`; the snapshot writer skips floor maps whose space no longer exists. Reach refusals in `MoveItem/pickup` and `TakeFromStack` now log both positions + space id so any surviving desync is diagnosable from the server log. E2E coverage: `tests/pickup_tcp.rs` (portal entry → walk → pickup; teardown/re-entry; disconnect-inside → reconnect).

**Related silent-failure UX fixed alongside**: dragging a movable-but-not-storable object (barrels…) onto the inventory, or onto a full/incompatible slot, was rejected with no message (`place_stack_in_slot_ref` returns false) — both now push a narrator line; drag-release slot hit-testing now also checks the `ItemSlotImage` families like the context-menu path, so drops on the slot art no longer fall through to a world-tile shove.

---

## Remote players frozen on one animation frame

**Symptom**: In TCP multiplayer, other players are visible but never play walk/idle animations and never change facing.

**Root cause**: Remote players spawn with `ClientRemotePlayerVisual` (no `Player`, no `ClientProjectedWorldObject`), but the clip-selection systems in `src/world/animation.rs` (`trigger_movement_animation`, `return_to_idle_animation`) only queried the other two markers. The spawn-time `AnimatedSprite` defaults to clip `"idle"`, which does not exist in the player sheet (only directional `idle_*`/`walk_*`), so `frame_count` fell back to 1 — permanently frame 0 of row 0. The local player escapes only because `return_to_idle_animation` resolves `"idle"` → `idle_<facing>` on frame 1.

**Fix**: Both systems' player branches widened to `Or<(With<Player>, With<ClientRemotePlayerVisual>)>` (both resolve clips from the `player` definition; `sync_remote_player_projection` already supplied `JustMoved`/`Facing`/`VisualOffset`). Note for new spawn paths: a sheet with only directional clips means the initial `"idle"` clip is invalid until some system resolves a directional one — make sure every animated entity is covered by one of the clip-selection systems.

---

## Client command silently "worked offline" but never crossed the wire (pre-unification bypass class)

**Symptom class**: A UI button or input handler works in embedded mode but does nothing (or behaves differently) in TCP mode — or vice versa.

**Root cause**: Before the 2026-08 loopback unification, EmbeddedClient bypassed the wire protocol entirely, so embedded-only code paths could drift from the networked ones. Since the unification, EmbeddedClient runs the real client/server message pipeline over an in-process loopback pipe (`network/loopback.rs`), and the same class of bug has exactly one remaining cause: pushing client intent into the wrong queue.

**The rule**: client intent (input, UI clicks, console submissions) goes into `ClientPendingCommands` — it is drained only by `flush_client_commands_to_server` and always crosses the wire, coming back attributed to the sending peer. `PendingGameCommands` is server-side (network ingest, admin REPL, quest/scripting producers, map editor); its untargeted entries resolve to "first player" and are trusted as server-internal. Pushing client intent into `PendingGameCommands` in the unified embedded App gets it consumed *locally* by a server drainer — a silent wire bypass that works offline and breaks (or double-fires) online.

**Also gone with the unification** (don't reference these in new code): `collect_game_events_from_authority`, `route_peer_ui_events_to_local`, `spawn_embedded_player_authoritative`, `sync_authoritative_player_display`, `sync_authoritative_player_position_view`, the character screens' direct-sqlite arms, and `LocalSelectedCharacter`. Frame ordering across the client/server halves is declared via the ungated `network::sets` SystemSets.
