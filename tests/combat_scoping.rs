//! Spatial scoping of combat feedback over the real wire: a player death is
//! visible (poof + chat line + sprite removal) to a nearby bystander but
//! completely silent for a peer across the map, and the dead player's remote
//! projection drops immediately — before the respawn click — then returns
//! once death is acknowledged.

mod common;

use bevy::prelude::*;
use common::{
    boot_server, pump, register_and_enter_world, server_addr, unique_test_path, wait_for_snapshot,
    TestClient,
};
use mud2::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use mud2::combat::damage_type::DamageType;
use mud2::game::commands::GameCommand;
use mud2::game::resources::GameUiEvent;
use mud2::network::protocol::ClientMessage;
use mud2::player::components::{PlayerId, PlayerIdentity};
use mud2::world::components::TilePosition;

/// Server-side entity of the player with the given replicated id.
fn server_player_entity(app: &mut App, player_id: PlayerId) -> Entity {
    let mut players = app.world_mut().query::<(Entity, &PlayerIdentity)>();
    players
        .iter(app.world())
        .find(|(_, identity)| identity.id == player_id)
        .map(|(entity, _)| entity)
        .expect("player entity for id")
}

/// Teleport a player server-side (authoritative components), far quicker than
/// walking 30+ real steps. Replication picks the move up like any other.
fn teleport_player(app: &mut App, player_id: PlayerId, tile: TilePosition) {
    let entity = server_player_entity(app, player_id);
    let mut position = app
        .world_mut()
        .get_mut::<TilePosition>(entity)
        .expect("player tile position");
    *position = tile;
}

fn has_death_poof(events: &[GameUiEvent]) -> bool {
    events.iter().any(|event| {
        matches!(event, GameUiEvent::VfxSpawn { definition_id, .. } if definition_id == "death_poof")
    })
}

fn has_defeated_line(lines: &[String]) -> bool {
    lines.iter().any(|line| line.contains("is defeated"))
}

#[test]
fn player_death_is_scoped_and_hides_the_body_until_respawn() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let addr = server_addr(&app);

    // Victim and nearby witness spawn together at the plaza (35, 25);
    // the far peer is teleported to the map corner, >30 tiles away.
    let mut victim = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut victim, "scope_victim", "Victim");
    let mut witness = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut witness, "scope_witness", "Witness");
    let mut far_peer = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut far_peer, "scope_far", "Faraway");

    let victim_id = wait_for_snapshot(&mut app, &mut victim, |s| s.local_player_id.is_some())
        .local_player_id
        .unwrap();
    let far_id = wait_for_snapshot(&mut app, &mut far_peer, |s| s.local_player_id.is_some())
        .local_player_id
        .unwrap();

    teleport_player(&mut app, far_id, TilePosition::ground(2, 2));

    // The witness sees the victim; the far peer stops seeing both plaza
    // players once its own teleport replicates.
    wait_for_snapshot(&mut app, &mut witness, |s| {
        s.remote_players.contains_key(&victim_id)
    });
    wait_for_snapshot(&mut app, &mut far_peer, |s| {
        s.player_tile_position == Some(TilePosition::ground(2, 2)) && s.remote_players.is_empty()
    });

    // Fresh UI-event buffers so the assertions below only see death traffic.
    witness.ui_events.clear();
    far_peer.ui_events.clear();
    victim.ui_events.clear();

    let victim_entity = server_player_entity(&mut app, victim_id);
    app.world_mut()
        .resource_mut::<PendingDamageEvents>()
        .push(DamageEvent {
            target: victim_entity,
            amount: 1_000_000.0,
            source: DamageSource::Environment,
            damage_type: DamageType::Blunt,
            vfx_override: None,
        });

    // Witness: the victim's projection drops without any respawn click.
    let witness_view = wait_for_snapshot(&mut app, &mut witness, |s| {
        !s.remote_players.contains_key(&victim_id)
    });
    assert!(
        has_death_poof(&witness.ui_events),
        "witness missed the poof"
    );
    assert!(
        has_defeated_line(&witness_view.chat_log_lines),
        "witness missed the defeat line: {:?}",
        witness_view.chat_log_lines
    );

    // Victim: dead (HP 0) and shown the death summary.
    let victim_view = wait_for_snapshot(&mut app, &mut victim, |s| {
        s.player_vitals.is_some_and(|v| v.health <= 0.0)
    });
    assert!(
        victim
            .ui_events
            .iter()
            .any(|event| matches!(event, GameUiEvent::DeathSummary { .. })),
        "victim never received the death summary"
    );
    let death_tile = victim_view.player_tile_position;

    // Far peer: pump a while, then assert total silence about the death.
    pump(&mut app, &mut far_peer, 20);
    assert!(
        !has_death_poof(&far_peer.ui_events),
        "death poof leaked across the map"
    );
    assert!(
        !has_defeated_line(&far_peer.state.chat_log_lines),
        "defeat chat line leaked across the map: {:?}",
        far_peer.state.chat_log_lines
    );

    // Respawn: vitals restore and the witness sees the victim again.
    victim.send(ClientMessage::Command(GameCommand::AcknowledgeDeath));
    wait_for_snapshot(&mut app, &mut victim, |s| {
        s.player_vitals.is_some_and(|v| v.health > 0.0)
    });
    let respawned = wait_for_snapshot(&mut app, &mut witness, |s| {
        s.remote_players.contains_key(&victim_id)
    });
    // Sanity: the victim respawned at their home tile (may or may not equal
    // the death tile), and the witness sees them at that tile.
    assert_eq!(
        respawned.remote_players[&victim_id].tile_position,
        wait_for_snapshot(&mut app, &mut victim, |s| s.player_tile_position.is_some())
            .player_tile_position
            .unwrap()
    );
    let _ = death_tile;
}
