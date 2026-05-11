//! Lighting data: world clock, `LightSource` ECS component, and the bridge
//! that attaches `LightSource` to projected world objects from authored YAML.
//!
//! Rendering lives in `crate::world::darkness` — this module is data-only
//! (no apply systems, no per-tile cache). The presentation layer reads
//! `LightSource` components and produces a single fullscreen darkness
//! overlay; see `darkness.rs` for the shader-side details.

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::world::components::ClientProjectedWorldObject;
use crate::world::map_layout::AmbientKeyframe;
use crate::world::object_definitions::{LightEmissionDef, OverworldObjectDefinitions};

/// Length of one in-game day in real seconds.
const SECONDS_PER_DAY: f32 = 20.0 * 60.0;

/// World-clock change threshold. Larger differences emit `WorldTimeChanged`.
pub const WORLD_TIME_EPSILON: f32 = 0.001;

/// Heartbeat: emit `WorldTimeChanged` if no event has fired in this many real
/// seconds, even when the value has barely moved.
pub const WORLD_TIME_HEARTBEAT_SECS: f32 = 10.0;

/// Server-authoritative world clock. `time_of_day ∈ [0, 1)`. Boots at noon
/// (0.5); not persisted across server restarts.
#[derive(Resource, Clone, Copy, Debug)]
pub struct WorldClock {
    pub time_of_day: f32,
    /// Real seconds since the last `WorldTimeChanged` emission. Drives the
    /// heartbeat-based emission path in `compute_events_for_peer`.
    pub seconds_since_emit: f32,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self {
            time_of_day: 0.5,
            seconds_since_emit: 0.0,
        }
    }
}

/// Advances `WorldClock.time_of_day` once per Update tick. Gated by
/// `simulation_active` so MapEditor freezes the clock.
pub fn advance_world_clock(time: Res<Time>, mut clock: ResMut<WorldClock>) {
    let dt = time.delta_secs();
    let advance = dt / SECONDS_PER_DAY;
    clock.time_of_day = (clock.time_of_day + advance).rem_euclid(1.0);
    clock.seconds_since_emit += dt;
}

/// ECS component carried by any entity that emits light (lit torch, player,
/// magic projectile, etc.). The darkness overlay shader reads each light's
/// world position from the source's Transform and its `radius`/`intensity`
/// from this component. `color` is currently unused by the shader — lights
/// only "remove darkness" — kept for forward-compat (e.g. tinted halos).
#[derive(Component, Clone, Copy, Debug)]
pub struct LightSource {
    pub color: [f32; 3],
    pub radius: f32,
    pub intensity: f32,
}

impl LightSource {
    pub fn new(color: [f32; 3], radius: f32, intensity: f32) -> Self {
        Self {
            color,
            radius,
            intensity,
        }
    }
}

impl From<&LightEmissionDef> for LightSource {
    fn from(def: &LightEmissionDef) -> Self {
        let r = def.color[0] as f32 / 255.0;
        let g = def.color[1] as f32 / 255.0;
        let b = def.color[2] as f32 / 255.0;
        Self {
            color: [r, g, b],
            radius: def.radius,
            intensity: def.intensity,
        }
    }
}

/// Runtime form of `AmbientKeyframe`: sRGB byte triple expanded to linear
/// f32 [0, 1]. Built from authored YAML via `convert_authored_keyframes`,
/// or supplied directly as the engine default by `default_day_night_curve`.
#[derive(Clone, Copy, Debug)]
pub struct AmbientKeyframeF32 {
    pub time: f32,
    pub color: [f32; 3],
    pub alpha: f32,
}

/// Engine default day/night curve. Used when a map has `has_day_night: true`
/// but supplies no explicit `outdoor_curve`. Mirrors the colors of the
/// retired 7-stop palette but adds explicit alpha so:
///   - noon (0.50) ⇒ alpha 0.0 (fully transparent — torches invisible)
///   - dusk/dawn ⇒ warm tint with mild darkening
///   - midnight ⇒ deep blue, navigable darkness (alpha 0.55, not pitch-black)
pub fn default_day_night_curve() -> [AmbientKeyframeF32; 7] {
    [
        AmbientKeyframeF32 {
            time: 0.00,
            color: [0.18, 0.22, 0.45],
            alpha: 0.55,
        },
        AmbientKeyframeF32 {
            time: 0.22,
            color: [0.18, 0.22, 0.45],
            alpha: 0.55,
        },
        AmbientKeyframeF32 {
            time: 0.30,
            color: [0.85, 0.65, 0.55],
            alpha: 0.30,
        },
        AmbientKeyframeF32 {
            time: 0.50,
            color: [1.00, 1.00, 1.00],
            alpha: 0.00,
        },
        AmbientKeyframeF32 {
            time: 0.70,
            color: [0.95, 0.55, 0.40],
            alpha: 0.30,
        },
        AmbientKeyframeF32 {
            time: 0.78,
            color: [0.18, 0.22, 0.45],
            alpha: 0.55,
        },
        AmbientKeyframeF32 {
            time: 1.00,
            color: [0.18, 0.22, 0.45],
            alpha: 0.55,
        },
    ]
}

/// Evaluate an outdoor ambient curve at `time_of_day ∈ [0, 1)`.
/// Returns `(rgb_linear, alpha)`. Cyclic: a curve with two keyframes at
/// t=0.2 and t=0.8 will interpolate from the 0.8 keyframe back round to
/// the 0.2 keyframe for t ∈ [0.0, 0.2) ∪ (0.8, 1.0).
pub fn evaluate_ambient_curve(
    keyframes: &[AmbientKeyframeF32],
    time_of_day: f32,
) -> ([f32; 3], f32) {
    let t = time_of_day.rem_euclid(1.0);
    match keyframes.len() {
        0 => ([1.0, 1.0, 1.0], 0.0),
        1 => (keyframes[0].color, keyframes[0].alpha.clamp(0.0, 1.0)),
        _ => {
            let first = keyframes[0];
            let last = *keyframes.last().unwrap();
            let (k0, k1) = if t < first.time {
                let k0 = AmbientKeyframeF32 {
                    time: last.time - 1.0,
                    color: last.color,
                    alpha: last.alpha,
                };
                (k0, first)
            } else if t > last.time {
                let k1 = AmbientKeyframeF32 {
                    time: first.time + 1.0,
                    color: first.color,
                    alpha: first.alpha,
                };
                (last, k1)
            } else {
                let mut k0 = keyframes[0];
                let mut k1 = keyframes[1];
                for w in keyframes.windows(2) {
                    if t >= w[0].time && t <= w[1].time {
                        k0 = w[0];
                        k1 = w[1];
                        break;
                    }
                }
                (k0, k1)
            };
            let span = (k1.time - k0.time).max(1e-6);
            let f = ((t - k0.time) / span).clamp(0.0, 1.0);
            let rgb = [
                k0.color[0] + (k1.color[0] - k0.color[0]) * f,
                k0.color[1] + (k1.color[1] - k0.color[1]) * f,
                k0.color[2] + (k1.color[2] - k0.color[2]) * f,
            ];
            let a = (k0.alpha + (k1.alpha - k0.alpha) * f).clamp(0.0, 1.0);
            (rgb, a)
        }
    }
}

/// Convert authored YAML keyframes to the linear-space runtime form. Sorts
/// by `time` ascending, wraps `time` into `[0, 1)`, clamps `alpha`.
pub fn convert_authored_keyframes(authored: &[AmbientKeyframe]) -> Vec<AmbientKeyframeF32> {
    let mut out: Vec<AmbientKeyframeF32> = authored
        .iter()
        .map(|k| AmbientKeyframeF32 {
            time: k.time.rem_euclid(1.0),
            color: srgb_u8_to_linear(k.color),
            alpha: k.alpha.clamp(0.0, 1.0),
        })
        .collect();
    out.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// `[u8; 3]` sRGB → `[f32; 3]` linear-ish (0..1). The darkness shader
/// further composes against the world without a separate sRGB conversion;
/// for our color-grading needs treating the ambient values as linear is
/// good enough and matches authored YAML's intuitive "RGB byte triples".
pub fn srgb_u8_to_linear(rgb: [u8; 3]) -> [f32; 3] {
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
    ]
}

/// Bridge from authored YAML lighting metadata + replicated `state` to the
/// `LightSource` ECS component on projected world objects. Inserts/updates/
/// removes the component each frame to track state changes (e.g. lighting
/// a torch).
pub fn sync_object_light_components(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    definitions: Res<OverworldObjectDefinitions>,
    query: Query<(Entity, &ClientProjectedWorldObject, Option<&LightSource>)>,
) {
    let _t = crate::diagnostics::SystemTimer::new("sync_object_light_components", 1.0);
    for (entity, projected, existing) in &query {
        let desired = client_state
            .world_objects
            .get(&projected.object_id)
            .and_then(|object| {
                let definition = definitions.get(&object.definition_id)?;
                definition
                    .light_for_state(object.state.as_deref())
                    .map(LightSource::from)
            });

        match (existing, desired) {
            (None, Some(light)) => {
                commands.entity(entity).insert(light);
            }
            (Some(_), None) => {
                commands.entity(entity).remove::<LightSource>();
            }
            (Some(prev), Some(next)) => {
                if !light_equals(prev, &next) {
                    commands.entity(entity).insert(next);
                }
            }
            (None, None) => {}
        }
    }
}

fn light_equals(a: &LightSource, b: &LightSource) -> bool {
    let eps = 1e-4;
    (a.radius - b.radius).abs() < eps
        && (a.intensity - b.intensity).abs() < eps
        && (a.color[0] - b.color[0]).abs() < eps
        && (a.color[1] - b.color[1]).abs() < eps
        && (a.color[2] - b.color[2]).abs() < eps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_emission_def_to_component_normalizes_color() {
        let def = LightEmissionDef {
            color: [255, 128, 0],
            radius: 5.0,
            intensity: 0.8,
        };
        let comp: LightSource = (&def).into();
        assert!((comp.color[0] - 1.0).abs() < 1e-6);
        assert!((comp.color[1] - 128.0 / 255.0).abs() < 1e-6);
        assert!((comp.color[2]).abs() < 1e-6);
        assert!((comp.radius - 5.0).abs() < 1e-6);
        assert!((comp.intensity - 0.8).abs() < 1e-6);
    }

    #[test]
    fn light_equals_uses_epsilon() {
        let a = LightSource::new([1.0, 1.0, 1.0], 5.0, 1.0);
        let b = LightSource::new([1.0 + 1e-5, 1.0, 1.0], 5.0, 1.0);
        assert!(light_equals(&a, &b));
        let c = LightSource::new([0.5, 1.0, 1.0], 5.0, 1.0);
        assert!(!light_equals(&a, &c));
    }

    #[test]
    fn evaluate_ambient_curve_zero_keyframes_returns_neutral() {
        let (rgb, a) = evaluate_ambient_curve(&[], 0.5);
        assert_eq!(rgb, [1.0, 1.0, 1.0]);
        assert_eq!(a, 0.0);
    }

    #[test]
    fn evaluate_ambient_curve_single_keyframe_is_constant() {
        let kf = [AmbientKeyframeF32 {
            time: 0.3,
            color: [0.4, 0.6, 0.8],
            alpha: 0.7,
        }];
        let (rgb, a) = evaluate_ambient_curve(&kf, 0.0);
        assert_eq!(rgb, [0.4, 0.6, 0.8]);
        assert!((a - 0.7).abs() < 1e-6);
        let (_, a2) = evaluate_ambient_curve(&kf, 0.9);
        assert!((a2 - 0.7).abs() < 1e-6);
    }

    #[test]
    fn evaluate_ambient_curve_linear_interpolation() {
        let kf = [
            AmbientKeyframeF32 {
                time: 0.0,
                color: [1.0, 1.0, 1.0],
                alpha: 1.0,
            },
            AmbientKeyframeF32 {
                time: 1.0,
                color: [0.0, 0.0, 0.0],
                alpha: 0.0,
            },
        ];
        let (rgb, a) = evaluate_ambient_curve(&kf, 0.5);
        assert!((rgb[0] - 0.5).abs() < 1e-6);
        assert!((rgb[1] - 0.5).abs() < 1e-6);
        assert!((rgb[2] - 0.5).abs() < 1e-6);
        assert!((a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn evaluate_ambient_curve_wraparound() {
        // Keyframes at t=0.2 (red, alpha 0) and t=0.8 (blue, alpha 1).
        // Evaluate at t=0.0 (before first): bracket is (t=0.8 mapped to -0.2)
        // and (t=0.2). Span = 0.4. At t=0.0, f = (0.0 - (-0.2)) / 0.4 = 0.5.
        // So midpoint: r/b averaged, alpha averaged.
        let kf = [
            AmbientKeyframeF32 {
                time: 0.2,
                color: [1.0, 0.0, 0.0],
                alpha: 0.0,
            },
            AmbientKeyframeF32 {
                time: 0.8,
                color: [0.0, 0.0, 1.0],
                alpha: 1.0,
            },
        ];
        let (rgb, a) = evaluate_ambient_curve(&kf, 0.0);
        assert!((rgb[0] - 0.5).abs() < 1e-6, "got r={}", rgb[0]);
        assert!((rgb[1] - 0.0).abs() < 1e-6);
        assert!((rgb[2] - 0.5).abs() < 1e-6, "got b={}", rgb[2]);
        assert!((a - 0.5).abs() < 1e-6, "got a={a}");
    }

    #[test]
    fn default_day_night_curve_at_noon_has_zero_alpha() {
        let curve = default_day_night_curve();
        let (_, a) = evaluate_ambient_curve(&curve, 0.5);
        assert!(a.abs() < 1e-6, "midday alpha must be 0, got {a}");
    }

    #[test]
    fn default_day_night_curve_at_noon_is_white() {
        let curve = default_day_night_curve();
        let (rgb, _) = evaluate_ambient_curve(&curve, 0.5);
        assert!((rgb[0] - 1.0).abs() < 1e-6);
        assert!((rgb[1] - 1.0).abs() < 1e-6);
        assert!((rgb[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn convert_authored_keyframes_sorts_and_clamps() {
        let authored = vec![
            AmbientKeyframe {
                time: 0.8,
                color: [255, 0, 0],
                alpha: 1.5, // clamps to 1.0
            },
            AmbientKeyframe {
                time: 0.2,
                color: [0, 255, 0],
                alpha: -0.3, // clamps to 0.0
            },
        ];
        let out = convert_authored_keyframes(&authored);
        assert_eq!(out.len(), 2);
        assert!((out[0].time - 0.2).abs() < 1e-6);
        assert!((out[1].time - 0.8).abs() < 1e-6);
        assert!((out[0].alpha - 0.0).abs() < 1e-6);
        assert!((out[1].alpha - 1.0).abs() < 1e-6);
        // sRGB byte → linear-ish f32: 255 → 1.0
        assert!((out[0].color[1] - 1.0).abs() < 1e-6);
        assert!((out[1].color[0] - 1.0).abs() < 1e-6);
    }
}
