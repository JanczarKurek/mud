# Movement-stutter performance investigation (2026-05-08 → 2026-05-09)

## Original symptom

Movement felt "glitchy/jagged" while playing — not a steady low frame rate
but occasional perceptible hitches during the 0.18 s smooth-scroll lerp.
Nothing in `cargo check` / `cargo clippy` flagged it; needed runtime metrics
to localise.

## Diagnostic tooling built

`src/diagnostics/mod.rs` (`DiagnosticsPlugin`, client-only — registered for
`EmbeddedClient` and `TcpClient` modes, never `HeadlessServer`).

Toggles, all on otherwise-unbound function keys:

| Key  | Effect |
|------|--------|
| F3   | Compact FPS readout (top-right) |
| F4   | Expanded panel: FPS, frame-time min/avg/p99/max over 120 frames, entity count, scroll progress, sim-pause status, vsync mode, spike intervals |
| F5   | Snapshot dump — multi-line `info!` block, copy-pasteable |
| F6   | Cycle present mode: `Fifo` → `Immediate` → `Mailbox` |
| F7   | Archetype histogram — entities grouped by component set |
| F8   | Pause `simulation_active` — flips every system gated on it (NPC AI, combat, regen, dialog tick, …) at runtime via `app::state::DiagnosticPause` |
| F10  | Hide all `FloorRenderCell`s (`Visibility::Hidden`) |
| F11  | Hide darkness overlay quad |
| F12  | Hide all `ClientProjectedWorldObject`s |

**Spike-frame attribution**: any frame where `Time<Real>::delta() > 18 ms`
gets an automatic `warn!` dump in `Last`. The dump shows:

- Per-schedule timing breakdown (First / PreUpdate / Update / PostUpdate /
  Last) plus PostUpdate sub-stages (UI Layout, Transform Propagate,
  Visibility, tail).
- `after-Last+next` (= `delta - main_total`, i.e. Extract + render-thread
  wait + present + vsync wait).
- `Changed<Transform>` and `Added<GlobalTransform>` counts for the frame —
  useful to verify that change-detection-aware code is actually preventing
  dirties.
- Sorted breakdown of per-system times for any system wrapped with
  `crate::diagnostics::SystemTimer::new("name", _)`.

To time a new system: drop `let _t = crate::diagnostics::SystemTimer::new("foo", 1.0);`
at its top. The drop-guard accumulates into a per-frame `HashMap` keyed by
name, surfacing in the next spike-dump under "instrumented total / breakdown".

## Investigation timeline

### 1. Confirmed the spike is real CPU/GPU work, not vsync illusion

Initial F5 with vsync ON: avg 16.65 ms, max 26.5 ms — looked like vsync
miss. Toggled F6 (vsync OFF): avg dropped to 10.1 ms but **max stayed at
25 ms**. Spike survives without vsync, so it's real per-frame work.

### 2. Suspected NPC AI thundering-herd

`src/npc/systems.rs::update_roaming_npcs`:
- `is_blocked_position` did `blocker_query.iter().any(...)` — full O(B)
  linear scan over ~2,900 colliders for **each** of 8 candidate tiles, for
  **each** NPC stepping that frame.
- `RoamingStepTimer { remaining_seconds }` initialised in
  `src/world/setup.rs` to the same `step_interval_seconds` for every NPC,
  so all NPCs synchronised and stepped on the same frame.

**Fix landed**: built a `HashSet<(SpaceId, TilePosition)>` of blocker
positions and a `HashMap<(SpaceId, TilePosition), Entity>` of NPC tiles
once at the top of `update_roaming_npcs`; replaced linear scans with O(1)
lookups. Added deterministic per-NPC jitter to initial timer values so
NPCs spread their steps across the cycle.

**Impact on spike**: none. Spike persisted at ~25 ms even with simulation
paused (F8). NPC AI was wasteful but not the cause.

### 3. Captured a full chrome trace via `bevy/trace_chrome`

Added `profiling` Cargo feature gating `bevy/trace_chrome`. Trace file was
1.5 GB; nothing obviously wide stood out in Perfetto. Conclusion: cost
isn't in any single system Bevy auto-instruments — it's either spread
thinly or in render-pipeline work outside the trace.

### 4. Localised via per-system timing + render-workload toggles

Wrapped 10 suspect systems with `SystemTimer`. Even at 1 ms threshold, no
single system exceeded — total instrumented work per spike frame was ~0.5–1 ms.

Added F10/F11/F12 to hide floor cells, darkness overlay, and projected
world objects respectively. **Hiding all three did not reduce the spike**.

### 5. Schedule-stage attribution caught Transform Propagate

Added markers around major Bevy schedule boundaries; spike dump showed
`PostUpdate` was 8.5 ms, with `transform propagate 6 ms`. With ~9k
entities mostly at rest, that was suspicious.

**Root cause #1**: `sync_floor_render_transforms` (4,480 cells) and
`sync_tile_transforms` (1,014 projected objects) were unconditionally
writing `transform.translation = Vec3::new(...)` every frame. `Mut<T>`'s
`DerefMut` calls `set_changed()` unconditionally — confirmed at
`bevy_ecs-0.18.1/src/change_detection/traits.rs:420-428`. So ~5,500
Transforms were marked changed per frame even when the value was
identical, and Bevy's `propagate_parent_transforms` re-walked them all.

**Fix landed**: every per-entity transform write guarded with
```rust
let new = Vec3::new(...);
if transform.translation != new {
    transform.translation = new;
}
```
(field read goes through `Deref`, doesn't set changed; assignment goes
through `DerefMut`, does — so the guard makes change detection mean
something).

`Changed<Transform>` count dropped from ~5,500 to 1–6 per frame.

### 6. Architectural fix: camera-based scrolling

User asked: "why are we transforming all the tiles instead of moving the
camera?" The right question.

The world used to render every sprite at `(tile - player) * tile_size + scroll`,
with the camera fixed at world origin. That's the legacy of older 2D
engines that couldn't move the camera cheaply; in Bevy, moving the camera
is one Transform write per frame instead of N.

**Refactor landed**:
- Sprites positioned at **absolute world coords** (`tile * tile_size +
  entity_offset`).
- `src/world/camera.rs::camera_follow` positions `Camera2d` at
  `player_tile * tile_size - view_scroll.snapped()` and updates the
  player's Transform to track the camera (so the player visually stays
  at screen center during the 0.18 s scroll lerp).
- Darkness overlay quad follows the camera each frame; its shader's
  `tile_xform` uniform origin is now constant `-0.5 * tile_size`
  (previously a function of player position and scroll).

Net change: a frame at rest writes ~3 Transforms (camera, player, overlay
when scrolling) instead of ~5,500.

`Changed<Transform>` after this fix: 1–6 per frame regardless of
movement state.

### 7. PostUpdate sub-stage markers exposed UI Layout

After the camera fix, `transform propagate 6.7 ms` persisted in the
dump — strange given so few changed transforms. Tightening the markers
revealed the bracket included UI Layout: `bevy_ui` runs
`UiSystems::Layout` `.before(TransformSystems::Propagate)`
(`bevy_ui-0.18.1/src/lib.rs:180`).

With markers split:
- UI Layout (Taffy): 2 ms
- Transform Propagate: 0.13–0.18 ms (good — fix worked)
- Visibility: 0.2 ms
- main total: ~5 ms

### 8. Final localisation: render pipeline / driver / compositor

`after-Last+next` (Extract + GPU + present + vsync wait) is the new spike
location at 13–22 ms with vsync OFF. Hiding all rendered content (F10 +
F11 + F12) **did not reduce it** — eliminates GPU fill rate, draw-call
cost, and shader work as causes.

**Conclusion**: the remaining ~15 ms of variability is below our app
layer — wgpu's render-pipeline state churn, Linux compositor sync
behaviour (KWin/Mutter often impose sync regardless of
`PresentMode::Immediate`), or driver-level wait. Not fixable from
application code without a GPU profiler (RenderDoc / NSight / Tracy via
`bevy/trace_tracy`).

## Code changes (summary)

| File | Change |
|------|--------|
| `src/diagnostics/mod.rs` (new) | `DiagnosticsPlugin`, F3–F12 toggles, spike-frame dump, `SystemTimer` helper |
| `src/app/state.rs` | `DiagnosticPause` resource; `simulation_active` checks it |
| `src/app/plugin.rs` | Register `DiagnosticsPlugin` in client modes |
| `src/lib.rs` | `pub mod diagnostics;` |
| `src/npc/systems.rs` | `update_roaming_npcs` builds spatial indices; helpers take `BlockerIndex`/`NpcTileIndex` instead of raw `Query`/`&[…]` |
| `src/world/setup.rs` | Per-NPC timer jitter (`jitter_frac * step`) |
| `src/world/floor_render.rs::sync_floor_render_transforms` | Absolute world coords; conditional Transform write |
| `src/world/systems.rs::sync_tile_transforms` | Absolute world coords; conditional Transform write |
| `src/world/systems.rs::sync_player_z` | Conditional write |
| `src/world/camera.rs` (new) | `camera_follow` system |
| `src/world/darkness.rs::update_darkness_overlay` | Overlay-follows-camera; `tile_xform` constant; conditional Transform write |
| `src/world/mod.rs` | Register `camera_follow`; ordering |
| `Cargo.toml` | `profiling` feature gates `bevy/trace_chrome` |

## Open knobs / mitigations not yet applied

1. **`PresentMode::Mailbox`** (cycle via F6) — different driver path from
   `Immediate`; sometimes dramatically better on Linux.
2. **Lengthen scroll lerp** from `0.18 s` to `0.25 s` in
   `src/world/animation.rs:368` (`view_scroll.duration = 0.18`) — wider
   interpolation window makes single-frame stalls less perceptible
   without changing actual frame timing.
3. **Tracy profiler** (`bevy/trace_tracy` Cargo feature plus the `tracy`
   capture client) — only path to see what wgpu/render-thread is doing
   below the application surface.

## Numbers, before vs after

| Metric (vsync OFF, standing still) | Before | After |
|---|---|---|
| `Changed<Transform>` per frame | ~5,500 | 1–6 |
| `transform propagate` cost | 3–6 ms | 0.13–0.18 ms |
| main schedule total | ~10 ms | ~5 ms |
| Worst-case spike | 25–35 ms | 18–26 ms |

The CPU side is now healthy. The remaining variability is in the GPU /
driver / compositor layer.

---

# Round 2 (2026-05-09 →)

Goal: localise the `after-Last+next` spike below the application layer.
Plan: cheap environment experiments first, Tracy if those don't explain it,
then re-run on a second machine to test "is this just my laptop?".

## Round 2 — Environment baseline (machine A)

```text
host        Linux nixos 6.18.2 #1-NixOS SMP PREEMPT_DYNAMIC x86_64
distro      NixOS 25.11 (Xantusia)
session     XDG_SESSION_TYPE=wayland, WAYLAND_DISPLAY=wayland-1
compositor  Hyprland 0.52.1
monitor     eDP-1, 1920x1080 @ 60.008 Hz, vrr=false
            currentFormat=XRGB8888
            directScanoutBlockedBy=[USER, CANDIDATE]   (window not fullscreen)
            tearingBlockedBy=[NOT_TORN, USER, CANDIDATE]
            solitaryBlockedBy=[WINDOWED, CANDIDATE]
hypr opts   misc:vrr = 0 (off)
            render:direct_scanout_to_window — no such option in 0.52
            render:explicit_sync — no such option in 0.52
            (option names changed; need to re-look in current Hyprland docs)

cpu         Intel Core i7-9850H @ 2.60 GHz, 12 logical cores
            governor: powersave (all cores), intel_pstate active, turbo on
            current freq at idle: ~2.3 GHz of 4.6 GHz max
            x86_pkg_temp 72 °C, TVGA 63 °C — not thermally throttled

gpus        GPU0: Intel UHD 630 (CFL GT2)   8086:3e9b   Mesa 25.2.6 (i915)
            GPU1: NVIDIA GeForce MX150       10de:1d10   nvidia 580.119.02
            GPU2: Intel UHD 630 (duplicate adapter listing)

active GPU  Intel UHD 630 — confirmed via nvidia-smi:
            MX150 is in P8 idle, 0% util, 0 MiB used while game runs.
            wgpu picks the iGPU on this Wayland session despite
            HighPerformance default; NVIDIA proprietary on Wayland with
            wlroots typically requires explicit PRIME offload to engage.
```

**Working hypothesis update:** the stutter is *not* an Optimus problem (the
discrete GPU is asleep). It's Intel iGPU + Mesa 25.2.6 + Hyprland's Wayland
presentation, on a 60 Hz panel with VRR off and direct scanout blocked
because the window is not solitary/fullscreen. Most likely root causes:

1. **Compositor composition cost** — every frame goes through Hyprland's
   GLES compositor pass on the same Intel iGPU we're rendering on.
   `directScanoutBlockedBy=[USER, CANDIDATE]` means we're paying for it.
2. **Mesa Vulkan WSI on wlroots** — `presentation_time` protocol path
   between wgpu / winit / Mesa anv driver. Known sources of variability.
3. **CPU governor `powersave`** — may not boost when the render thread
   suddenly needs >16 ms of work; could cause Mesa-side stalls.

## Round 2 — Display-stack experiments (to run)

For each variant: `cargo run --release --bin mud2`, F4 (expanded panel),
walk around ~15 s, F5 (snapshot dump). Record p99 frame time and
`after-Last+next` p99 from the dump.

The 9850H idles at ~2.3 GHz; if the render thread spike is 5 ms wider
than expected, the governor may be the cause.

| # | Variant | Hypothesis tested | stutters / 10 s | max spike |
|---|---------|-------------------|----------------:|----------:|
| 1 | Default windowed (`cargo run --release --bin mud2`) | Baseline (Intel iGPU, Vulkan, Hypr windowed) | ~70 | ~20 ms |
| 2 | Hyprland fullscreen — `Super+F` | Eliminates compositor composition (biggest single Wayland win on Hyprland) | ~10 | ~20 ms |
| 3 | `WGPU_BACKEND=gl cargo run --release --bin mud2` — *failed*: panic at `bevy_render-0.18.1/src/renderer/mod.rs:281` ("Unable to find a GPU!") on `instance.request_adapter()` returning `None`. Bevy enabled the GL backend (`Backends::all()` is the default and `WGPU_BACKEND=gl` narrows it to GL), but wgpu couldn't find any GL adapter capable of rendering to the Wayland surface. On NixOS + Hyprland this is almost certainly a `libEGL.so` discovery / EGL Wayland-platform config issue at the dynamic-linker layer rather than a missing system component. Skipping — not central to this investigation. | — | — |
| 4 | `MESA_VK_DEVICE_SELECT=10de:1d10 __NV_PRIME_RENDER_OFFLOAD=1 __VK_LAYER_NV_optimus=NVIDIA_only __GLX_VENDOR_LIBRARY_NAME=nvidia cargo run --release --bin mud2` | Force NVIDIA dGPU. Wayland PRIME offload can be unstable; expect crashes or *worse* stutter. Useful negative result. |  |  |
| 5 | `sudo cpupower frequency-set -g performance` then run | Removes governor latency from suspect list |  |  |
| 6 | `hyprctl keyword misc:vrr 1` then run | Even if panel doesn't support VRR (eDP rarely does), Hyprland's frame scheduler changes |  |  |
| 7 | `vblank_mode=0 __GL_SYNC_TO_VBLANK=0 cargo run --release --bin mud2` | Bypasses Mesa-side vsync logic on the GL path (only matters with variant 3) |  |  |
| 8 | `WAYLAND_DISPLAY= DISPLAY=:0 cargo run --release --bin mud2` (forces XWayland) | Isolates Wayland-presentation logic from the rest of the render path |  |  |

Most-likely winner: **#2** (fullscreen direct-scanout). If it doesn't help,
we are not paying compositor composition cost as we thought, and #5
(governor) becomes the next cheapest test.

### Round 2 — Variant 2 result interpretation (2026-05-09)

7× improvement (70 → 10 stutters / 10 s) confirms Hyprland composition
was the dominant cost. **But max spike is still ~20 ms**, so something
else is also occasionally pushing us past the 16.67 ms vsync boundary.
Residual ~1 stutter/s suggests one of:

- Mesa anv pipeline-state churn (one-time pipeline compiles on first
  visibility of a new mesh/material combo)
- Hyprland still doing *some* sync work even with direct scanout active
- CPU governor (`powersave`) — render thread occasionally clocked down
- Kernel scheduler hiccup (less likely with PREEMPT_DYNAMIC kernel)

### Round 2 — Follow-up experiments (run in fullscreen)

After variant 2 reduced stutter 7×, the cleanest signal for residual
causes comes from running each remaining experiment **also in
fullscreen**, since baseline is now fullscreen.

| # | Variant | Stutters / 10 s | Max spike |
|---|---------|----------------:|----------:|
| 2-baseline | Fullscreen, default env | ~10 | ~20 ms |
| 2+gov | Fullscreen + `sudo cpupower frequency-set -g performance` | | |
| 2+vrr | Fullscreen + `hyprctl keyword misc:vrr 1` (panel reports vrr=false but Hyprland scheduler still changes) | | |
| 2+nvidia | Fullscreen + `__NV_PRIME_RENDER_OFFLOAD=1 __VK_LAYER_NV_optimus=NVIDIA_only` (force MX150 dGPU) | | |
| 2+tracy | Fullscreen + `cargo run --release --features tracy --bin mud2` with `tracy` GUI attached | (see Tracy section) | |

`2+gov` is the cheapest test and most likely to either eliminate the
residual or rule out the CPU side.

## Round 2 — Tracy

Only if the experiments above don't explain the spike. Cargo feature added
to opt in (`Cargo.toml`):

```toml
tracy = ["bevy/trace_tracy"]
```

Build & run:

```bash
# 1. Install Tracy capture client matching tracing-tracy 0.11.x.
#    On NixOS:    nix-shell -p tracy
#    Verify version:  tracy --version
#    The Bevy 0.18.1 → tracing-tracy → Tracy protocol must match exactly,
#    or capture client will refuse to connect.

# 2. Start capture client:
tracy &                                     # GUI
# OR for headless capture:
tracy-capture -o /tmp/mud2-stutter.tracy &

# 3. Run game with the tracy feature:
cargo run --release --features tracy --bin mud2

# 4. Reproduce stutter (walk around 15 s), close game.
#    `tracy-capture` saves the file; open it in `tracy`.
```

What to look at first:
1. **Frame view** — top bar shows per-frame timeline. Click a wide frame
   that aligns with our F5 spike-frame log timestamps.
2. **Render-thread band** — Bevy's `pipelined_rendering::render_thread`
   span. Look for the wide child span(s):
   - `render_app::render` → `extract` (Main → Render copy)
   - Render-graph nodes (clear, MainPass2d, UI, Tonemapping)
   - `present` — wait time on `surface.present()` is presentation/vsync
3. **GPU view** — wgpu emits GPU-side timestamps via Tracy; look for queue
   submission gaps that don't correspond to anything CPU-side.
4. **Pipeline cache misses** — first time a (mesh, material) combination
   appears, wgpu compiles a pipeline. Big single-frame spike on first
   visibility of new content.

After capture, paste top-3 wide spans per spike frame here and the
hypothesis they support.

## Round 2 — Open questions

- Does Hyprland 0.52 still have a `direct_scanout_to_window`-equivalent
  option, or has it been moved into `monitor` config? Check
  https://wiki.hyprland.org for current name (the option list returned
  empty for `render:` namespace).
- Should we drop the Bevy `debug` feature for the cleanest measurement?
  (`Cargo.toml:7` — current is `["dynamic_linking", "debug"]`.) `debug`
  enables wgpu validation in release builds, which adds nontrivial
  per-draw overhead.
- Is `dynamic_linking` worth keeping in this measurement? It speeds up
  iteration but its effect on runtime spikes is unclear; safer to leave
  on so we don't accidentally regress in some other way.
