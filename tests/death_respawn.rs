//! End-to-end death flow over the real wire, focused on the server-side
//! de-spatialization contract: dying strips `SpaceResident`/`TilePosition`
//! (so an engaged NPC drops aggro and can't re-acquire the body), the corpse
//! phase is bounded by the respawn click, and acknowledging death re-inserts
//! the spatial components at the respawn point — after which NPCs can aggro
//! the same entity again.
//!
//! The wire-level scoping of the death (witness loses the body, victim keeps
//! vitals/chat, far peers hear nothing) is covered by `combat_scoping.rs`.

mod common;

use bevy::prelude::*;
use common::{
    boot_server, pump, register_and_enter_world, server_addr, unique_test_path, wait_for_snapshot,
    TestClient,
};
use mud2::combat::components::{AttackProfile, CombatTarget};
use mud2::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use mud2::combat::damage_type::DamageType;
use mud2::game::commands::GameCommand;
use mud2::network::protocol::ClientMessage;
use mud2::npc::components::{
    AiMemory, AiState, Faction, HostileBehavior, Npc, RoamBounds, RoamingBehavior,
    RoamingRandomState, RoamingStepTimer,
};
use mud2::player::components::{AwaitingRespawn, PlayerId, PlayerIdentity, VitalStats};
use mud2::world::components::{SpaceId, SpaceResident, TilePosition};

/// Server-side entity of the player with the given replicated id.
fn server_player_entity(app: &mut App, player_id: PlayerId) -> Entity {
    let mut players = app.world_mut().query::<(Entity, &PlayerIdentity)>();
    players
        .iter(app.world())
        .find(|(_, identity)| identity.id == player_id)
        .map(|(entity, _)| entity)
        .expect("player entity for id")
}

/// Spawn a melee hostile that detects across the whole map, next to `tile`.
fn spawn_hostile(app: &mut App, space_id: SpaceId, tile: TilePosition) -> Entity {
    app.world_mut()
        .spawn((
            Npc,
            SpaceResident { space_id },
            tile,
            RoamingBehavior {
                bounds: RoamBounds {
                    min_x: 0,
                    min_y: 0,
                    max_x: 60,
                    max_y: 60,
                },
                step_interval_seconds: 0.05,
                step_interval_jitter_seconds: 0.0,
                idle_pause_chance: 0.0,
                momentum_bias: 0.6,
            },
            HostileBehavior {
                detect_distance_tiles: 20,
                disengage_distance_tiles: 30,
                alert_duration_seconds: 4.0,
                requires_line_of_sight: false,
                perception: 0,
            },
            AttackProfile::melee(),
            RoamingStepTimer {
                remaining_seconds: 0.0,
            },
            RoamingRandomState { seed: 1 },
            AiState::default(),
            AiMemory::default(),
            Faction::MonsterSide,
            VitalStats::full(500.0, 0.0),
        ))
        .id()
}

/// Force the NPC's next AI tick to fire on the next frame.
fn force_ai_tick(app: &mut App, npc: Entity) {
    app.world_mut()
        .get_mut::<RoamingStepTimer>(npc)
        .expect("npc step timer")
        .remaining_seconds = 0.0;
}

#[test]
fn death_despatializes_player_and_npc_reaggros_after_respawn() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let addr = server_addr(&app);

    let mut victim = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut victim, "death_victim", "Victim");
    let victim_id = wait_for_snapshot(&mut app, &mut victim, |s| s.local_player_id.is_some())
        .local_player_id
        .unwrap();
    let victim_entity = server_player_entity(&mut app, victim_id);

    let victim_space = app
        .world()
        .get::<SpaceResident>(victim_entity)
        .expect("live victim has a space")
        .space_id;
    let victim_tile = *app
        .world()
        .get::<TilePosition>(victim_entity)
        .expect("live victim has a tile");

    // A hostile adjacent to the victim engages within a few AI ticks.
    let npc = spawn_hostile(
        &mut app,
        victim_space,
        TilePosition::new(victim_tile.x + 1, victim_tile.y, victim_tile.z),
    );
    for _ in 0..20 {
        force_ai_tick(&mut app, npc);
        pump(&mut app, &mut victim, 1);
        if app.world().get::<CombatTarget>(npc).is_some() {
            break;
        }
    }
    assert_eq!(
        app.world().get::<CombatTarget>(npc).map(|t| t.entity),
        Some(victim_entity),
        "NPC should engage the adjacent live player"
    );

    // Kill the victim through the real damage pipeline.
    app.world_mut()
        .resource_mut::<PendingDamageEvents>()
        .push(DamageEvent {
            target: victim_entity,
            amount: 1_000_000.0,
            source: DamageSource::Environment,
            damage_type: DamageType::Blunt,
            vfx_override: None,
            attacker: None,
        });
    wait_for_snapshot(&mut app, &mut victim, |s| {
        s.player_vitals.is_some_and(|v| v.health <= 0.0)
    });

    // Server contract: the dead player is de-spatialized and marked.
    assert!(
        app.world().get::<SpaceResident>(victim_entity).is_none(),
        "death must remove SpaceResident"
    );
    assert!(
        app.world().get::<TilePosition>(victim_entity).is_none(),
        "death must remove TilePosition"
    );
    let awaiting = app
        .world()
        .get::<AwaitingRespawn>(victim_entity)
        .expect("dead player must be AwaitingRespawn");
    assert_eq!(awaiting.death_space, victim_space);

    // The NPC drops aggro on its next AI ticks and cannot re-acquire the body.
    for _ in 0..10 {
        force_ai_tick(&mut app, npc);
        pump(&mut app, &mut victim, 1);
    }
    assert!(
        app.world().get::<CombatTarget>(npc).is_none(),
        "a dead player must not hold NPC aggro"
    );
    assert!(
        matches!(app.world().get::<AiState>(npc), Some(AiState::Wander)),
        "NPC should return to Wander instead of crowding the grave, got {:?}",
        app.world().get::<AiState>(npc)
    );

    // Respawn: spatial components come back at the respawn point, marker gone.
    victim.send(ClientMessage::Command(GameCommand::AcknowledgeDeath));
    wait_for_snapshot(&mut app, &mut victim, |s| {
        s.player_vitals.is_some_and(|v| v.health > 0.0) && s.player_tile_position.is_some()
    });
    assert!(
        app.world().get::<AwaitingRespawn>(victim_entity).is_none(),
        "respawn must clear AwaitingRespawn"
    );
    let respawn_tile = *app
        .world()
        .get::<TilePosition>(victim_entity)
        .expect("respawned player has a tile again");
    assert!(
        app.world().get::<SpaceResident>(victim_entity).is_some(),
        "respawned player has a space again"
    );

    // Teleport the respawned victim back next to the NPC: detection must work
    // again on the same (re-spatialized) entity.
    let npc_tile = *app.world().get::<TilePosition>(npc).expect("npc tile");
    let mut position = app
        .world_mut()
        .get_mut::<TilePosition>(victim_entity)
        .expect("respawned player tile");
    *position = TilePosition::new(npc_tile.x + 1, npc_tile.y, npc_tile.z);
    for _ in 0..20 {
        force_ai_tick(&mut app, npc);
        pump(&mut app, &mut victim, 1);
        if app.world().get::<CombatTarget>(npc).is_some() {
            break;
        }
    }
    assert_eq!(
        app.world().get::<CombatTarget>(npc).map(|t| t.entity),
        Some(victim_entity),
        "NPC must be able to re-aggro the respawned player"
    );
    let _ = respawn_tile;
}
