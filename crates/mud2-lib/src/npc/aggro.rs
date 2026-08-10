//! Aggro-on-damage: an NPC that takes attributed damage learns exactly who
//! hit it and turns on them — even when the attacker stands outside its
//! `detect_distance_tiles` (the whole point: bows and spells outrange most
//! mobs' eyesight, and "shot from nowhere" used to leave the victim milling
//! around a stale noise ping instead of charging the shooter).
//!
//! Self-defense is universal by design: a town guard that is not
//! `hostile_towards` players still retaliates when a player attacks it. Only
//! *proactive* aggression goes through the faction/tag hostility model.
//!
//! `PendingNpcAggro` is deliberately shaped as a generic "this NPC now hates
//! that entity" queue rather than a damage-specific one: a future
//! guilt/witness system (guards attacking a player they *saw* murder a
//! villager) can push the same events without touching this system.

use bevy::prelude::*;

use crate::combat::components::CombatTarget;
use crate::npc::components::{
    AiMemory, AiState, Companion, HostileBehavior, Npc, RoamingStepTimer,
};
use crate::player::components::VitalStats;
use crate::world::components::SpaceResident;

/// One "make `victim` target `attacker`" request.
#[derive(Clone, Copy, Debug)]
pub struct NpcAggroEvent {
    pub victim: Entity,
    pub attacker: Entity,
}

/// Queue of aggro requests, drained by [`apply_damage_aggro`]. Pushed by
/// `apply_pending_damage` for every surviving NPC hit by a known attacker.
#[derive(Resource, Default)]
pub struct PendingNpcAggro {
    pub items: Vec<NpcAggroEvent>,
}

impl PendingNpcAggro {
    pub fn push(&mut self, event: NpcAggroEvent) {
        self.items.push(event);
    }
}

type AggroVictimQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut AiState,
        &'static mut AiMemory,
        &'static mut RoamingStepTimer,
        &'static VitalStats,
        &'static SpaceResident,
        Option<&'static Companion>,
    ),
    (With<Npc>, With<HostileBehavior>),
>;

/// Drains [`PendingNpcAggro`]: the victim (if it can fight at all — has
/// `HostileBehavior` — and isn't already committed to a live fight) locks a
/// `CombatTarget` on its attacker and goes straight to `Pursue`. Runs after
/// `apply_pending_damage`; the retarget takes effect on the victim's next AI
/// tick, which the zeroed step timer pulls forward to the next frame.
pub fn apply_damage_aggro(
    time: Res<Time>,
    mut pending: ResMut<PendingNpcAggro>,
    mut victims: AggroVictimQuery,
    attacker_query: Query<&SpaceResident>,
    mut commands: Commands,
) {
    if pending.items.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    for event in std::mem::take(&mut pending.items) {
        let Ok((mut ai_state, mut ai_memory, mut timer, vitals, resident, companion)) =
            victims.get_mut(event.victim)
        else {
            continue;
        };
        if vitals.health <= 0.0 {
            continue;
        }
        // Attacker gone or on another map: nothing to charge at.
        let Ok(attacker_resident) = attacker_query.get(event.attacker) else {
            continue;
        };
        if attacker_resident.space_id != resident.space_id {
            continue;
        }
        // A companion never turns on its own owner over stray splash damage.
        if companion.is_some_and(|c| c.owner == event.attacker) {
            continue;
        }
        // Already committed to a fight: don't ping-pong between attackers.
        // Wander/Alert/Flee victims retarget — including a fleeing NPC, whose
        // flee only survives if the pursue tick re-proves unreachability.
        if matches!(*ai_state, AiState::Pursue { .. } | AiState::Engage { .. }) {
            continue;
        }
        *ai_state = AiState::Pursue {
            target: event.attacker,
        };
        ai_memory.contact_grace_until = now + crate::npc::systems::CONTACT_GRACE_SECS;
        timer.remaining_seconds = 0.0;
        commands.entity(event.victim).insert(CombatTarget {
            entity: event.attacker,
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;
    use crate::npc::components::{Faction, RoamBounds, RoamingBehavior};
    use crate::player::components::{Player, PlayerId, PlayerIdentity};
    use crate::world::components::{SpaceId, TilePosition};

    const TEST_SPACE: SpaceId = SpaceId(0);

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<PendingNpcAggro>();
        app.add_systems(Update, apply_damage_aggro);
        app
    }

    /// An NPC with combat AI, detect radius 5 — deliberately smaller than the
    /// distances the tests shoot from.
    fn spawn_npc(app: &mut App, pos: TilePosition, faction: Faction) -> Entity {
        app.world_mut()
            .spawn((
                Npc,
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                pos,
                RoamingBehavior {
                    bounds: RoamBounds {
                        min_x: 0,
                        min_y: 0,
                        max_x: 30,
                        max_y: 30,
                    },
                    step_interval_seconds: 0.5,
                    step_interval_jitter_seconds: 0.0,
                    idle_pause_chance: 0.0,
                    momentum_bias: 0.6,
                },
                HostileBehavior {
                    detect_distance_tiles: 5,
                    disengage_distance_tiles: 8,
                    alert_duration_seconds: 4.0,
                    requires_line_of_sight: false,
                    perception: 0,
                },
                RoamingStepTimer {
                    remaining_seconds: 100.0,
                },
                AiState::default(),
                crate::npc::components::AiMemory::default(),
                VitalStats::full(20.0, 0.0),
                faction,
            ))
            .id()
    }

    fn spawn_player(app: &mut App, pos: TilePosition) -> Entity {
        app.world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(1)),
                SpaceResident {
                    space_id: TEST_SPACE,
                },
                pos,
            ))
            .id()
    }

    fn push_aggro(app: &mut App, victim: Entity, attacker: Entity) {
        app.world_mut()
            .resource_mut::<PendingNpcAggro>()
            .push(NpcAggroEvent { victim, attacker });
    }

    #[test]
    fn aggro_on_damage_beyond_detect_radius() {
        let mut app = test_app();
        // Distance 10 — double the NPC's detect radius of 5. Aggro-on-damage
        // must work regardless of sight range.
        let npc = spawn_npc(&mut app, TilePosition::ground(0, 0), Faction::MonsterSide);
        let player = spawn_player(&mut app, TilePosition::ground(10, 0));

        push_aggro(&mut app, npc, player);
        app.update();

        assert_eq!(
            app.world().get::<CombatTarget>(npc).map(|t| t.entity),
            Some(player),
            "shot NPC must lock its attacker even beyond detect radius"
        );
        assert!(
            matches!(*app.world().get::<AiState>(npc).unwrap(), AiState::Pursue { target } if target == player)
        );
        assert_eq!(
            app.world()
                .get::<RoamingStepTimer>(npc)
                .unwrap()
                .remaining_seconds,
            0.0,
            "step timer zeroed so the retarget acts on the next AI tick"
        );
    }

    #[test]
    fn guard_retaliates_when_shot_by_player() {
        let mut app = test_app();
        // PlayerSide guard, not hostile_towards players — self-defense is
        // universal, so it still turns on its attacker.
        let guard = spawn_npc(&mut app, TilePosition::ground(0, 0), Faction::PlayerSide);
        let player = spawn_player(&mut app, TilePosition::ground(7, 0));

        push_aggro(&mut app, guard, player);
        app.update();

        assert_eq!(
            app.world().get::<CombatTarget>(guard).map(|t| t.entity),
            Some(player),
            "an attacked guard retaliates regardless of faction/tags"
        );
    }

    #[test]
    fn committed_fight_is_not_ping_ponged() {
        let mut app = test_app();
        let npc = spawn_npc(&mut app, TilePosition::ground(0, 0), Faction::MonsterSide);
        let first = spawn_player(&mut app, TilePosition::ground(3, 0));
        let second = spawn_player(&mut app, TilePosition::ground(5, 0));

        *app.world_mut().get_mut::<AiState>(npc).unwrap() = AiState::Pursue { target: first };
        app.world_mut()
            .entity_mut(npc)
            .insert(CombatTarget { entity: first });

        push_aggro(&mut app, npc, second);
        app.update();

        assert_eq!(
            app.world().get::<CombatTarget>(npc).map(|t| t.entity),
            Some(first),
            "an NPC already committed to a fight must not switch on stray damage"
        );
    }

    #[test]
    fn companion_never_turns_on_its_owner() {
        let mut app = test_app();
        let owner = spawn_player(&mut app, TilePosition::ground(1, 0));
        let pet = spawn_npc(&mut app, TilePosition::ground(0, 0), Faction::PlayerSide);
        app.world_mut().entity_mut(pet).insert(Companion {
            owner,
            owner_player: Some(PlayerId(1)),
            follow_close_tiles: 2,
        });

        push_aggro(&mut app, pet, owner);
        app.update();

        assert!(
            app.world().get::<CombatTarget>(pet).is_none(),
            "splash damage from the owner must not aggro their own companion"
        );
    }

    #[test]
    fn dead_or_cross_space_cases_are_skipped() {
        let mut app = test_app();
        let npc = spawn_npc(&mut app, TilePosition::ground(0, 0), Faction::MonsterSide);
        app.world_mut().get_mut::<VitalStats>(npc).unwrap().health = 0.0;
        let player = spawn_player(&mut app, TilePosition::ground(4, 0));

        push_aggro(&mut app, npc, player);
        app.update();
        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "a dead victim must not acquire a target"
        );

        // Alive again, but the attacker moved to another space.
        app.world_mut().get_mut::<VitalStats>(npc).unwrap().health = 10.0;
        app.world_mut()
            .get_mut::<SpaceResident>(player)
            .unwrap()
            .space_id = SpaceId(9);
        push_aggro(&mut app, npc, player);
        app.update();
        assert!(
            app.world().get::<CombatTarget>(npc).is_none(),
            "an attacker in another space must not be chased"
        );
    }
}
