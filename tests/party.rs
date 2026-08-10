//! Party lifecycle over the real wire: invite → popup → accept → both peers
//! fold the same roster; a credited kill splits XP between in-range members
//! with the group bonus; leaving disbands a two-man party; a disconnect is
//! reaped by `cleanup_invalid_parties` and narrated to the survivor.
//!
//! Run with `--test-threads=1` like the other e2e suites.

mod common;

use bevy::prelude::*;
use common::{
    boot_server, collect_ui_events, pump, register_and_enter_world, server_addr, unique_test_path,
    wait_for_snapshot, TestClient,
};
use mud2::combat::damage::{DamageEvent, DamageSource, PendingDamageEvents};
use mud2::combat::damage_type::DamageType;
use mud2::game::commands::GameCommand;
use mud2::game::resources::GameUiEvent;
use mud2::network::protocol::ClientMessage;
use mud2::npc::components::Npc;
use mud2::player::components::{PlayerId, VitalStats};
use mud2::world::components::{OverworldObject, SpaceId, SpaceResident, TilePosition};
use mud2::world::object_registry::ObjectRegistry;

/// Spawn a minimal killable NPC (no AI — it only exists to die) and return
/// its entity. Shape matches what `DamageTargetQuery` + the NPC death branch
/// in `apply_pending_damage` need.
fn spawn_target_dummy(app: &mut App, space_id: SpaceId, tile: TilePosition) -> Entity {
    let object_id = app
        .world_mut()
        .resource_mut::<ObjectRegistry>()
        .allocate_runtime_id("training_dummy");
    app.world_mut()
        .spawn((
            Npc,
            SpaceResident { space_id },
            tile,
            OverworldObject {
                object_id,
                definition_id: "training_dummy".to_owned(),
                placement_seq: 0,
            },
            VitalStats::full(10.0, 0.0),
        ))
        .id()
}

#[test]
fn party_lifecycle_shared_xp_and_disconnect_reap() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let addr = server_addr(&app);

    // Both spawn at the plaza, well inside each other's interest radius.
    let mut alric = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut alric, "party_alric", "Alric");
    let mut bree = TestClient::connect(addr);
    register_and_enter_world(&mut app, &mut bree, "party_bree", "Bree");

    let alric_id = wait_for_snapshot(&mut app, &mut alric, |s| s.local_player_id.is_some())
        .local_player_id
        .unwrap();
    let bree_id = wait_for_snapshot(&mut app, &mut bree, |s| s.local_player_id.is_some())
        .local_player_id
        .unwrap();

    // Alric resolves Bree's object id from his replicated remote-player view —
    // exactly what the context menu click yields.
    let bree_object_id = wait_for_snapshot(&mut app, &mut alric, |s| {
        s.remote_players.contains_key(&bree_id)
    })
    .remote_players[&bree_id]
        .object_id;

    // Invite → Bree's popup event carries the inviter and a party size of 1
    // (accepting forms a fresh pair).
    bree.ui_events.clear();
    alric.send(ClientMessage::Command(GameCommand::InviteToParty {
        target_object_id: bree_object_id,
    }));
    let events = collect_ui_events(&mut app, &mut bree, 10);
    assert!(
        events.iter().any(|event| matches!(
            event,
            GameUiEvent::PartyInviteReceived {
                from_player_id,
                from_name,
                party_size: 1,
            } if *from_player_id == alric_id && from_name == "Alric"
        )),
        "Bree never received the invite popup event: {events:?}"
    );

    // Accept → both peers fold the same two-member roster with real names.
    bree.send(ClientMessage::Command(GameCommand::AcceptPartyInvite {
        from: alric_id,
    }));
    let alric_view = wait_for_snapshot(&mut app, &mut alric, |s| s.party.is_some());
    let bree_view = wait_for_snapshot(&mut app, &mut bree, |s| s.party.is_some());
    for (who, view) in [("Alric", &alric_view), ("Bree", &bree_view)] {
        let party = view.party.as_ref().unwrap();
        assert_eq!(party.leader, alric_id, "{who}: wrong leader");
        let names: Vec<&str> = party
            .members
            .iter()
            .map(|member| member.display_name.as_str())
            .collect();
        assert_eq!(names, vec!["Alric", "Bree"], "{who}: wrong roster");
        assert!(
            party.members.iter().all(|member| member.in_range),
            "{who}: co-located members should be in range"
        );
        assert!(
            party.members.iter().all(|member| member.share_pct == 50),
            "{who}: equal-level pair should split 50/50: {:?}",
            party.members
        );
        assert!(party.members[0].is_leader && !party.members[1].is_leader);
    }
    // Bree's accepted invite is closed server-side too.
    assert!(
        bree.ui_events
            .iter()
            .any(|event| matches!(event, GameUiEvent::PartyInviteClosed)),
        "accepting should close the invite popup"
    );

    // Credited kill next to both members: base 75 XP (level-1 victim), pool
    // ×1.15 = 86, split evenly between two level-1 members = 43 each.
    let plaza = alric_view.player_tile_position.unwrap();
    let space = alric_view
        .current_space
        .as_ref()
        .expect("current space")
        .space_id;
    let dummy = spawn_target_dummy(&mut app, space, TilePosition::ground(plaza.x + 1, plaza.y));
    app.world_mut()
        .resource_mut::<PendingDamageEvents>()
        .push(DamageEvent {
            target: dummy,
            amount: 1_000_000.0,
            source: DamageSource::Player(alric_id),
            damage_type: DamageType::Blunt,
            vfx_override: None,
            attacker: None,
        });
    let alric_after = wait_for_snapshot(&mut app, &mut alric, |s| {
        s.experience.as_ref().is_some_and(|e| e.current_xp == 43)
    });
    let bree_after = wait_for_snapshot(&mut app, &mut bree, |s| {
        s.experience.as_ref().is_some_and(|e| e.current_xp == 43)
    });
    for (who, view) in [("Alric", &alric_after), ("Bree", &bree_after)] {
        assert!(
            view.chat_log_lines
                .iter()
                .any(|line| line.contains("You gain 43 XP from the party's kill.")),
            "{who} missed the share line: {:?}",
            view.chat_log_lines
        );
        // The killer's solo broadcast is suppressed for partied kills.
        assert!(
            !view
                .chat_log_lines
                .iter()
                .any(|line| line.contains("gained 75 XP")),
            "{who} saw the unsplit solo XP line: {:?}",
            view.chat_log_lines
        );
    }

    // Leave → a two-man party disbands; both peers fold `None`.
    alric.send(ClientMessage::Command(GameCommand::LeaveParty));
    wait_for_snapshot(&mut app, &mut alric, |s| s.party.is_none());
    wait_for_snapshot(&mut app, &mut bree, |s| s.party.is_none());

    // Re-form, then drop Bree's connection: `cleanup_invalid_parties` reaps
    // the party and narrates the disconnect to the survivor.
    alric.send(ClientMessage::Command(GameCommand::InviteToParty {
        target_object_id: bree_object_id,
    }));
    pump(&mut app, &mut bree, 10);
    bree.send(ClientMessage::Command(GameCommand::AcceptPartyInvite {
        from: alric_id,
    }));
    wait_for_snapshot(&mut app, &mut alric, |s| s.party.is_some());
    drop(bree);
    let survivor = wait_for_snapshot(&mut app, &mut alric, |s| s.party.is_none());
    assert!(
        survivor
            .chat_log_lines
            .iter()
            .any(|line| line.contains("left the party (disconnected)")),
        "survivor missed the disconnect narration: {:?}",
        survivor.chat_log_lines
    );

    let _ = (bree_id, PlayerId(0));
}
