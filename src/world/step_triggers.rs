//! Passive `on_stepped` object triggers.
//!
//! When a player or NPC moves onto a tile, the four authoritative movement
//! sites push a `StepEvent` into `PendingStepEvents`. `process_step_triggers`
//! drains the queue once per frame, finds any colocated objects whose
//! definition declared `on_stepped:` (and whose current `ObjectState` matches
//! the trigger's `from` filter), and runs the three supported effect kinds:
//!
//! * `ApplyEffect` — appends an entry to the stepper's `MagicEffects` via
//!   `MagicEffects::apply`. The trap is the caster (`None`) so any DoT
//!   killing blow grants no XP.
//! * `ApplyDamage` — rolls a `DamageExpr` and pushes a `DamageEvent` with
//!   `DamageSource::Environment` for `apply_pending_damage`.
//! * `SetState` — runs `apply_state_transition` (collider/visual swap +
//!   `ObjectRegistry` mirror), so a sprung bear trap visibly snaps shut and
//!   is persisted across saves for free.
//!
//! Server-authoritative. Clients observe the result via existing
//! `WorldObjectUpserted`, `PlayerVitalsChanged`, and `PlayerEffectsChanged`
//! events — no new wire format.

use bevy::prelude::*;

use crate::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use crate::combat::damage_expr::DamageExpr;
use crate::magic::effects::MagicEffects;
use crate::magic::resources::EffectSpec;
use crate::npc::components::Npc;
use crate::player::components::{AttributeSet, Player};
use crate::world::components::{ObjectState, OverworldObject, SpaceId, SpaceResident, TilePosition};
use crate::world::interactions::apply_state_transition;
use crate::world::object_definitions::{
    OverworldObjectDefinitions, StepEffectDef, StepTriggerDef,
};
use crate::world::object_registry::ObjectRegistry;

/// Spawn-time component carrying the parsed triggers from
/// `OverworldObjectDefinition::on_stepped`. `DamageExpr` is parsed eagerly so
/// authoring mistakes panic at world load, not at trap-spring time.
#[derive(Component, Clone, Debug)]
pub struct OnSteppedTriggers(pub Vec<StepTrigger>);

#[derive(Clone, Debug)]
pub struct StepTrigger {
    /// Allowed source states. Empty = matches any state (or stateless object).
    pub from: Vec<String>,
    /// If `Some(interval)`, this trigger also fires every `interval` seconds
    /// for every entity currently colocated with the object (in addition to
    /// the one-shot fire on entry). `None` = legacy on-entry-only behavior.
    pub tick_seconds: Option<f32>,
    /// Runtime accumulator for the periodic tick. Per-object, per-trigger.
    /// Cloned from a fresh `0.0` at spawn time so each object instance
    /// keeps its own phase. Unused when `tick_seconds` is `None`.
    pub accumulator: f32,
    pub effects: Vec<StepEffect>,
}

#[derive(Clone, Debug)]
pub enum StepEffect {
    ApplyEffect(EffectSpec),
    ApplyDamage(DamageExpr),
    SetState(String),
}

impl StepTrigger {
    /// Build the runtime form from authored YAML, parsing every damage
    /// expression eagerly. Panics with the definition id baked into the
    /// message so a malformed `amount:` field surfaces immediately at world
    /// load.
    pub fn from_def_list(defs: &[StepTriggerDef], context: &str) -> Vec<Self> {
        defs.iter()
            .map(|def| StepTrigger {
                from: def.from.clone(),
                tick_seconds: def.tick_seconds,
                accumulator: 0.0,
                effects: def
                    .effects
                    .iter()
                    .map(|effect| match effect {
                        StepEffectDef::ApplyEffect {
                            effect,
                            magnitude,
                            seconds,
                            secondary_magnitude,
                        } => StepEffect::ApplyEffect(EffectSpec {
                            kind: *effect,
                            magnitude: *magnitude,
                            seconds: *seconds,
                            secondary_magnitude: *secondary_magnitude,
                        }),
                        StepEffectDef::ApplyDamage { amount } => {
                            let expr = DamageExpr::parse(amount).unwrap_or_else(|err| {
                                panic!(
                                    "on_stepped damage expression '{amount}' on '{context}' \
                                     failed to parse: {err}"
                                )
                            });
                            StepEffect::ApplyDamage(expr)
                        }
                        StepEffectDef::SetState { state } => StepEffect::SetState(state.clone()),
                    })
                    .collect(),
            })
            .collect()
    }

    /// True iff `current_state` (or its absence) satisfies the trigger's
    /// `from` filter. Empty filter = always matches (works for stateless
    /// objects too).
    pub fn state_matches(&self, current_state: Option<&str>) -> bool {
        self.from.is_empty()
            || current_state.is_some_and(|cs| self.from.iter().any(|s| s == cs))
    }

    /// Collect this trigger's declared effects into the per-stepper
    /// accumulators used by `process_step_triggers` and
    /// `process_continuous_step_triggers`. Caller is responsible for state
    /// matching — this just dispatches by `StepEffect` variant.
    pub fn gather_effects(
        &self,
        object_id: u64,
        effect_specs: &mut Vec<EffectSpec>,
        damage_amounts: &mut Vec<f32>,
        state_transition: &mut Option<(u64, String)>,
    ) {
        for effect in &self.effects {
            match effect {
                StepEffect::ApplyEffect(spec) => effect_specs.push(*spec),
                StepEffect::ApplyDamage(expr) => {
                    let rolled = expr.roll(&AttributeSet::default()).max(0) as f32;
                    damage_amounts.push(rolled);
                }
                StepEffect::SetState(new_state) => {
                    *state_transition = Some((object_id, new_state.clone()));
                }
            }
        }
    }
}

/// Queue of "an entity just landed on this tile" events. Pushed by every
/// authoritative movement site (player normal step, player portal, admin
/// teleport, NPC step) and drained once per frame by
/// `process_step_triggers`.
#[derive(Resource, Default)]
pub struct PendingStepEvents {
    pub events: Vec<StepEvent>,
}

#[derive(Clone, Copy, Debug)]
pub struct StepEvent {
    pub entity: Entity,
    pub space_id: SpaceId,
    pub tile: TilePosition,
}

impl PendingStepEvents {
    pub fn push(&mut self, event: StepEvent) {
        self.events.push(event);
    }
}

/// Drains `PendingStepEvents`, finds matching `OnSteppedTriggers` at each
/// stepped tile, and applies the declared effects through existing pipelines
/// (`MagicEffects::apply`, `PendingDamageEvents`, `apply_state_transition`).
#[allow(clippy::too_many_arguments)]
pub fn process_step_triggers(
    mut pending_steps: ResMut<PendingStepEvents>,
    mut pending_damage: ResMut<PendingDamageEvents>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut commands: Commands,
    mut object_queries: ParamSet<(
        Query<
            (
                &SpaceResident,
                &TilePosition,
                &OverworldObject,
                Option<&ObjectState>,
                &OnSteppedTriggers,
            ),
            Without<Player>,
        >,
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
    mut stepper_effects: Query<&mut MagicEffects>,
) {
    let events = std::mem::take(&mut pending_steps.events);
    if events.is_empty() {
        return;
    }

    struct PendingWork {
        stepper: Entity,
        damage_amounts: Vec<f32>,
        effect_specs: Vec<EffectSpec>,
        // The last SetState encountered wins if multiple triggers request one.
        state_transition: Option<(u64, String)>,
    }

    let mut work: Vec<PendingWork> = Vec::new();

    {
        let lookup = object_queries.p0();
        for event in &events {
            let mut effect_specs: Vec<EffectSpec> = Vec::new();
            let mut damage_amounts: Vec<f32> = Vec::new();
            let mut state_transition: Option<(u64, String)> = None;

            for (resident, tile, object, state_opt, triggers) in lookup.iter() {
                if resident.space_id != event.space_id || *tile != event.tile {
                    continue;
                }
                let current_state = state_opt.map(|s| s.0.as_str());

                for trigger in &triggers.0 {
                    if !trigger.state_matches(current_state) {
                        continue;
                    }
                    trigger.gather_effects(
                        object.object_id,
                        &mut effect_specs,
                        &mut damage_amounts,
                        &mut state_transition,
                    );
                }
            }

            if !effect_specs.is_empty()
                || !damage_amounts.is_empty()
                || state_transition.is_some()
            {
                work.push(PendingWork {
                    stepper: event.entity,
                    damage_amounts,
                    effect_specs,
                    state_transition,
                });
            }
        }
    }

    // Phase 2: apply effects to the stepper's MagicEffects and queue damage.
    for w in &work {
        if let Ok(mut effects) = stepper_effects.get_mut(w.stepper) {
            for spec in &w.effect_specs {
                effects.apply(*spec, None);
            }
        }
        for amount in &w.damage_amounts {
            pending_damage.push(DamageEvent {
                target: w.stepper,
                amount: *amount,
                source: DamageSource::Environment,
            });
        }
    }

    // Phase 3: state transitions (separate query shape, matched to the
    // existing `apply_state_transition` helper).
    let mut state_query = object_queries.p1();
    for w in &work {
        if let Some((object_id, new_state)) = &w.state_transition {
            apply_state_transition(
                *object_id,
                new_state,
                &definitions,
                &mut object_registry,
                &mut commands,
                &mut state_query,
            );
        }
    }
}

/// Drives the "while standing on the tile" half of step triggers. For every
/// trigger declared with `tick_seconds: Some(interval)`, this system
/// accumulates frame `dt` on the trigger and — every full `interval` worth —
/// applies the trigger's effects to every entity currently colocated with
/// the object whose `ObjectState` still matches the trigger's `from` filter.
///
/// Each re-application pushes a fresh `ActiveEffect` entry on the stepper
/// (no special "refresh" path); the existing L2 stacking in
/// `tick_dot_effects` handles aggregation. Triggers with `tick_seconds:
/// None` are ignored — pure legacy one-shot-on-entry semantics.
#[allow(clippy::too_many_arguments)]
pub fn process_continuous_step_triggers(
    time: Res<Time>,
    mut pending_damage: ResMut<PendingDamageEvents>,
    definitions: Res<OverworldObjectDefinitions>,
    mut object_registry: ResMut<ObjectRegistry>,
    mut commands: Commands,
    mut object_queries: ParamSet<(
        Query<
            (
                &SpaceResident,
                &TilePosition,
                &OverworldObject,
                Option<&ObjectState>,
                &mut OnSteppedTriggers,
            ),
            Without<Player>,
        >,
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
    on_tile_query: Query<
        (Entity, &SpaceResident, &TilePosition),
        Or<(With<Player>, With<Npc>)>,
    >,
    mut stepper_effects: Query<&mut MagicEffects>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    // Defends against a frame stall: if `dt` is unusually large we still
    // only fire this many ticks per object per frame. The accumulator gets
    // clamped to `interval` afterwards so the next frame fires immediately.
    const MAX_TICKS_PER_FRAME: u32 = 4;

    struct TickWork {
        stepper: Entity,
        damage_amounts: Vec<f32>,
        effect_specs: Vec<EffectSpec>,
        state_transition: Option<(u64, String)>,
    }

    let mut work: Vec<TickWork> = Vec::new();

    // Snapshot the on-tile entity list once per frame. SpaceResident +
    // TilePosition both implement Copy so this is cheap.
    let on_tile: Vec<(Entity, SpaceId, TilePosition)> = on_tile_query
        .iter()
        .map(|(e, r, t)| (e, r.space_id, *t))
        .collect();

    // Phase 1: advance accumulators and gather per-(stepper, tick) work.
    {
        let mut triggers_query = object_queries.p0();
        for (resident, tile, object, state_opt, mut triggers) in triggers_query.iter_mut() {
            let current_state = state_opt.map(|s| s.0.clone());
            for trigger in &mut triggers.0 {
                let Some(interval) = trigger.tick_seconds else {
                    continue;
                };
                if interval <= 0.0 {
                    continue;
                }
                if !trigger.state_matches(current_state.as_deref()) {
                    // Hold phase at zero while the state filter rejects the
                    // trigger so a long "off" stretch doesn't burst on resume.
                    trigger.accumulator = 0.0;
                    continue;
                }
                trigger.accumulator += dt;
                let mut ticks_fired = 0u32;
                while trigger.accumulator >= interval && ticks_fired < MAX_TICKS_PER_FRAME {
                    trigger.accumulator -= interval;
                    ticks_fired += 1;
                    for (entity, space_id, t) in &on_tile {
                        if *space_id != resident.space_id || *t != *tile {
                            continue;
                        }
                        let mut effect_specs: Vec<EffectSpec> = Vec::new();
                        let mut damage_amounts: Vec<f32> = Vec::new();
                        let mut state_transition: Option<(u64, String)> = None;
                        trigger.gather_effects(
                            object.object_id,
                            &mut effect_specs,
                            &mut damage_amounts,
                            &mut state_transition,
                        );
                        if effect_specs.is_empty()
                            && damage_amounts.is_empty()
                            && state_transition.is_none()
                        {
                            continue;
                        }
                        work.push(TickWork {
                            stepper: *entity,
                            damage_amounts,
                            effect_specs,
                            state_transition,
                        });
                    }
                }
                if trigger.accumulator > interval {
                    trigger.accumulator = interval;
                }
            }
        }
    }

    // Phase 2: apply gathered effects.
    for w in &work {
        if let Ok(mut effects) = stepper_effects.get_mut(w.stepper) {
            for spec in &w.effect_specs {
                effects.apply(*spec, None);
            }
        }
        for amount in &w.damage_amounts {
            pending_damage.push(DamageEvent {
                target: w.stepper,
                amount: *amount,
                source: DamageSource::Environment,
            });
        }
    }

    // Phase 3: state transitions through the matching query shape.
    let mut state_query = object_queries.p1();
    for w in &work {
        if let Some((object_id, new_state)) = &w.state_transition {
            apply_state_transition(
                *object_id,
                new_state,
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
    use crate::combat::damage_expr::DamageExpr;
    use crate::magic::resources::EffectKind;
    use crate::world::object_definitions::StepEffectDef;

    #[test]
    fn build_step_triggers_parses_damage_expr() {
        let defs = vec![StepTriggerDef {
            from: vec!["armed".to_owned()],
            tick_seconds: None,
            effects: vec![
                StepEffectDef::ApplyDamage {
                    amount: "2d6+4".to_owned(),
                },
                StepEffectDef::ApplyEffect {
                    effect: EffectKind::Chill,
                    magnitude: 1.0,
                    seconds: 4.0,
                    secondary_magnitude: Some(2.0),
                },
                StepEffectDef::SetState {
                    state: "sprung".to_owned(),
                },
            ],
        }];
        let triggers = StepTrigger::from_def_list(&defs, "bear_trap");
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].from, vec!["armed".to_owned()]);
        assert_eq!(triggers[0].effects.len(), 3);
        assert!(matches!(triggers[0].effects[0], StepEffect::ApplyDamage(_)));
        assert!(matches!(
            triggers[0].effects[1],
            StepEffect::ApplyEffect(_)
        ));
        match &triggers[0].effects[2] {
            StepEffect::SetState(s) => assert_eq!(s, "sprung"),
            _ => panic!("expected SetState"),
        }
    }

    #[test]
    #[should_panic(expected = "failed to parse")]
    fn build_step_triggers_panics_on_bad_damage_expr() {
        let defs = vec![StepTriggerDef {
            from: vec![],
            tick_seconds: None,
            effects: vec![StepEffectDef::ApplyDamage {
                amount: "not-a-dice-expression".to_owned(),
            }],
        }];
        let _ = StepTrigger::from_def_list(&defs, "broken_trap");
    }

    #[test]
    fn damage_expr_rolls_with_default_attributes() {
        // Environmental damage must work without any attacker stats.
        let expr = DamageExpr::parse("1d4+2").unwrap();
        for _ in 0..20 {
            let rolled = expr.roll(&AttributeSet::default());
            assert!((3..=6).contains(&rolled));
        }
    }

    #[test]
    fn tick_seconds_defaults_to_none() {
        // A trigger declared without `tick_seconds` keeps legacy one-shot
        // semantics — the parsed runtime form must reflect that.
        let defs = vec![StepTriggerDef {
            from: vec![],
            tick_seconds: None,
            effects: vec![StepEffectDef::ApplyEffect {
                effect: EffectKind::Burning,
                magnitude: 4.0,
                seconds: 2.0,
                secondary_magnitude: None,
            }],
        }];
        let triggers = StepTrigger::from_def_list(&defs, "test");
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].tick_seconds, None);
        assert_eq!(triggers[0].accumulator, 0.0);
    }

    #[test]
    fn tick_seconds_is_propagated_and_accumulator_starts_zero() {
        let defs = vec![StepTriggerDef {
            from: vec![],
            tick_seconds: Some(1.0),
            effects: vec![StepEffectDef::ApplyEffect {
                effect: EffectKind::Burning,
                magnitude: 4.0,
                seconds: 2.0,
                secondary_magnitude: None,
            }],
        }];
        let triggers = StepTrigger::from_def_list(&defs, "test");
        assert_eq!(triggers[0].tick_seconds, Some(1.0));
        // Each freshly-spawned object owns its own accumulator at 0.0; this
        // is what gives `process_continuous_step_triggers` deterministic
        // first-tick timing per object.
        assert_eq!(triggers[0].accumulator, 0.0);
    }

    #[test]
    fn state_matches_empty_from_accepts_any_state() {
        let trigger = StepTrigger {
            from: vec![],
            tick_seconds: None,
            accumulator: 0.0,
            effects: vec![],
        };
        assert!(trigger.state_matches(None));
        assert!(trigger.state_matches(Some("anything")));
    }

    #[test]
    fn state_matches_filters_by_from_list() {
        let trigger = StepTrigger {
            from: vec!["lit".to_owned()],
            tick_seconds: None,
            accumulator: 0.0,
            effects: vec![],
        };
        assert!(trigger.state_matches(Some("lit")));
        assert!(!trigger.state_matches(Some("extinguished")));
        assert!(!trigger.state_matches(None));
    }

    #[test]
    fn gather_effects_collects_each_variant() {
        // Build a trigger whose effect list exercises every StepEffect
        // variant; `gather_effects` must funnel each into the matching
        // accumulator vector exactly once.
        let trigger = StepTrigger {
            from: vec![],
            tick_seconds: Some(1.0),
            accumulator: 0.0,
            effects: vec![
                StepEffect::ApplyEffect(EffectSpec {
                    kind: EffectKind::Burning,
                    magnitude: 4.0,
                    seconds: 2.0,
                    secondary_magnitude: None,
                }),
                StepEffect::ApplyDamage(DamageExpr::parse("1d1+0").unwrap()),
                StepEffect::SetState("sprung".to_owned()),
            ],
        };
        let mut effect_specs: Vec<EffectSpec> = Vec::new();
        let mut damage_amounts: Vec<f32> = Vec::new();
        let mut state_transition: Option<(u64, String)> = None;
        trigger.gather_effects(
            42,
            &mut effect_specs,
            &mut damage_amounts,
            &mut state_transition,
        );
        assert_eq!(effect_specs.len(), 1);
        assert_eq!(effect_specs[0].kind, EffectKind::Burning);
        assert_eq!(damage_amounts.len(), 1);
        // 1d1+0 always rolls 1.
        assert_eq!(damage_amounts[0], 1.0);
        assert_eq!(state_transition, Some((42, "sprung".to_owned())));
    }
}
