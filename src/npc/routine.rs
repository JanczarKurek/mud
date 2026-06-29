//! NPC "life agenda" — patrol routes, day/night schedules and posed station
//! activities that run *parallel* to the combat FSM in `npc::systems`.
//!
//! The combat state machine (`AiState`) stays authoritative for threats: the
//! routine is only consulted in `update_roaming_npcs` when `step_ai` left the
//! NPC in `Wander` with no fresh combat target (see `consult_routine` there).
//! When a threat appears, combat preempts the routine automatically, and the
//! NPC's pose is cleared. When combat ends and the NPC returns to `Wander`, the
//! routine re-derives the correct goal from the world clock within one tick.
//!
//! `routine_step` is a *pure* function (no ECS, no pathfinder coupling): it maps
//! `(routine, state, tile, time_of_day)` to a `RoutineIntent` and the next
//! `RoutineState`. The caller turns a `GoTo` intent into an actual step using
//! the existing `astar_next_step` / `choose_seek_step` helpers, and reconciles
//! the NPC's `ObjectState` to `RoutineState::active_pose`.

use std::collections::HashMap;

use bevy::prelude::Component;

use crate::world::components::TilePosition;
use crate::world::direction::Direction;
use crate::world::map_layout::{PatrolModeDef, RoutineInstanceDef};
use crate::world::object_definitions::ActivityDef;

use super::components::RoamingRandomState;

/// Fallback dwell (seconds) at a station whose activity declares no dwell
/// range, and the gap between flavor barks for a patrol waypoint (which has no
/// activity). Keeps a posed NPC from re-barking every tick.
const DEFAULT_DWELL_SECONDS: f32 = 5.0;

/// An NPC's baked life agenda. Built at spawn from the instance-level
/// `routine:` YAML (coordinates) plus the type-level `activities:` library.
/// Attached only to hand-placed NPCs that declare a `routine:`.
#[derive(Component, Clone, Debug)]
pub struct Routine {
    /// Followed when no schedule window is active. `None` = stand idle when
    /// off-schedule (falls through to the FSM's default wander).
    pub patrol: Option<PatrolRoute>,
    /// Time-of-day windows. First match wins; evaluated against the world
    /// clock's `time_of_day ∈ [0, 1)`.
    pub schedule: Vec<ScheduleWindow>,
    /// Activity library copied from the type definition, keyed by name. A
    /// schedule window references an entry here for its pose + flavor barks.
    pub activities: HashMap<String, ActivitySpec>,
}

#[derive(Clone, Debug)]
pub struct PatrolRoute {
    pub waypoints: Vec<Waypoint>,
    pub mode: PatrolMode,
}

#[derive(Clone, Copy, Debug)]
pub struct Waypoint {
    pub tile: TilePosition,
    pub dwell_seconds: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatrolMode {
    /// `0 → 1 → 2 → 0 → …`
    Loop,
    /// `0 → 1 → 2 → 1 → 0 → 1 → …`
    PingPong,
    /// `0 → 1 → 2` then hold at the final waypoint.
    Once,
}

impl From<PatrolModeDef> for PatrolMode {
    fn from(def: PatrolModeDef) -> Self {
        match def {
            PatrolModeDef::Loop => PatrolMode::Loop,
            PatrolModeDef::PingPong => PatrolMode::PingPong,
            PatrolModeDef::Once => PatrolMode::Once,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScheduleWindow {
    /// Inclusive start, exclusive end, in `[0, 1)`. `to < from` wraps midnight.
    pub from: f32,
    pub to: f32,
    /// Activity name; looked up in `Routine::activities` for pose + barks.
    pub activity: String,
    /// Tile the NPC stands on while performing the activity.
    pub at: TilePosition,
    /// Direction to face while dwelling (e.g. toward the anvil). `None` keeps
    /// the last facing.
    pub face: Option<Direction>,
}

/// Type-level activity description: the pose to adopt and flavor to emit.
#[derive(Clone, Debug)]
pub struct ActivitySpec {
    /// `ObjectState` name to apply while performing this activity (drives the
    /// per-state animation sheet). `None` = no pose change (keep base sprite).
    pub pose_state: Option<String>,
    pub barks: Vec<String>,
    pub dwell_min: f32,
    pub dwell_max: f32,
}

/// Per-NPC routine progress, written back each AI tick (mirrors `AiMemory`).
#[derive(Component, Clone, Debug)]
pub struct RoutineState {
    pub phase: RoutinePhase,
    pub waypoint_index: usize,
    /// PingPong direction: `true` = walking up the waypoint list.
    pub patrol_forward: bool,
    /// Elapsed-seconds deadline: at a station, the next flavor-bark time; at a
    /// patrol waypoint, when the dwell ends and the NPC advances.
    pub dwell_until: f32,
    /// `ObjectState` the routine currently has applied, so the caller can
    /// reconcile the entity's pose and clear it on combat.
    pub active_pose: Option<String>,
    /// Schedule activity currently being performed (detects window switches).
    pub active_activity: Option<String>,
}

impl Default for RoutineState {
    fn default() -> Self {
        Self {
            phase: RoutinePhase::Idle,
            waypoint_index: 0,
            patrol_forward: true,
            dwell_until: 0.0,
            active_pose: None,
            active_activity: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RoutinePhase {
    /// No active goal this tick (off-schedule with no patrol).
    #[default]
    Idle,
    /// Walking toward the current goal tile.
    Traveling,
    /// Arrived; holding at the goal (posed station or waypoint pause).
    Dwelling,
}

/// What the routine wants the NPC to do this tick. The caller owns movement
/// (turning `GoTo` into a real step) and pose reconciliation (reading
/// `RoutineState::active_pose`).
#[derive(Clone, Debug)]
pub enum RoutineIntent {
    /// No agenda — let the FSM's default wander run.
    None,
    /// Walk toward `target`; optionally face `face` on arrival.
    GoTo {
        target: TilePosition,
        face: Option<Direction>,
    },
    /// Hold position; optionally face a direction and emit a flavor bark.
    Hold {
        face: Option<Direction>,
        bark: Option<String>,
    },
}

impl Routine {
    /// Bake a runtime `Routine` from the instance YAML (`routine:`) and the
    /// type's activity library (`activities:`).
    pub fn from_def(
        def: &RoutineInstanceDef,
        activity_defs: &HashMap<String, ActivityDef>,
    ) -> Self {
        let patrol = def.patrol.as_ref().map(|p| PatrolRoute {
            mode: p.mode.into(),
            waypoints: p
                .waypoints
                .iter()
                .map(|w| Waypoint {
                    tile: TilePosition::new(w.x, w.y, w.z),
                    dwell_seconds: w.dwell.max(0.0),
                })
                .collect(),
        });
        let schedule = def
            .schedule
            .iter()
            .map(|s| ScheduleWindow {
                from: s.from,
                to: s.to,
                activity: s.activity.clone(),
                at: s.at.to_tile_position(),
                face: s.face,
            })
            .collect();
        let activities = activity_defs
            .iter()
            .map(|(name, a)| {
                (
                    name.clone(),
                    ActivitySpec {
                        pose_state: a.pose_state.clone(),
                        barks: a.barks.clone(),
                        dwell_min: a.dwell.min,
                        dwell_max: a.dwell.max,
                    },
                )
            })
            .collect();
        Self {
            patrol,
            schedule,
            activities,
        }
    }

    /// The schedule window covering `time_of_day`, if any. First match wins.
    fn active_window(&self, time_of_day: f32) -> Option<&ScheduleWindow> {
        self.schedule
            .iter()
            .find(|w| time_in_window(w.from, w.to, time_of_day))
    }
}

/// Wrap-aware membership test for a `[from, to)` window in `[0, 1)`. When
/// `to < from` the window straddles midnight (e.g. `from = 0.78, to = 0.25`).
pub fn time_in_window(from: f32, to: f32, t: f32) -> bool {
    if from <= to {
        t >= from && t < to
    } else {
        t >= from || t < to
    }
}

/// Pure routine decision. Returns the next progress state and what to do this
/// tick. Schedule windows take priority over the patrol route.
pub fn routine_step(
    routine: &Routine,
    mut state: RoutineState,
    tile: TilePosition,
    time_of_day: f32,
    elapsed: f32,
    rng: &mut RoamingRandomState,
) -> (RoutineState, RoutineIntent) {
    if let Some(window) = routine.active_window(time_of_day) {
        let intent = schedule_step(routine, &mut state, tile, window, elapsed, rng);
        return (state, intent);
    }
    if let Some(patrol) = routine.patrol.as_ref() {
        if !patrol.waypoints.is_empty() {
            let intent = patrol_step(&mut state, tile, patrol, elapsed);
            return (state, intent);
        }
    }
    // Off-schedule with no patrol: drop any pose and defer to default wander.
    state.phase = RoutinePhase::Idle;
    state.active_pose = None;
    state.active_activity = None;
    (state, RoutineIntent::None)
}

/// Travel to the window's station tile, then dwell there in the activity's
/// pose, emitting flavor barks on the dwell timer.
fn schedule_step(
    routine: &Routine,
    state: &mut RoutineState,
    tile: TilePosition,
    window: &ScheduleWindow,
    elapsed: f32,
    rng: &mut RoamingRandomState,
) -> RoutineIntent {
    if tile != window.at {
        state.phase = RoutinePhase::Traveling;
        state.active_pose = None;
        state.active_activity = Some(window.activity.clone());
        return RoutineIntent::GoTo {
            target: window.at,
            face: window.face,
        };
    }

    let activity = routine.activities.get(&window.activity);
    let just_arrived = state.phase != RoutinePhase::Dwelling
        || state.active_activity.as_deref() != Some(window.activity.as_str());
    if just_arrived {
        state.phase = RoutinePhase::Dwelling;
        state.active_activity = Some(window.activity.clone());
        state.active_pose = activity.and_then(|a| a.pose_state.clone());
        state.dwell_until = elapsed + sample_dwell(activity, rng);
        return RoutineIntent::Hold {
            face: window.face,
            bark: None,
        };
    }

    // Continuing the dwell: emit a flavor bark when the timer lapses.
    let mut bark = None;
    if elapsed >= state.dwell_until {
        bark = pick_bark(activity, rng);
        state.dwell_until = elapsed + sample_dwell(activity, rng);
    }
    RoutineIntent::Hold {
        face: window.face,
        bark,
    }
}

/// Walk the waypoint loop with per-waypoint dwell pauses.
fn patrol_step(
    state: &mut RoutineState,
    tile: TilePosition,
    patrol: &PatrolRoute,
    elapsed: f32,
) -> RoutineIntent {
    state.active_pose = None;
    state.active_activity = None;
    let len = patrol.waypoints.len();
    let current = patrol.waypoints[state.waypoint_index % len];

    if tile != current.tile {
        state.phase = RoutinePhase::Traveling;
        return RoutineIntent::GoTo {
            target: current.tile,
            face: None,
        };
    }

    // Arrived at the waypoint.
    if state.phase != RoutinePhase::Dwelling {
        state.phase = RoutinePhase::Dwelling;
        state.dwell_until = elapsed + current.dwell_seconds;
        return RoutineIntent::Hold {
            face: None,
            bark: None,
        };
    }

    if elapsed >= state.dwell_until {
        advance_waypoint(state, patrol);
        let next = patrol.waypoints[state.waypoint_index % len];
        if tile != next.tile {
            state.phase = RoutinePhase::Traveling;
            return RoutineIntent::GoTo {
                target: next.tile,
                face: None,
            };
        }
        // Degenerate: the next waypoint is the tile we already stand on.
        state.dwell_until = elapsed + next.dwell_seconds;
    }
    RoutineIntent::Hold {
        face: None,
        bark: None,
    }
}

/// Advance `waypoint_index` to the next waypoint per the patrol mode.
fn advance_waypoint(state: &mut RoutineState, patrol: &PatrolRoute) {
    let len = patrol.waypoints.len();
    if len <= 1 {
        return;
    }
    match patrol.mode {
        PatrolMode::Loop => {
            state.waypoint_index = (state.waypoint_index + 1) % len;
        }
        PatrolMode::Once => {
            if state.waypoint_index + 1 < len {
                state.waypoint_index += 1;
            }
        }
        PatrolMode::PingPong => {
            if state.patrol_forward {
                if state.waypoint_index + 1 < len {
                    state.waypoint_index += 1;
                } else {
                    state.patrol_forward = false;
                    state.waypoint_index = state.waypoint_index.saturating_sub(1);
                }
            } else if state.waypoint_index > 0 {
                state.waypoint_index -= 1;
            } else {
                state.patrol_forward = true;
                state.waypoint_index = 1.min(len - 1);
            }
        }
    }
}

fn sample_dwell(activity: Option<&ActivitySpec>, rng: &mut RoamingRandomState) -> f32 {
    match activity {
        Some(a) if a.dwell_max > a.dwell_min => {
            a.dwell_min + rng.next_f32() * (a.dwell_max - a.dwell_min)
        }
        Some(a) => a.dwell_min.max(0.0),
        None => DEFAULT_DWELL_SECONDS,
    }
}

fn pick_bark(activity: Option<&ActivitySpec>, rng: &mut RoamingRandomState) -> Option<String> {
    let a = activity?;
    if a.barks.is_empty() {
        return None;
    }
    Some(a.barks[rng.next_index(a.barks.len())].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng() -> RoamingRandomState {
        RoamingRandomState { seed: 0x1234_5678 }
    }

    fn pos(x: i32, y: i32) -> TilePosition {
        TilePosition::new(x, y, 0)
    }

    fn patrol_routine(mode: PatrolMode, tiles: &[(i32, i32)]) -> Routine {
        Routine {
            patrol: Some(PatrolRoute {
                mode,
                waypoints: tiles
                    .iter()
                    .map(|&(x, y)| Waypoint {
                        tile: pos(x, y),
                        dwell_seconds: 0.0,
                    })
                    .collect(),
            }),
            schedule: Vec::new(),
            activities: HashMap::new(),
        }
    }

    #[test]
    fn time_window_wraps_midnight() {
        // Daytime window.
        assert!(time_in_window(0.25, 0.78, 0.5));
        assert!(!time_in_window(0.25, 0.78, 0.8));
        // Night window straddling midnight.
        assert!(time_in_window(0.78, 0.25, 0.9));
        assert!(time_in_window(0.78, 0.25, 0.1));
        assert!(!time_in_window(0.78, 0.25, 0.5));
    }

    #[test]
    fn patrol_loop_advances_and_wraps() {
        let routine = patrol_routine(PatrolMode::Loop, &[(0, 0), (2, 0), (2, 2)]);
        let mut state = RoutineState::default();
        let mut r = rng();

        // At waypoint 0 with zero dwell: first tick registers the dwell, second
        // tick advances and heads to waypoint 1.
        let (s, _) = routine_step(&routine, state, pos(0, 0), 0.5, 0.0, &mut r);
        state = s;
        let (s, intent) = routine_step(&routine, state, pos(0, 0), 0.5, 1.0, &mut r);
        state = s;
        assert!(matches!(intent, RoutineIntent::GoTo { target, .. } if target == pos(2, 0)));
        assert_eq!(state.waypoint_index, 1);

        // Arrive at waypoint 1, dwell, advance to 2.
        let (s, _) = routine_step(&routine, state, pos(2, 0), 0.5, 2.0, &mut r);
        state = s;
        let (s, intent) = routine_step(&routine, state, pos(2, 0), 0.5, 3.0, &mut r);
        state = s;
        assert!(matches!(intent, RoutineIntent::GoTo { target, .. } if target == pos(2, 2)));
        assert_eq!(state.waypoint_index, 2);

        // Arrive at the last waypoint, dwell, loop back to 0.
        let (s, _) = routine_step(&routine, state, pos(2, 2), 0.5, 4.0, &mut r);
        state = s;
        let (s, intent) = routine_step(&routine, state, pos(2, 2), 0.5, 5.0, &mut r);
        state = s;
        assert!(matches!(intent, RoutineIntent::GoTo { target, .. } if target == pos(0, 0)));
        assert_eq!(state.waypoint_index, 0);
    }

    #[test]
    fn patrol_ping_pong_reverses_at_ends() {
        let routine = patrol_routine(PatrolMode::PingPong, &[(0, 0), (1, 0), (2, 0)]);
        let mut state = RoutineState::default();

        // Walk forward to the end: indices visited should be 0,1,2 then reverse
        // back toward 1,0.
        let order = [0usize, 1, 2, 1, 0, 1, 2];
        for window in order.windows(2) {
            let from = window[0];
            let to = window[1];
            // Stand on the current waypoint, dwell, then advance one step.
            let here = routine.patrol.as_ref().unwrap().waypoints[from].tile;
            let mut r = rng();
            let (s, _) = routine_step(&routine, state, here, 0.5, 0.0, &mut r);
            state = s;
            let (s, intent) = routine_step(&routine, state, here, 0.5, 1.0, &mut r);
            state = s;
            let expected = routine.patrol.as_ref().unwrap().waypoints[to].tile;
            assert!(
                matches!(intent, RoutineIntent::GoTo { target, .. } if target == expected),
                "from {from} expected goto {to}, got {intent:?}"
            );
        }
    }

    #[test]
    fn schedule_window_drives_travel_then_pose() {
        let mut activities = HashMap::new();
        activities.insert(
            "work".to_string(),
            ActivitySpec {
                pose_state: Some("working".to_string()),
                barks: vec!["*clang*".to_string()],
                dwell_min: 4.0,
                dwell_max: 4.0,
            },
        );
        let routine = Routine {
            patrol: None,
            schedule: vec![ScheduleWindow {
                from: 0.25,
                to: 0.78,
                activity: "work".to_string(),
                at: pos(5, 5),
                face: Some(Direction::North),
            }],
            activities,
        };
        let mut r = rng();

        // During the day, away from the station → travel toward it, no pose yet.
        let (state, intent) = routine_step(
            &routine,
            RoutineState::default(),
            pos(0, 0),
            0.5,
            0.0,
            &mut r,
        );
        assert!(matches!(intent, RoutineIntent::GoTo { target, .. } if target == pos(5, 5)));
        assert_eq!(state.active_pose, None);

        // Standing on the station → hold and adopt the working pose.
        let (state, intent) = routine_step(&routine, state, pos(5, 5), 0.5, 1.0, &mut r);
        assert!(matches!(intent, RoutineIntent::Hold { .. }));
        assert_eq!(state.active_pose.as_deref(), Some("working"));

        // Outside the window → pose cleared, no agenda.
        let (state, intent) = routine_step(&routine, state, pos(5, 5), 0.9, 2.0, &mut r);
        assert!(matches!(intent, RoutineIntent::None));
        assert_eq!(state.active_pose, None);
    }
}
