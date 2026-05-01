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

/// Day/night palette, keyed by `t ∈ [0, 1)`. Returns a multiplier applied
/// to the space's outdoor ambient — so noon is identity (1,1,1) and midnight
/// is a deep blue.
pub fn day_night_palette(time_of_day: f32) -> [f32; 3] {
    const STOPS: &[(f32, [f32; 3])] = &[
        (0.00, [0.18, 0.22, 0.45]),
        (0.22, [0.18, 0.22, 0.45]),
        (0.30, [0.85, 0.65, 0.55]),
        (0.50, [1.00, 1.00, 1.00]),
        (0.70, [0.95, 0.55, 0.40]),
        (0.78, [0.18, 0.22, 0.45]),
        (1.00, [0.18, 0.22, 0.45]),
    ];
    let t = time_of_day.rem_euclid(1.0);
    for w in STOPS.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t >= t0 && t <= t1 {
            let span = (t1 - t0).max(1e-6);
            let f = (t - t0) / span;
            return [
                c0[0] + (c1[0] - c0[0]) * f,
                c0[1] + (c1[1] - c0[1]) * f,
                c0[2] + (c1[2] - c0[2]) * f,
            ];
        }
    }
    [1.0, 1.0, 1.0]
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
    fn day_night_palette_at_noon_is_neutral() {
        let c = day_night_palette(0.5);
        assert!((c[0] - 1.0).abs() < 1e-6);
        assert!((c[1] - 1.0).abs() < 1e-6);
        assert!((c[2] - 1.0).abs() < 1e-6);
    }

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
}
