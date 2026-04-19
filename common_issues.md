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

## Remote player movement appears jagged

**Symptom**: Other players' sprites snap to position rather than smoothly sliding.

**Root cause**: `sync_remote_player_projection` updated `TilePosition` but did not insert `VisualOffset` / `JustMoved` components the way `sync_client_world_projection` did for world objects.

**Fix**: Added the same `VisualOffset` + `JustMoved` insertion block to `sync_remote_player_projection` in `src/world/systems.rs` (guarded by `dx.abs() <= 1 && dy.abs() <= 1` to skip teleports).
