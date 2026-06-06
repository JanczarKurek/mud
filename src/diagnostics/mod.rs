//! Client-side performance diagnostics overlay.
//!
//! Four keys, all client-only:
//! - **F2** toggles a debug tile-grid overlay (gameplay only) — draws gizmo
//!   lines along tile boundaries within the visible viewport so coordinates
//!   are easy to eyeball during play.
//! - **F3** toggles a compact FPS / frame-time readout.
//! - **F4** toggles an expanded panel: rolling-window min/avg/p99/max frame
//!   times, entity count, and live `ViewScrollOffset` progress.
//! - **F5** dumps the same numbers to the log so the user can paste them back.
//! - **F6** toggles the primary window's vsync (Fifo ↔ Immediate). Useful to
//!   distinguish "real CPU spikes" from "missed-vsync deadline" cliffs at 60 Hz.
//! - **F7** dumps an archetype histogram (entity count grouped by component
//!   set) to the log. Quick way to see what's bloating the entity count.
//! - **F8** toggles `DiagnosticPause::simulation` — flips every system gated on
//!   `simulation_active` (NPC AI, combat, regen, dialog tick, ...). If frame
//!   spikes vanish under F8, the cause is simulation-side; if they persist,
//!   it's presentation/render.
//! - **F9** toggles floor rendering between the atlas art and a flat
//!   `debug_color` view (one solid block per floor type, per tile). Useful for
//!   debugging floor coverage/flavors without the autotile art in the way.
//!   Works in gameplay and the map editor.
//! - **F10** toggles visibility on all `FloorRenderCell`s (the biggest single
//!   render-cost archetype, 4k+ entities). Despawning is irreversible; flipping
//!   `Visibility::Hidden` is enough to drop them from the render extract path.
//! - **F11** toggles visibility on the darkness-overlay quad. The shader runs
//!   per-pixel with a 32-light loop and a 1089-bit indoor mask, so a GPU stall
//!   here would also surface as a CPU spike under vsync.
//! - **F12** toggles visibility on all `ClientProjectedWorldObject`s
//!   (~1k visible NPCs/items). Combined with F10/F11, lets you bisect which
//!   render workload contributes the after-Last GPU spike.
//!
//! Built on Bevy's stock `FrameTimeDiagnosticsPlugin` and
//! `EntityCountDiagnosticsPlugin`. We keep our own 120-sample ring of
//! `Time<Real>::delta_secs()` because the stock plugin only exposes a smoothed
//! average, and a smoothed average hides the very spikes we care about.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use bevy::camera::visibility::VisibilitySystems;
use bevy::diagnostic::{
    DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::time::Real;
use bevy::transform::TransformSystems;
use bevy::ui::UiSystems;
use bevy::window::{PresentMode, PrimaryWindow};

use crate::app::state::{ClientAppState, DiagnosticPause};
use crate::world::components::ClientProjectedWorldObject;
use crate::world::darkness::DarknessOverlay;
use crate::world::floor_render::{FloorDebugRender, FloorRenderCell};
use crate::world::resources::ViewScrollOffset;
use crate::world::WorldConfig;

const SAMPLE_WINDOW: usize = 120;
const SPIKE_THRESHOLD_MS: f32 = 18.0;
const SPIKE_HISTORY_SECONDS: f32 = 30.0;

pub struct DiagnosticsPlugin;

impl Plugin for DiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
        ))
        .init_resource::<PerfOverlayState>()
        .init_resource::<FrameTimeBuffer>()
        .init_resource::<SpikeTracker>()
        .init_resource::<DiagnosticPause>()
        .init_resource::<PendingDebugActions>()
        .add_systems(Startup, spawn_overlays)
        .add_systems(First, clear_frame_timings)
        .add_systems(PreUpdate, mark_pre_update_start)
        .add_systems(Update, mark_update_start)
        .add_systems(
            PostUpdate,
            mark_post_update_start.before(UiSystems::Prepare),
        )
        .add_systems(
            PostUpdate,
            (
                // After UI Layout finishes, before TransformSystems::Propagate
                // starts. Without the .before constraint here, the marker could
                // float anywhere after PostLayout — including past Propagate —
                // and we'd attribute UI Layout cost to "transform propagate".
                mark_post_ui_end
                    .after(UiSystems::PostLayout)
                    .before(TransformSystems::Propagate),
                mark_post_xform_end
                    .after(TransformSystems::Propagate)
                    .before(VisibilitySystems::CheckVisibility),
                mark_post_visibility_end.after(VisibilitySystems::CheckVisibility),
                // Keep mark_post_xform_start unused now that we have the ui_end
                // marker as the lower bound for "transform propagate" — saves
                // the schedule constraint, but keep the field for back-compat.
                mark_post_xform_start.before(TransformSystems::Propagate),
            ),
        )
        .add_systems(Last, mark_last_start.before(dump_spike_frame_breakdown))
        .add_systems(Last, dump_spike_frame_breakdown)
        .add_systems(
            Update,
            (
                sample_frame_time,
                track_spikes,
                handle_overlay_input,
                apply_overlay_visibility,
                update_compact_overlay,
                update_expanded_overlay,
            ),
        )
        .add_systems(Update, handle_archetype_dump)
        .add_systems(
            Update,
            draw_debug_grid.run_if(in_state(ClientAppState::InGame)),
        );
    }
}

#[derive(Resource, Default)]
pub struct PerfOverlayState {
    pub compact_visible: bool,
    pub expanded_visible: bool,
    pub floor_hidden: bool,
    pub darkness_hidden: bool,
    pub objects_hidden: bool,
    pub grid_visible: bool,
}

/// Externally-queued debug effects, drained by `handle_overlay_input` each
/// frame. The Debug menu in the top bar pushes here so menu items and F-keys
/// share one effect path.
#[derive(Resource, Default)]
pub struct PendingDebugActions {
    pub actions: Vec<DebugAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugAction {
    ToggleGrid,
    ToggleFpsCompact,
    ToggleFpsExpanded,
    TogglePauseSim,
    ToggleHideFloor,
    ToggleFloorDebugColor,
    ToggleHideDarkness,
    ToggleHideObjects,
    LogSnapshot,
    CycleVsync,
}

#[derive(Resource, Default)]
struct FrameTimeBuffer {
    samples: VecDeque<f32>,
}

impl FrameTimeBuffer {
    fn push(&mut self, dt_ms: f32) {
        if self.samples.len() == SAMPLE_WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(dt_ms);
    }

    fn stats(&self) -> Option<FrameStats> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<f32> = self.samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let len = sorted.len();
        let min = sorted[0];
        let max = sorted[len - 1];
        let avg = sorted.iter().sum::<f32>() / len as f32;
        // Nearest-rank p99: ceil(0.99 * n) - 1, clamped.
        let p99_idx = ((0.99 * len as f32).ceil() as usize)
            .saturating_sub(1)
            .min(len - 1);
        let p99 = sorted[p99_idx];
        Some(FrameStats {
            min,
            max,
            avg,
            p99,
            count: len,
        })
    }
}

struct FrameStats {
    min: f32,
    max: f32,
    avg: f32,
    p99: f32,
    count: usize,
}

#[derive(Component)]
struct CompactOverlayRoot;

#[derive(Component)]
struct ExpandedOverlayRoot;

#[derive(Component)]
struct CompactFpsText;

#[derive(Resource, Default)]
struct SpikeTracker {
    timestamps: VecDeque<f32>,
}

#[derive(Component, Clone, Copy)]
enum ExpandedField {
    Fps,
    FrameTime,
    EntityCount,
    Scroll,
    Vsync,
    Spikes,
    SimPause,
}

fn spawn_overlays(mut commands: Commands) {
    // Compact: top-right, single line.
    commands
        .spawn((
            CompactOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                top: px(8.0),
                right: px(8.0),
                padding: UiRect::all(px(6.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(i32::MAX - 6),
        ))
        .with_children(|parent| {
            parent.spawn((
                CompactFpsText,
                Text::new("FPS: --"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });

    // Expanded: top-right, stacked metric lines. Sits directly under the
    // compact overlay's slot — since both default to hidden, only one is
    // visible at a time unless the user explicitly toggles both on.
    commands
        .spawn((
            ExpandedOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                top: px(40.0),
                right: px(8.0),
                padding: UiRect::all(px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(2.0),
                min_width: px(300.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            GlobalZIndex(i32::MAX - 6),
        ))
        .with_children(|parent| {
            for field in [
                ExpandedField::Fps,
                ExpandedField::FrameTime,
                ExpandedField::Spikes,
                ExpandedField::SimPause,
                ExpandedField::EntityCount,
                ExpandedField::Scroll,
                ExpandedField::Vsync,
            ] {
                parent.spawn((
                    field,
                    Text::new(""),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            }
        });
}

fn sample_frame_time(time: Res<Time<Real>>, mut buf: ResMut<FrameTimeBuffer>) {
    let dt_ms = time.delta_secs() * 1000.0;
    if dt_ms > 0.0 {
        buf.push(dt_ms);
    }
}

fn track_spikes(time: Res<Time<Real>>, mut tracker: ResMut<SpikeTracker>) {
    let dt_ms = time.delta_secs() * 1000.0;
    let now = time.elapsed_secs();
    if dt_ms >= SPIKE_THRESHOLD_MS {
        tracker.timestamps.push_back(now);
    }
    while let Some(&front) = tracker.timestamps.front() {
        if now - front > SPIKE_HISTORY_SECONDS {
            tracker.timestamps.pop_front();
        } else {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_overlay_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut pending: ResMut<PendingDebugActions>,
    mut state: ResMut<PerfOverlayState>,
    diagnostics: Res<DiagnosticsStore>,
    buf: Res<FrameTimeBuffer>,
    scroll: Res<ViewScrollOffset>,
    mut pause: ResMut<DiagnosticPause>,
    mut floor_debug: ResMut<FloorDebugRender>,
    mut window_q: Query<&mut Window, With<PrimaryWindow>>,
    floor_q: Query<Entity, With<FloorRenderCell>>,
    darkness_q: Query<Entity, With<DarknessOverlay>>,
    objects_q: Query<Entity, With<ClientProjectedWorldObject>>,
    mut commands: Commands,
) {
    let mut queued = std::mem::take(&mut pending.actions);

    if keys.just_pressed(KeyCode::F2) {
        queued.push(DebugAction::ToggleGrid);
    }
    if keys.just_pressed(KeyCode::F3) {
        queued.push(DebugAction::ToggleFpsCompact);
    }
    if keys.just_pressed(KeyCode::F4) {
        queued.push(DebugAction::ToggleFpsExpanded);
    }
    if keys.just_pressed(KeyCode::F5) {
        queued.push(DebugAction::LogSnapshot);
    }
    if keys.just_pressed(KeyCode::F6) {
        queued.push(DebugAction::CycleVsync);
    }
    if keys.just_pressed(KeyCode::F8) {
        queued.push(DebugAction::TogglePauseSim);
    }
    if keys.just_pressed(KeyCode::F9) {
        queued.push(DebugAction::ToggleFloorDebugColor);
    }
    if keys.just_pressed(KeyCode::F10) {
        queued.push(DebugAction::ToggleHideFloor);
    }
    if keys.just_pressed(KeyCode::F11) {
        queued.push(DebugAction::ToggleHideDarkness);
    }
    if keys.just_pressed(KeyCode::F12) {
        queued.push(DebugAction::ToggleHideObjects);
    }

    let mut window = window_q.single_mut().ok();
    let present_mode = window.as_deref().map(|w| w.present_mode);

    for action in queued.drain(..) {
        match action {
            DebugAction::ToggleGrid => apply_grid_toggle(&mut state),
            DebugAction::ToggleFpsCompact => apply_fps_compact_toggle(&mut state),
            DebugAction::ToggleFpsExpanded => apply_fps_expanded_toggle(&mut state),
            DebugAction::TogglePauseSim => apply_pause_toggle(&mut pause),
            DebugAction::ToggleHideFloor => {
                apply_floor_hide_toggle(&mut state, &mut commands, &floor_q);
            }
            DebugAction::ToggleFloorDebugColor => {
                floor_debug.debug_color_only = !floor_debug.debug_color_only;
                info!(
                    "Diagnostics: floor render = {}",
                    if floor_debug.debug_color_only {
                        "DEBUG COLOR"
                    } else {
                        "ATLAS"
                    }
                );
            }
            DebugAction::ToggleHideDarkness => {
                apply_darkness_hide_toggle(&mut state, &mut commands, &darkness_q);
            }
            DebugAction::ToggleHideObjects => {
                apply_objects_hide_toggle(&mut state, &mut commands, &objects_q);
            }
            DebugAction::LogSnapshot => {
                log_snapshot(&diagnostics, &buf, &scroll, present_mode, pause.simulation);
            }
            DebugAction::CycleVsync => {
                if let Some(window) = window.as_deref_mut() {
                    apply_cycle_vsync(window);
                }
            }
        }
    }
}

fn apply_grid_toggle(state: &mut PerfOverlayState) {
    state.grid_visible = !state.grid_visible;
    info!(
        "Diagnostics: debug grid {}",
        if state.grid_visible { "ON" } else { "OFF" }
    );
}

fn apply_fps_compact_toggle(state: &mut PerfOverlayState) {
    state.compact_visible = !state.compact_visible;
}

fn apply_fps_expanded_toggle(state: &mut PerfOverlayState) {
    state.expanded_visible = !state.expanded_visible;
}

fn apply_pause_toggle(pause: &mut DiagnosticPause) {
    pause.simulation = !pause.simulation;
    info!(
        "Diagnostics: simulation {}",
        if pause.simulation {
            "PAUSED"
        } else {
            "RUNNING"
        }
    );
}

fn apply_floor_hide_toggle(
    state: &mut PerfOverlayState,
    commands: &mut Commands,
    floor_q: &Query<Entity, With<FloorRenderCell>>,
) {
    state.floor_hidden = !state.floor_hidden;
    let target = if state.floor_hidden {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    let mut count = 0usize;
    for entity in floor_q {
        commands.entity(entity).insert(target);
        count += 1;
    }
    info!(
        "Diagnostics: floor cells {} ({} entities)",
        if state.floor_hidden {
            "HIDDEN"
        } else {
            "VISIBLE"
        },
        count,
    );
}

fn apply_darkness_hide_toggle(
    state: &mut PerfOverlayState,
    commands: &mut Commands,
    darkness_q: &Query<Entity, With<DarknessOverlay>>,
) {
    state.darkness_hidden = !state.darkness_hidden;
    let target = if state.darkness_hidden {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    for entity in darkness_q {
        commands.entity(entity).insert(target);
    }
    info!(
        "Diagnostics: darkness overlay {}",
        if state.darkness_hidden {
            "HIDDEN"
        } else {
            "VISIBLE"
        },
    );
}

fn apply_objects_hide_toggle(
    state: &mut PerfOverlayState,
    commands: &mut Commands,
    objects_q: &Query<Entity, With<ClientProjectedWorldObject>>,
) {
    state.objects_hidden = !state.objects_hidden;
    let target = if state.objects_hidden {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    let mut count = 0usize;
    for entity in objects_q {
        commands.entity(entity).insert(target);
        count += 1;
    }
    info!(
        "Diagnostics: projected world objects {} ({} entities)",
        if state.objects_hidden {
            "HIDDEN"
        } else {
            "VISIBLE"
        },
        count,
    );
}

/// Cycle Fifo → Immediate → Mailbox → Fifo. Mailbox is "uncapped GPU, no
/// tearing" — wgpu queues frames in a triple buffer and present picks the
/// latest. Different driver/compositor path from Immediate, useful when
/// Immediate still appears to be syncing.
fn apply_cycle_vsync(window: &mut Window) {
    let new_mode = match window.present_mode {
        PresentMode::Fifo | PresentMode::AutoVsync | PresentMode::FifoRelaxed => {
            PresentMode::Immediate
        }
        PresentMode::Immediate => PresentMode::Mailbox,
        _ => PresentMode::Fifo,
    };
    window.present_mode = new_mode;
    info!(
        "Diagnostics: present_mode -> {:?} (vsync {})",
        new_mode,
        if is_vsync(new_mode) { "ON" } else { "OFF" }
    );
}

/// F2 overlay — draws the tile grid as gizmo lines over the visible viewport.
///
/// Tile centers sit at `(x * tile_size, y * tile_size)` in world space, so
/// tile boundaries fall at `(n + 0.5) * tile_size` for integer `n`. The camera
/// is unscaled, so the visible viewport is the primary window's logical size
/// centered on the camera translation. We only draw lines that intersect the
/// viewport — drawing the full map's worth of gizmos every frame would push
/// thousands of line segments through the gizmo buffer for nothing.
fn draw_debug_grid(
    state: Res<PerfOverlayState>,
    world_config: Res<WorldConfig>,
    camera_q: Query<&Transform, With<Camera2d>>,
    window_q: Query<&Window, With<PrimaryWindow>>,
    mut gizmos: Gizmos,
) {
    if !state.grid_visible {
        return;
    }
    let Ok(camera) = camera_q.single() else {
        return;
    };
    let Ok(window) = window_q.single() else {
        return;
    };

    let ts = world_config.tile_size;
    if ts <= 0.0 {
        return;
    }
    let half_w = window.width() * 0.5;
    let half_h = window.height() * 0.5;
    let cx = camera.translation.x;
    let cy = camera.translation.y;
    let left = cx - half_w;
    let right = cx + half_w;
    let bottom = cy - half_h;
    let top = cy + half_h;

    // Convert viewport bounds into the integer range of tile boundaries to
    // draw. Boundary `n` sits at x = (n - 0.5) * ts, where tile `n` spans
    // [(n - 0.5) * ts, (n + 0.5) * ts]. Solve for n at `left` and `right`.
    let first_col = ((left / ts) + 0.5).floor() as i32;
    let last_col = ((right / ts) + 0.5).ceil() as i32;
    let first_row = ((bottom / ts) + 0.5).floor() as i32;
    let last_row = ((top / ts) + 0.5).ceil() as i32;

    let color = Color::srgba(1.0, 1.0, 1.0, 0.18);
    for col in first_col..=last_col {
        let x = (col as f32 - 0.5) * ts;
        gizmos.line_2d(Vec2::new(x, bottom), Vec2::new(x, top), color);
    }
    for row in first_row..=last_row {
        let y = (row as f32 - 0.5) * ts;
        gizmos.line_2d(Vec2::new(left, y), Vec2::new(right, y), color);
    }
}

/// Global per-name accumulator. `clear_frame_timings` zeroes it at the start
/// of every frame; `SystemTimer::drop` adds to it; `dump_spike_frame_breakdown`
/// reads it at end-of-frame and emits a sorted log line only if the frame
/// itself was a spike. The Mutex contention is one short critical section per
/// instrumented system per frame — negligible in practice.
fn frame_timings() -> &'static Mutex<HashMap<&'static str, f32>> {
    static MAP: OnceLock<Mutex<HashMap<&'static str, f32>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Captured at the start of `First` and again at boundaries between schedules
/// so the spike dump can split the frame time into:
/// - main schedule (First → Last on the main thread),
/// - and "after-Last" (Extract + render-thread wait + present + vsync).
fn schedule_marks() -> &'static Mutex<ScheduleMarks> {
    static MARKS: OnceLock<Mutex<ScheduleMarks>> = OnceLock::new();
    MARKS.get_or_init(|| Mutex::new(ScheduleMarks::default()))
}

#[derive(Default, Clone, Copy)]
struct ScheduleMarks {
    first_start: Option<Instant>,
    pre_update_start: Option<Instant>,
    update_start: Option<Instant>,
    post_update_start: Option<Instant>,
    /// Just before `TransformSystems::Propagate` runs.
    post_xform_start: Option<Instant>,
    /// Just after `TransformSystems::Propagate` finishes.
    post_xform_end: Option<Instant>,
    /// Just after `VisibilitySystems::CheckVisibility` finishes.
    post_visibility_end: Option<Instant>,
    /// Just after `UiSystems::PostLayout` finishes.
    post_ui_end: Option<Instant>,
    last_start: Option<Instant>,
    last_end: Option<Instant>,
}

/// Drop-guard timer for ad-hoc per-system timing. Insert
/// `let _t = SystemTimer::new("name", _);` at the top of any system you want
/// timed; on drop, the elapsed milliseconds are added to a per-frame map
/// keyed by `name`. The threshold parameter is preserved as a no-op so the
/// existing call sites compile unchanged.
pub struct SystemTimer {
    name: &'static str,
    start: Instant,
}

impl SystemTimer {
    pub fn new(name: &'static str, _threshold_ms_unused: f32) -> Self {
        Self {
            name,
            start: Instant::now(),
        }
    }
}

impl Drop for SystemTimer {
    fn drop(&mut self) {
        let ms = self.start.elapsed().as_secs_f32() * 1000.0;
        if let Ok(mut t) = frame_timings().lock() {
            *t.entry(self.name).or_insert(0.0) += ms;
        }
    }
}

/// Runs in `First` (before any user system). Zeroes the accumulator from the
/// previous frame so each `SystemTimer` drop only contributes to the current
/// frame's totals.
fn clear_frame_timings() {
    if let Ok(mut t) = frame_timings().lock() {
        t.clear();
    }
    if let Ok(mut m) = schedule_marks().lock() {
        m.first_start = Some(Instant::now());
    }
}

fn mark_pre_update_start() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.pre_update_start = Some(Instant::now());
    }
}

fn mark_update_start() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.update_start = Some(Instant::now());
    }
}

fn mark_post_update_start() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.post_update_start = Some(Instant::now());
    }
}

fn mark_last_start() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.last_start = Some(Instant::now());
    }
}

fn mark_post_xform_start() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.post_xform_start = Some(Instant::now());
    }
}

fn mark_post_xform_end() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.post_xform_end = Some(Instant::now());
    }
}

fn mark_post_visibility_end() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.post_visibility_end = Some(Instant::now());
    }
}

fn mark_post_ui_end() {
    if let Ok(mut m) = schedule_marks().lock() {
        m.post_ui_end = Some(Instant::now());
    }
}

/// Runs in `Last` (after every user system has dropped its `SystemTimer`).
/// If the most recent `Time<Real>` delta crossed `SPIKE_THRESHOLD_MS`, dumps
/// the sorted per-system breakdown so we can see which named work — if any —
/// dominated the spike. Quiet on normal frames.
fn dump_spike_frame_breakdown(
    time: Res<Time<Real>>,
    changed_transforms: Query<(), Changed<Transform>>,
    added_global_transforms: Query<(), Added<GlobalTransform>>,
) {
    let dt_ms = time.delta_secs() * 1000.0;
    let now = Instant::now();
    if let Ok(mut m) = schedule_marks().lock() {
        m.last_end = Some(now);
    }
    if dt_ms < SPIKE_THRESHOLD_MS {
        return;
    }
    let marks = match schedule_marks().lock() {
        Ok(m) => *m,
        Err(_) => return,
    };
    let entries: Vec<(&'static str, f32)> = match frame_timings().lock() {
        Ok(t) => t.iter().map(|(k, v)| (*k, *v)).collect(),
        Err(_) => return,
    };
    let mut entries = entries;
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let total: f32 = entries.iter().map(|(_, v)| *v).sum();

    let to_ms = |a: Option<Instant>, b: Option<Instant>| -> Option<f32> {
        match (a, b) {
            (Some(a), Some(b)) if b >= a => Some((b - a).as_secs_f32() * 1000.0),
            _ => None,
        }
    };
    let first_to_pre = to_ms(marks.first_start, marks.pre_update_start);
    let pre_to_update = to_ms(marks.pre_update_start, marks.update_start);
    let update_to_post = to_ms(marks.update_start, marks.post_update_start);
    let post_to_last = to_ms(marks.post_update_start, marks.last_start);
    let last_to_end = to_ms(marks.last_start, marks.last_end);
    let main_total = to_ms(marks.first_start, marks.last_end);
    let after_last = main_total.map(|m| (dt_ms - m).max(0.0));

    // PostUpdate sub-stages, in actual Bevy execution order:
    //   PostUpdate start → UI Layout → Transform Propagate → Visibility → tail.
    // UI Layout (Taffy) runs `.before(TransformSystems::Propagate)`, so the
    // tightened markers split them apart.
    let pu_ui_layout = to_ms(marks.post_update_start, marks.post_ui_end);
    let pu_xform = to_ms(marks.post_ui_end, marks.post_xform_end);
    let pu_visibility = to_ms(marks.post_xform_end, marks.post_visibility_end);
    let pu_tail = to_ms(marks.post_visibility_end, marks.last_start);

    let fmt = |v: Option<f32>| match v {
        Some(x) => format!("{x:>7.3} ms"),
        None => "    n/a".to_string(),
    };

    let changed_xform_count = changed_transforms.iter().count();
    let added_global_count = added_global_transforms.iter().count();

    let mut msg = format!(
        "\n[spike {:.2} ms] schedules:\n  \
         First                       {}\n  \
         PreUpdate                   {}\n  \
         Update                      {}\n  \
         PostUpdate                  {}\n  \
           ├ UI Layout (Taffy)       {}\n  \
           ├ Transform Propagate     {}\n  \
           ├ Visibility check        {}\n  \
           └ tail (extract prep, etc){}\n  \
         Last                        {}\n  \
         (main total                 {})\n  \
         after-Last+next             {}  ← Extract + GPU sync + present + vsync wait\n\
         change-detection: Changed<Transform> = {}, Added<GlobalTransform> = {}\n\
         instrumented (Update-side world systems) total {:.2} ms — breakdown:",
        dt_ms,
        fmt(first_to_pre),
        fmt(pre_to_update),
        fmt(update_to_post),
        fmt(post_to_last),
        fmt(pu_ui_layout),
        fmt(pu_xform),
        fmt(pu_visibility),
        fmt(pu_tail),
        fmt(last_to_end),
        fmt(main_total),
        fmt(after_last),
        changed_xform_count,
        added_global_count,
        total
    );
    for (name, ms) in entries.iter().take(20) {
        msg.push_str(&format!("\n  {:>7.3} ms  {}", ms, name));
    }
    warn!("{}", msg);
}

fn is_vsync(mode: PresentMode) -> bool {
    matches!(
        mode,
        PresentMode::Fifo | PresentMode::FifoRelaxed | PresentMode::AutoVsync
    )
}

fn handle_archetype_dump(world: &mut World) {
    if !world
        .resource::<ButtonInput<KeyCode>>()
        .just_pressed(KeyCode::F7)
    {
        return;
    }
    log_archetype_histogram(world);
}

fn log_archetype_histogram(world: &World) {
    let archetypes = world.archetypes();
    let components = world.components();

    let mut entries: Vec<(usize, String)> = archetypes
        .iter()
        .filter(|a| !a.is_empty())
        .map(|a| {
            let mut names: Vec<String> = a
                .components()
                .iter()
                .filter_map(|cid| components.get_info(*cid))
                .map(|ci| {
                    // Component names come back as full paths like
                    // "mud2::world::components::TilePosition" — last segment
                    // is plenty for an at-a-glance histogram.
                    let full = ci.name();
                    let full_ref: &str = full.as_ref();
                    full_ref.rsplit("::").next().unwrap_or(full_ref).to_string()
                })
                .collect();
            names.sort_unstable();
            let label = if names.len() <= 6 {
                names.join(" + ")
            } else {
                format!("{} (+{} more)", names[..6].join(" + "), names.len() - 6)
            };
            (a.len() as usize, label)
        })
        .collect();

    entries.sort_by(|a, b| b.0.cmp(&a.0));

    let total: usize = entries.iter().map(|(c, _)| c).sum();
    let mut out = String::from("\n===== ARCHETYPE HISTOGRAM =====\n");
    out.push_str(&format!("Total entities:  {}\n", total));
    out.push_str(&format!("Archetype count: {}\n", entries.len()));
    out.push_str("---\n");
    for (count, label) in entries.iter().take(25) {
        out.push_str(&format!("  {:>5}  {}\n", count, label));
    }
    if entries.len() > 25 {
        let tail: usize = entries.iter().skip(25).map(|(c, _)| c).sum();
        out.push_str(&format!(
            "  ... ({} more archetypes, {} entities)\n",
            entries.len() - 25,
            tail
        ));
    }
    out.push_str("===============================");
    info!("{}", out);
}

fn apply_overlay_visibility(
    state: Res<PerfOverlayState>,
    mut compact: Query<&mut Node, (With<CompactOverlayRoot>, Without<ExpandedOverlayRoot>)>,
    mut expanded: Query<&mut Node, (With<ExpandedOverlayRoot>, Without<CompactOverlayRoot>)>,
) {
    if !state.is_changed() {
        return;
    }
    if let Ok(mut node) = compact.single_mut() {
        node.display = if state.compact_visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Ok(mut node) = expanded.single_mut() {
        node.display = if state.expanded_visible {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn fps_color(fps: f32) -> Color {
    if fps >= 100.0 {
        Color::srgb(0.55, 1.0, 0.55)
    } else if fps >= 30.0 {
        Color::srgb(1.0, 0.95, 0.5)
    } else {
        Color::srgb(1.0, 0.45, 0.45)
    }
}

fn update_compact_overlay(
    state: Res<PerfOverlayState>,
    diagnostics: Res<DiagnosticsStore>,
    mut q: Query<(&mut Text, &mut TextColor), With<CompactFpsText>>,
) {
    if !state.compact_visible {
        return;
    }
    let Ok((mut text, mut color)) = q.single_mut() else {
        return;
    };
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0) as f32;
    let frame_time = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0) as f32;
    text.0 = format!("FPS: {fps:>3.0}  ({frame_time:.1} ms)");
    color.0 = fps_color(fps);
}

fn update_expanded_overlay(
    state: Res<PerfOverlayState>,
    diagnostics: Res<DiagnosticsStore>,
    buf: Res<FrameTimeBuffer>,
    scroll: Res<ViewScrollOffset>,
    tracker: Res<SpikeTracker>,
    pause: Res<DiagnosticPause>,
    time: Res<Time<Real>>,
    window_q: Query<&Window, With<PrimaryWindow>>,
    mut q: Query<(&ExpandedField, &mut Text, &mut TextColor)>,
) {
    if !state.expanded_visible {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0) as f32;
    let entity_count = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.value())
        .unwrap_or(0.0) as i64;
    let present_mode = window_q.single().ok().map(|w| w.present_mode);
    let stats = buf.stats();
    let now_secs = time.elapsed_secs();

    for (field, mut text, mut color) in q.iter_mut() {
        match field {
            ExpandedField::Fps => {
                text.0 = format!("FPS (smoothed): {fps:.1}");
                color.0 = fps_color(fps);
            }
            ExpandedField::FrameTime => match &stats {
                Some(s) => {
                    text.0 = format!(
                        "Frame ms — min {:.1}  avg {:.1}  p99 {:.1}  max {:.1}  (n={})",
                        s.min, s.avg, s.p99, s.max, s.count
                    );
                    // Color by max — a single 50ms hitch during a 0.18s scroll
                    // is exactly what makes movement feel "jaggy", so the line
                    // should turn red even if average FPS looks fine.
                    let max_fps_equiv = if s.max > 0.0 { 1000.0 / s.max } else { 0.0 };
                    color.0 = fps_color(max_fps_equiv);
                }
                None => {
                    text.0 = "Frame ms — collecting samples...".into();
                    color.0 = Color::WHITE;
                }
            },
            ExpandedField::EntityCount => {
                text.0 = format!("Entities: {entity_count}");
                color.0 = Color::WHITE;
            }
            ExpandedField::Scroll => {
                let lerp = &scroll.lerp;
                let active = lerp.duration > 0.0 && lerp.elapsed < lerp.duration;
                if active {
                    let pct = (lerp.elapsed / lerp.duration * 100.0).clamp(0.0, 100.0);
                    text.0 = format!(
                        "Scroll lerp: {pct:>3.0}%  ({:.3}/{:.3}s)  offset=({:.0}, {:.0})",
                        lerp.elapsed, lerp.duration, lerp.current.x, lerp.current.y,
                    );
                    color.0 = Color::srgb(0.7, 0.85, 1.0);
                } else {
                    text.0 = "Scroll lerp: idle".into();
                    color.0 = Color::srgb(0.6, 0.6, 0.6);
                }
            }
            ExpandedField::Spikes => {
                let count_10s = tracker
                    .timestamps
                    .iter()
                    .filter(|&&t| now_secs - t <= 10.0)
                    .count();
                let last = tracker.timestamps.back().map(|t| now_secs - t);
                let intervals: Vec<f32> = tracker
                    .timestamps
                    .iter()
                    .zip(tracker.timestamps.iter().skip(1))
                    .map(|(a, b)| b - a)
                    .rev()
                    .take(5)
                    .collect();
                let intervals_str = if intervals.is_empty() {
                    String::new()
                } else {
                    let s: Vec<String> = intervals.iter().map(|d| format!("{d:.2}")).collect();
                    format!("  intervals(s): {}", s.join(", "))
                };
                let last_str = match last {
                    Some(d) => format!("{d:.1}s ago"),
                    None => "never".into(),
                };
                text.0 = format!(
                    "Spikes (>{:.0}ms): {} in 10s, last {}{}",
                    SPIKE_THRESHOLD_MS, count_10s, last_str, intervals_str,
                );
                color.0 = if count_10s == 0 {
                    Color::srgb(0.55, 1.0, 0.55)
                } else if count_10s < 5 {
                    Color::srgb(1.0, 0.95, 0.5)
                } else {
                    Color::srgb(1.0, 0.5, 0.5)
                };
            }
            ExpandedField::SimPause => {
                if pause.simulation {
                    text.0 = "Simulation: PAUSED  [F8 toggles]".into();
                    color.0 = Color::srgb(1.0, 0.6, 0.3);
                } else {
                    text.0 = "Simulation: running  [F8 toggles]".into();
                    color.0 = Color::srgb(0.7, 0.7, 0.7);
                }
            }
            ExpandedField::Vsync => match present_mode {
                Some(mode) => {
                    let on = is_vsync(mode);
                    text.0 = format!(
                        "Present: {:?}  (vsync {})  [F6 toggles]",
                        mode,
                        if on { "ON" } else { "OFF" }
                    );
                    color.0 = if on {
                        Color::WHITE
                    } else {
                        Color::srgb(1.0, 0.7, 0.4)
                    };
                }
                None => {
                    text.0 = "Present: ?".into();
                    color.0 = Color::WHITE;
                }
            },
        }
    }
}

fn log_snapshot(
    diagnostics: &DiagnosticsStore,
    buf: &FrameTimeBuffer,
    scroll: &ViewScrollOffset,
    present_mode: Option<PresentMode>,
    simulation_paused: bool,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let frame_time = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let entity_count = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.value())
        .unwrap_or(0.0) as i64;
    let stats = buf.stats();
    let scroll_active = scroll.lerp.duration > 0.0 && scroll.lerp.elapsed < scroll.lerp.duration;

    let stats_str = match stats {
        Some(s) => format!(
            "  min: {:.2} ms\n  avg: {:.2} ms\n  p99: {:.2} ms\n  max: {:.2} ms\n  samples: {}",
            s.min, s.avg, s.p99, s.max, s.count
        ),
        None => "  (collecting...)".into(),
    };

    let present_str = match present_mode {
        Some(mode) => format!(
            "{:?} (vsync {})",
            mode,
            if is_vsync(mode) { "ON" } else { "OFF" }
        ),
        None => "?".to_string(),
    };

    info!(
        "\n===== PERF SNAPSHOT =====\n\
         FPS (smoothed):         {:.1}\n\
         Frame time (smoothed):  {:.2} ms\n\
         Frame time (last {} frames):\n{}\n\
         Entities:               {}\n\
         Present mode:           {}\n\
         Simulation:             {}\n\
         Scroll lerp:            {} (elapsed={:.3}s / duration={:.3}s, current=({:.1}, {:.1}))\n\
         =========================",
        fps,
        frame_time,
        SAMPLE_WINDOW,
        stats_str,
        entity_count,
        present_str,
        if simulation_paused {
            "PAUSED (F8)"
        } else {
            "running"
        },
        if scroll_active { "ACTIVE" } else { "idle" },
        scroll.lerp.elapsed,
        scroll.lerp.duration,
        scroll.lerp.current.x,
        scroll.lerp.current.y,
    );
}
