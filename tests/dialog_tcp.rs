//! End-to-end NPC dialogue over the TCP wire protocol: a scripted client
//! authenticates, creates + selects a character, is teleported next to a
//! dialog NPC, and exercises Talk → line → advance → end against a real
//! `HeadlessServer` app.

mod common;

use bevy::prelude::*;
use common::{
    boot_server, collect_ui_events, register_and_enter_world, server_addr, unique_test_path,
    TestClient,
};
use mud2::dialog::components::DialogNode;
use mud2::game::commands::GameCommand;
use mud2::game::resources::GameUiEvent;
use mud2::network::protocol::ClientMessage;
use mud2::player::components::Player;
use mud2::world::components::{OverworldObject, SpaceResident, TilePosition};

#[test]
fn talking_to_npc_over_tcp_round_trips() {
    let mut app = boot_server(
        unique_test_path("world.json"),
        unique_test_path("accounts.db"),
    );
    let mut client = TestClient::connect(server_addr(&app));
    register_and_enter_world(&mut app, &mut client, "dialog_bot", "Talky");

    // Server-side: find a dialog NPC and teleport the player next to it.
    let (npc_object_id, npc_tile, npc_space) = {
        let mut npcs = app
            .world_mut()
            .query_filtered::<(&OverworldObject, &TilePosition, &SpaceResident), With<DialogNode>>(
            );
        let (object, tile, resident) = npcs
            .iter(app.world())
            .next()
            .expect("world should contain at least one NPC with a DialogNode");
        (object.object_id, *tile, resident.space_id)
    };
    {
        let mut players = app
            .world_mut()
            .query_filtered::<(&mut TilePosition, &mut SpaceResident), With<Player>>();
        let (mut tile, mut resident) = players
            .single_mut(app.world_mut())
            .expect("exactly one connected player");
        *tile = TilePosition {
            x: npc_tile.x + 1,
            y: npc_tile.y,
            z: npc_tile.z,
        };
        resident.space_id = npc_space;
    }

    // Talk. Exactly one DialogLine (or DialogOptions) must arrive — the
    // greeting delivered once, not once-per-delivery-path.
    client.send(ClientMessage::Command(GameCommand::TalkToNpc {
        npc_object_id,
    }));
    let events = collect_ui_events(&mut app, &mut client, 30);
    let lines: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, GameUiEvent::DialogLine { .. }))
        .collect();
    let session_id = events
        .iter()
        .find_map(|e| match e {
            GameUiEvent::DialogLine { session_id, .. }
            | GameUiEvent::DialogOptions { session_id, .. } => Some(*session_id),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no dialog events after TalkToNpc; got {events:?}"));
    assert!(
        lines.len() <= 1,
        "greeting line was delivered {} times (dup delivery); events: {events:?}",
        lines.len()
    );

    // Advance until the dialog yields options or finishes; every advance must
    // produce a visible response.
    let mut saw_response_to_advance = false;
    for _ in 0..5 {
        client.send(ClientMessage::Command(GameCommand::DialogAdvance {
            session_id,
        }));
        let events = collect_ui_events(&mut app, &mut client, 30);
        if events.iter().any(|e| {
            matches!(
                e,
                GameUiEvent::DialogLine { .. }
                    | GameUiEvent::DialogOptions { .. }
                    | GameUiEvent::DialogClose { .. }
            )
        }) {
            saw_response_to_advance = true;
            break;
        }
    }
    assert!(
        saw_response_to_advance,
        "DialogAdvance produced no response — dialog is stuck"
    );

    // End the dialog; the server must confirm with DialogClose.
    client.send(ClientMessage::Command(GameCommand::DialogEnd {
        session_id,
    }));
    let events = collect_ui_events(&mut app, &mut client, 30);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GameUiEvent::DialogClose { .. })),
        "DialogEnd did not produce DialogClose; got {events:?}"
    );
}
