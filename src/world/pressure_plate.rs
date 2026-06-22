//! Pressure-plate "hold" behaviour — the puzzle layer of Cluster B
//! (`docs/utility_systems.md` §4).
//!
//! A pressure plate is an ordinary stateful object carrying a [`PressurePlate`]
//! component (parsed from its definition's `pressure_plate:` block at spawn).
//! Each tick [`update_pressure_plates`] recomputes whether the plate's tile is
//! *occupied* — by a player, an NPC, or a resting object heavy enough to hold it
//! ([`PressurePlate::held_by_weight`]) — and drives the plate (and any wired
//! target, e.g. a door) between its pressed/released states using the same
//! [`apply_state_transition`] path that interactions use. The net effect: shove
//! a barrel (Cluster B push) onto a plate and the wired door stays open while
//! the barrel rests there; pull it off and the door closes.
//!
//! Server-authoritative. State changes replicate through the existing
//! `WorldObjectUpserted` diff — no new wire format. Unlike `on_stepped`
//! triggers (fire-and-forget on entry, players/NPCs only), this system is
//! level-triggered on live occupancy and counts resting objects too.

use bevy::prelude::*;

use crate::npc::components::Npc;
use crate::player::components::Player;
use crate::world::components::{ObjectState, OverworldObject, SpaceResident, TilePosition};
use crate::world::interactions::apply_state_transition;
use crate::world::object_definitions::{OverworldObjectDefinitions, PressurePlateDef};
use crate::world::object_registry::ObjectRegistry;

/// Runtime form of [`PressurePlateDef`], attached at spawn by the
/// `apply_overworld_definition_components!` macro.
#[derive(Component, Clone, Debug)]
pub struct PressurePlate {
    pub pressed_state: String,
    pub released_state: String,
    pub min_weight: f32,
    pub target_property: Option<String>,
    pub target_pressed_state: Option<String>,
    pub target_released_state: Option<String>,
}

impl PressurePlate {
    pub fn from_def(def: &PressurePlateDef) -> Self {
        Self {
            pressed_state: def.pressed_state.clone(),
            released_state: def.released_state.clone(),
            min_weight: def.min_weight,
            target_property: def.target_property.clone(),
            target_pressed_state: def.target_pressed_state.clone(),
            target_released_state: def.target_released_state.clone(),
        }
    }

    /// The state the plate should be in given current occupancy.
    pub fn desired_state(&self, occupied: bool) -> &str {
        if occupied {
            &self.pressed_state
        } else {
            &self.released_state
        }
    }

    /// The state the wired target should be in, when this plate drives one.
    /// `None` when the plate has no fully-specified wiring (both pressed and
    /// released target states must be authored).
    pub fn desired_target_state(&self, occupied: bool) -> Option<&str> {
        match (
            self.target_pressed_state.as_deref(),
            self.target_released_state.as_deref(),
        ) {
            (Some(pressed), Some(released)) => Some(if occupied { pressed } else { released }),
            _ => None,
        }
    }

    /// Does a resting object weighing `weight` kg hold this plate down? A
    /// weightless object (decals, pre-weight-system scenery) never counts;
    /// otherwise it must meet `min_weight`.
    pub fn held_by_weight(&self, weight: f32) -> bool {
        weight > 0.0 && weight >= self.min_weight
    }
}

/// Recompute each pressure plate's occupancy and drive its (and its wired
/// target's) state. Server-only; registered with `run_if(simulation_active)`.
#[allow(clippy::type_complexity)]
pub fn update_pressure_plates(
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut commands: Commands,
    occupant_query: Query<(&SpaceResident, &TilePosition), Or<(With<Player>, With<Npc>)>>,
    object_query: Query<(&SpaceResident, &TilePosition, &OverworldObject), Without<Player>>,
    mut plate_queries: ParamSet<(
        Query<(
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            &PressurePlate,
            &ObjectState,
        )>,
        Query<
            (
                Entity,
                &SpaceResident,
                &TilePosition,
                &OverworldObject,
                &mut ObjectState,
            ),
            Without<Player>,
        >,
    )>,
) {
    /// A plate (and optional wired target) whose state needs to change.
    struct PlateTransition {
        plate_id: u64,
        plate_state: String,
        wired: Option<(u64, String)>,
    }

    // Phase 1: read plates + occupancy, gather the transitions that are due.
    let mut transitions: Vec<PlateTransition> = Vec::new();
    {
        let plates = plate_queries.p0();
        for (resident, tile, object, plate, state) in plates.iter() {
            let on_tile = |r: &SpaceResident, t: &TilePosition| {
                r.space_id == resident.space_id && t.x == tile.x && t.y == tile.y
            };
            let occupied = occupant_query.iter().any(|(r, t)| on_tile(r, t))
                || object_query.iter().any(|(r, t, o)| {
                    o.object_id != object.object_id
                        && on_tile(r, t)
                        && plate.held_by_weight(
                            definitions.get(&o.definition_id).map_or(0.0, |d| d.weight),
                        )
                });

            let desired = plate.desired_state(occupied);
            if state.0 == desired {
                continue;
            }

            let wired = plate
                .target_property
                .as_deref()
                .zip(plate.desired_target_state(occupied))
                .and_then(|(prop, target_state)| {
                    object_registry
                        .properties(object.object_id)
                        .and_then(|props| props.get(prop))
                        .and_then(|raw| raw.parse::<u64>().ok())
                        .map(|target_id| (target_id, target_state.to_owned()))
                });

            transitions.push(PlateTransition {
                plate_id: object.object_id,
                plate_state: desired.to_owned(),
                wired,
            });
        }
    }

    if transitions.is_empty() {
        return;
    }

    // Phase 2: apply through the shared state-transition helper (swaps the
    // collider/visual and mirrors `properties["state"]` for persistence).
    let mut state_query = plate_queries.p1();
    for transition in &transitions {
        apply_state_transition(
            transition.plate_id,
            &transition.plate_state,
            &definitions,
            &mut object_registry,
            &mut commands,
            &mut state_query,
        );
        if let Some((target_id, target_state)) = &transition.wired {
            apply_state_transition(
                *target_id,
                target_state,
                &definitions,
                &mut object_registry,
                &mut commands,
                &mut state_query,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate(min_weight: f32, wired: bool) -> PressurePlate {
        PressurePlate::from_def(&PressurePlateDef {
            pressed_state: "pressed".to_owned(),
            released_state: "released".to_owned(),
            min_weight,
            target_property: wired.then(|| "target".to_owned()),
            target_pressed_state: wired.then(|| "open".to_owned()),
            target_released_state: wired.then(|| "closed".to_owned()),
        })
    }

    #[test]
    fn desired_state_follows_occupancy() {
        let p = plate(0.0, false);
        assert_eq!(p.desired_state(true), "pressed");
        assert_eq!(p.desired_state(false), "released");
    }

    #[test]
    fn wired_target_state_requires_both_states() {
        let wired = plate(0.0, true);
        assert_eq!(wired.desired_target_state(true), Some("open"));
        assert_eq!(wired.desired_target_state(false), Some("closed"));
        // A plate with no authored target states drives only itself.
        let unwired = plate(0.0, false);
        assert_eq!(unwired.desired_target_state(true), None);
    }

    #[test]
    fn weight_threshold_gates_object_holds() {
        let p = plate(10.0, false);
        assert!(!p.held_by_weight(0.0), "weightless never holds a plate");
        assert!(!p.held_by_weight(9.9), "below threshold doesn't hold");
        assert!(p.held_by_weight(10.0), "at threshold holds");
        assert!(p.held_by_weight(12.0), "above threshold holds");
        // min_weight 0 → any object with non-zero weight holds it.
        let any = plate(0.0, false);
        assert!(!any.held_by_weight(0.0));
        assert!(any.held_by_weight(0.1));
    }
}
